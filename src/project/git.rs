//! Git dependency sources, fetched with the system `git` binary.
//!
//! `.pkg/git/db/KEY` is a bare repository holding everything fetched from one
//! URL. `.pkg/git/checkouts/KEY/COMMIT` is a snapshot of one commit's tree,
//! written beside its final path and renamed into place, so an interrupted
//! fetch never leaves a partial snapshot that later builds would trust.
//! Snapshots are not write-protected: an existing snapshot is reused as is, so
//! edits to its files persist until it is deleted. `KEY` combines a readable
//! repository name with a hash of the URL.
use super::manifest::GitSelector;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

type Result<T> = std::result::Result<T, String>;

/// Fetches and snapshots repositories below one `.pkg` directory.
pub struct Cache {
    directory: PathBuf,
    offline: bool,
    /// URLs already fetched by this command, so a graph that names one
    /// repository many times contacts it once.
    fetched: HashSet<String>,
}

impl Cache {
    pub fn new(directory: PathBuf, offline: bool) -> Cache {
        Cache {
            directory,
            offline,
            fetched: HashSet::new(),
        }
    }

    fn key(url: &str) -> String {
        let name: String = url
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .rsplit(['/', ':', '\\'])
            .next()
            .unwrap_or("")
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            .take(40)
            .collect();
        let hash = Sha256::digest(url.as_bytes());
        let hex: String = hash[..8].iter().map(|b| format!("{b:02x}")).collect();
        if name.is_empty() || name.starts_with('.') {
            hex
        } else {
            format!("{name}-{hex}")
        }
    }

    fn database(&self, url: &str) -> PathBuf {
        self.directory.join("git/db").join(Self::key(url))
    }

    /// The commit that `selector` names in `url`, fetching first unless
    /// offline. Offline resolution uses what an earlier fetch stored.
    pub fn resolve(&mut self, url: &str, selector: &GitSelector) -> Result<String> {
        let database = self.database(url);
        if self.offline {
            if !database.is_dir() {
                return Err(format!(
                    "{url} is not in the package cache, and --offline forbids fetching it"
                ));
            }
        } else {
            self.fetch(url)?;
        }
        let spec = match selector {
            GitSelector::Tag(tag) => format!("refs/tags/{tag}^{{commit}}"),
            GitSelector::Rev(rev) => format!("{rev}^{{commit}}"),
        };
        let output = git(&database)
            .args([
                "rev-parse",
                "--verify",
                "--quiet",
                "--end-of-options",
                &spec,
            ])
            .output()
            .map_err(spawn_error)?;
        let commit = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !output.status.success() || commit.len() != 40 {
            let what = match selector {
                GitSelector::Tag(tag) => format!("tag `{tag}`"),
                GitSelector::Rev(rev) => format!("commit {rev}"),
            };
            let place = if self.offline {
                " in the package cache (--offline)"
            } else {
                ""
            };
            return Err(format!("{url} has no {what}{place}"));
        }
        Ok(commit)
    }

    /// The snapshot directory of `commit`, creating it if needed.
    pub fn checkout(&mut self, url: &str, commit: &str) -> Result<PathBuf> {
        let parent = self.directory.join("git/checkouts").join(Self::key(url));
        let snapshot = parent.join(commit);
        if snapshot.is_dir() {
            return Ok(snapshot);
        }
        let database = self.database(url);
        if !self.has_commit(&database, commit) {
            if self.offline {
                return Err(format!(
                    "commit {commit} of {url} is not in the package cache, and --offline forbids fetching it"
                ));
            }
            self.fetch(url)?;
            if !self.has_commit(&database, commit) {
                return Err(format!(
                    "the locked commit {commit} is no longer available from {url}; `plenty update` selects a current one"
                ));
            }
        }
        std::fs::create_dir_all(&parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        let staging = tempfile::tempdir_in(&parent).map_err(|e| e.to_string())?;
        let tree = staging.path().join("tree");
        std::fs::create_dir(&tree).map_err(|e| e.to_string())?;
        // A private index keeps the shared database untouched by checkouts.
        let index = staging.path().join("index");
        for args in [
            &["read-tree", "--end-of-options", commit][..],
            &["checkout-index", "--all", "--force"][..],
        ] {
            run(git(&database)
                .env("GIT_INDEX_FILE", &index)
                .arg("--work-tree")
                .arg(&tree)
                .args(args))
            .map_err(|e| format!("extracting {commit} of {url}: {e}"))?;
        }
        match std::fs::rename(&tree, &snapshot) {
            Ok(()) => {}
            // Another command finished the same snapshot first.
            Err(_) if snapshot.is_dir() => {}
            Err(e) => return Err(format!("{}: {e}", snapshot.display())),
        }
        Ok(snapshot)
    }

    fn has_commit(&self, database: &Path, commit: &str) -> bool {
        database.is_dir()
            && git(database)
                .args(["cat-file", "-e", "--end-of-options"])
                .arg(format!("{commit}^{{commit}}"))
                .output()
                .is_ok_and(|o| o.status.success())
    }

    /// Mirror every branch and tag of `url`. Forced updates let a moved tag
    /// move here too; locked builds never consult tags, only commits.
    fn fetch(&mut self, url: &str) -> Result<()> {
        if self.fetched.contains(url) {
            return Ok(());
        }
        let database = self.database(url);
        if !database.is_dir() {
            let parent = database.parent().unwrap();
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            let staging = tempfile::tempdir_in(parent).map_err(|e| e.to_string())?;
            let bare = staging.path().join("db");
            run(Command::new("git")
                .args(["init", "--quiet", "--bare"])
                .arg(&bare))?;
            if std::fs::rename(&bare, &database).is_err() && !database.is_dir() {
                return Err(format!("could not create {}", database.display()));
            }
        }
        eprintln!("{:>10} {url}", "Fetching");
        run(git(&database).args([
            "fetch",
            "--quiet",
            "--force",
            "--no-write-fetch-head",
            "--end-of-options",
            url,
            "+refs/heads/*:refs/heads/*",
            "+refs/tags/*:refs/tags/*",
        ]))
        .map_err(|e| format!("fetching {url}: {e}"))?;
        self.fetched.insert(url.to_owned());
        Ok(())
    }
}

fn git(database: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("--git-dir")
        .arg(database)
        // Never wait for credentials on a terminal the user is not watching.
        .env("GIT_TERMINAL_PROMPT", "0");
    command
}

fn spawn_error(error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        "git dependencies need the `git` program on PATH".into()
    } else {
        format!("running git: {error}")
    }
}

fn run(command: &mut Command) -> Result<()> {
    let output = command.output().map_err(spawn_error)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_readable_and_distinct() {
        let a = Cache::key("https://example.com/media.git");
        let b = Cache::key("https://example.org/media.git");
        assert!(a.starts_with("media-"), "{a}");
        assert_ne!(a, b);
        assert!(!Cache::key("file:///..").contains('/'));
    }
}
