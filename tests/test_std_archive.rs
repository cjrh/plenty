//! `std.archive`: TAR and ZIP reading and writing, and extraction confined to
//! a new directory. Hostile archives are built here byte by byte; interop is
//! checked against the system `tar` and CPython's `zipfile` when installed.
#![cfg(all(target_os = "linux", target_pointer_width = "64"))]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const PRELUDE: &str = r#"import std.archive as archive
import std.bytes
import std.fs as fs
import std.io as io
import std.path as path
from std.archive import ArchiveError, EntryKind, Method, Policy, Summary
from std.bytes import Bytes
from std.fs import Root
from std.sync import CancellationToken

def report(result: Result[Summary, ArchiveError]) -> Result[(), Failure]:
    match result:
        case Ok(summary):
            print("ok", summary.entries, summary.files, summary.bytes, summary.skipped)?
        case Err(error):
            print(error)?
    Ok(())

def untar(parent: &Root, name: &str, policy: &Policy) -> Result[(), Failure]:
    file = parent.read_file(&path.from_text(name)?)?
    cancel = CancellationToken()?
    mut reader = archive.tar_reader(file)?
    report(archive.extract_tar(&mut reader, parent, &path.from_text("out")?, policy, &cancel))

def unzip(parent: &Root, name: &str, policy: &Policy) -> Result[(), Failure]:
    file = parent.read_file(&path.from_text(name)?)?
    cancel = CancellationToken()?
    match archive.zip_reader(file, policy.max_entries):
        case Ok(opened):
            mut reader = opened
            report(archive.extract_zip(&mut reader, parent, &path.from_text("out")?, policy, &cancel))
        case Err(error):
            print(error)?
            Ok(())

def save(parent: &Root, name: &str, data: Bytes) -> Result[(), Failure]:
    mut source = io.cursor(data)
    cancel = CancellationToken()?
    drop(fs.copy_from(&mut source, parent, &path.from_text(name)?, 1099511627776, &cancel)?)
    Ok(())

def tar_into(parent: &Root, file: io.FileStream, target: &path.Path, policy: &Policy, cancel: &CancellationToken) -> Result[Summary, ArchiveError]:
    mut reader = archive.tar_reader(file)?
    archive.extract_tar(&mut reader, parent, target, policy, cancel)

def zip_into(parent: &Root, file: io.FileStream, target: &path.Path, policy: &Policy, cancel: &CancellationToken) -> Result[Summary, ArchiveError]:
    mut reader = archive.zip_reader(file, policy.max_entries)?
    archive.extract_zip(&mut reader, parent, target, policy, cancel)

def text(value: &str) -> Result[Bytes, Failure]:
    Ok(std.bytes.from_text(value)?)

def name(value: &str) -> Result[path.Path, Failure]:
    Ok(path.from_text(value)?)
"#;

struct Program {
    _directory: tempfile::TempDir,
    executable: PathBuf,
}

impl Program {
    fn new(body: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let entry = directory.path().join("main.plenty");
        let executable = directory.path().join("program");
        let body = body
            .lines()
            .map(|line| format!("    {line}\n"))
            .collect::<String>();
        let source = format!(
            "{PRELUDE}\ndef main() -> Result[(), Failure]:\n    argv = args_bytes()?\n    parent = fs.root(&path.from_bytes(copy(argv[1])?))?\n{body}    Ok(())\n"
        );
        std::fs::write(&entry, &source).unwrap();
        plenty::compile_file_to_executable(&entry, &executable, None)
            .unwrap_or_else(|error| panic!("{error}\n{source}"));
        Self {
            _directory: directory,
            executable,
        }
    }

    /// Runs in `directory` and returns stdout without test markers.
    fn run(&self, directory: &Path) -> String {
        self.run_with(directory, &[])
    }

    fn run_with(&self, directory: &Path, extra: &[&str]) -> String {
        let output = Command::new(&self.executable)
            .arg(directory)
            .args(extra)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.starts_with("__test_"))
            .collect::<Vec<_>>()
            .join("\n")
            .replace("std.archive.ArchiveError.", "")
    }
}

/// Runs a program that extracts `input` (by its extension) with `policy`
/// edits applied, and returns its report and the directory's names.
fn extract(input: &str, data: &[u8], policy: &str) -> (String, Vec<String>) {
    let directory = tempfile::tempdir().unwrap();
    let report = extract_in(directory.path(), input, data, policy);
    (report, listing(directory.path()))
}

fn extract_in(directory: &Path, input: &str, data: &[u8], policy: &str) -> String {
    std::fs::write(directory.join(input), data).unwrap();
    let call = if input.ends_with(".zip") {
        "unzip"
    } else {
        "untar"
    };
    let program = Program::new(&format!(
        "mut policy = archive.policy()\n{policy}\n{call}(&parent, \"{input}\", &policy)?\n"
    ));
    program.run(directory)
}

/// Relative paths under `root`, sorted, directories with a trailing slash.
fn listing(root: &Path) -> Vec<String> {
    fn walk(root: &Path, at: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(at).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative = path.strip_prefix(root).unwrap().to_string_lossy();
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                out.push(format!("{relative}/"));
                walk(root, &path, out);
            } else if kind.is_symlink() {
                out.push(format!("{relative}@"));
            } else {
                out.push(relative.into_owned());
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn octal(field: &mut [u8], value: u64) {
    let text = format!("{value:0width$o}", width = field.len() - 1);
    field[..text.len()].copy_from_slice(text.as_bytes());
}

/// Appends a ustar header for `name` and `data` padded to whole blocks.
fn tar_entry(out: &mut Vec<u8>, name: &[u8], flag: u8, data: &[u8], link: &[u8]) {
    let mut header = [0u8; 512];
    header[..name.len()].copy_from_slice(name);
    octal(&mut header[100..108], 0o644);
    octal(&mut header[108..116], 0);
    octal(&mut header[116..124], 0);
    octal(&mut header[124..136], data.len() as u64);
    octal(&mut header[136..148], 1_700_000_000);
    header[156] = flag;
    header[157..157 + link.len()].copy_from_slice(link);
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    reseal(&mut header);
    out.extend_from_slice(&header);
    out.extend_from_slice(data);
    out.resize(out.len().div_ceil(512) * 512, 0);
}

/// Recomputes a header's checksum after an edit.
fn reseal(header: &mut [u8]) {
    header[148..156].fill(b' ');
    let sum: u32 = header.iter().map(|&b| u32::from(b)).sum();
    octal(&mut header[148..155], u64::from(sum));
}

fn tar_end(out: &mut Vec<u8>) {
    out.resize(out.len() + 1024, 0);
}

fn tar_of(entries: &[(&[u8], u8, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, flag, data) in entries {
        tar_entry(&mut out, name, *flag, data, b"");
    }
    tar_end(&mut out);
    out
}

struct ZipMember<'a> {
    name: &'a [u8],
    data: &'a [u8],
    mode: Option<u32>,
    flags: u16,
}

fn member<'a>(name: &'a [u8], data: &'a [u8]) -> ZipMember<'a> {
    ZipMember {
        name,
        data,
        mode: None,
        flags: 0,
    }
}

/// A stored-method ZIP; `offsets` overrides each central offset when given.
fn zip_of(members: &[ZipMember], offsets: Option<&[u32]>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (index, item) in members.iter().enumerate() {
        let offset = out.len() as u32;
        let crc = crc32(item.data);
        let size = item.data.len() as u32;
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&item.flags.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(item.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(item.name);
        out.extend_from_slice(item.data);
        let (made, external) = match item.mode {
            Some(mode) => (3u16 << 8 | 20, mode << 16),
            None => (20, 0),
        };
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&made.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&item.flags.to_le_bytes());
        central.extend_from_slice(&[0; 6]);
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&(item.name.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0; 8]);
        central.extend_from_slice(&external.to_le_bytes());
        let offset = offsets.map_or(offset, |list| list[index]);
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(item.name);
    }
    let start = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(members.len() as u16).to_le_bytes());
    out.extend_from_slice(&(members.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn tool(command: &str, args: &[&str], directory: &Path) -> Option<Vec<u8>> {
    let output = Command::new(command)
        .args(args)
        .current_dir(directory)
        .output()
        .ok()?;
    assert!(
        output.status.success(),
        "{command} {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Some(output.stdout)
}

#[test]
fn writers_round_trip_through_readers_and_extraction() {
    let directory = tempfile::tempdir().unwrap();
    let program = Program::new(
        r#"
mut tar = archive.tar_writer(io.cursor(std.bytes.empty()?))?
tar.add_dir(&name("docs")?, 493, 1700000000)?
tar.add_file(&name("./docs//readme.txt")?, &text("hello tar")?, 420, 1700000000)?
tar.add_file(&name("a/b/c.txt")?, &text("deep")?, 384, 1700000001)?
tar.finish()?
mut reader = archive.tar_reader(io.cursor(tar.inner.data.copy()?))?
while True:
    match reader.next()?:
        case Some(entry):
            mut out = std.bytes.empty()?
            drop(io.read_up_to(&mut reader, &mut out, 100)?)
            print(decode_utf8(entry.name.as_bytes())?, entry.kind, entry.size, entry.mode, entry.mtime, out.to_text()?)?
        case Nothing:
            break
mut zip = archive.zip_writer(io.cursor(std.bytes.empty()?))?
zip.add_dir(&name("docs")?, 493, 1700000000)?
zip.add_file(&name("docs/readme.txt")?, &text("hello zip hello zip hello zip")?, Method.Deflate, 420, 1700000000)?
zip.add_file(&name("raw.bin")?, &text("stored")?, Method.Stored, 384, 1700000002)?
zip.finish()?
mut entries = archive.zip_reader(io.cursor(zip.inner.data.copy()?), 100)?
for index in range(0, entries.len()):
    entries.open(index)?
    mut out = std.bytes.empty()?
    drop(io.read_up_to(&mut entries, &mut out, 100)?)
    listed = entries.entries()
    entry = &listed[index]
    print(decode_utf8(entry.name.as_bytes())?, entry.kind, entry.size, entry.mode, entry.mtime, out.to_text()?)?
cancel = CancellationToken()?
policy = archive.policy()
mut again = archive.tar_reader(io.cursor(tar.inner.data.copy()?))?
report(archive.extract_tar(&mut again, &parent, &name("tree")?, &policy, &cancel))?
report(archive.extract_zip(&mut entries, &parent, &name("zipped")?, &policy, &cancel))?
report(archive.extract_zip(&mut entries, &parent, &name("zipped")?, &policy, &cancel))?
"#,
    );
    let out = program.run(directory.path());
    assert_eq!(
        out,
        "docs/ EntryKind.Directory 0 493 1700000000 \n\
         docs/readme.txt EntryKind.File 9 420 1700000000 hello tar\n\
         a/b/c.txt EntryKind.File 4 384 1700000001 deep\n\
         docs/ EntryKind.Directory 0 493 1700000000 \n\
         docs/readme.txt EntryKind.File 29 420 1700000000 hello zip hello zip hello zip\n\
         raw.bin EntryKind.File 6 384 1700000002 stored\n\
         ok 3 2 13 0\n\
         ok 3 2 35 0\n\
         Exists"
            .replace("EntryKind.", "std.archive.EntryKind.")
    );
    assert_eq!(
        listing(directory.path()),
        [
            "tree/",
            "tree/a/",
            "tree/a/b/",
            "tree/a/b/c.txt",
            "tree/docs/",
            "tree/docs/readme.txt",
            "zipped/",
            "zipped/docs/",
            "zipped/docs/readme.txt",
            "zipped/raw.bin",
        ]
    );
    assert_eq!(
        std::fs::read(directory.path().join("zipped/docs/readme.txt")).unwrap(),
        b"hello zip hello zip hello zip"
    );
}

#[test]
fn system_tar_reads_our_archives_and_we_read_its_archives() {
    let directory = tempfile::tempdir().unwrap();
    let long = format!("{}/{}", "d".repeat(80), "f".repeat(120));
    let program = Program::new(&format!(
        r#"
mut tar = archive.tar_writer(io.cursor(std.bytes.empty()?))?
tar.add_file(&name("{long}")?, &text("long name")?, 420, 1700000000)?
tar.add_dir(&name("empty")?, 493, 1700000000)?
tar.add_symlink(&name("link")?, &name("{long}")?, 1700000000)?
tar.finish()?
save(&parent, "ours.tar", tar.inner.data.copy()?)?
"#
    ));
    program.run(directory.path());
    let Some(listed) = tool("tar", &["-tf", "ours.tar"], directory.path()) else {
        eprintln!("skipped: tar is not installed");
        return;
    };
    assert_eq!(
        String::from_utf8(listed).unwrap(),
        format!("{long}\nempty/\nlink\n")
    );
    std::fs::create_dir(directory.path().join("x")).unwrap();
    tool("tar", &["-xf", "../ours.tar"], &directory.path().join("x")).unwrap();
    assert_eq!(
        std::fs::read(directory.path().join("x").join(&long)).unwrap(),
        b"long name"
    );
    assert_eq!(
        std::fs::read_link(directory.path().join("x/link"))
            .unwrap()
            .to_string_lossy(),
        long
    );

    for format in ["gnu", "pax", "ustar"] {
        let source = tempfile::tempdir().unwrap();
        let nested = source.path().join("tree/sub");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("small.txt"), b"small").unwrap();
        let big: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(source.path().join("tree/big.bin"), &big).unwrap();
        // ustar splits names into a 155-byte prefix and a 100-byte name.
        let name = if format == "ustar" {
            format!("{}/{}", "p".repeat(90), "n".repeat(90))
        } else {
            format!("{}/{}", "p".repeat(120), "n".repeat(150))
        };
        std::fs::create_dir_all(source.path().join("tree").join(&name).parent().unwrap()).unwrap();
        std::fs::write(source.path().join("tree").join(&name), b"named").unwrap();
        let archive = source.path().join("system.tar");
        tool(
            "tar",
            &[
                &format!("--format={format}"),
                "-cf",
                archive.to_str().unwrap(),
                "tree",
            ],
            source.path(),
        )
        .unwrap();
        let out = tempfile::tempdir().unwrap();
        let report = extract_in(
            out.path(),
            "system.tar",
            &std::fs::read(&archive).unwrap(),
            "",
        );
        assert_eq!(report, "ok 6 3 100010 0", "{format}");
        assert_eq!(
            std::fs::read(out.path().join("out/tree/sub/small.txt")).unwrap(),
            b"small"
        );
        assert_eq!(
            std::fs::read(out.path().join("out/tree/big.bin")).unwrap(),
            big
        );
        assert_eq!(
            std::fs::read(out.path().join("out/tree").join(&name)).unwrap(),
            b"named",
            "{format}"
        );
    }
}

const PYTHON_ZIP: &str = r#"
import sys, zipfile
with zipfile.ZipFile(sys.argv[1], "w") as z:
    z.writestr("stored.txt", b"stored data", zipfile.ZIP_STORED)
    z.writestr("dir/deflated.txt", b"deflated " * 1000, zipfile.ZIP_DEFLATED)
    z.writestr("dir/", b"")
    z.writestr("café.txt", b"utf8 name")
    with z.open("wide.bin", "w", force_zip64=True) as f:
        f.write(bytes(range(256)) * 40)
    z.comment = b"a trailing comment"
"#;

const PYTHON_CHECK: &str = r#"
import sys, zipfile
with zipfile.ZipFile(sys.argv[1]) as z:
    assert z.testzip() is None
    for info in z.infolist():
        print(info.filename, info.file_size, info.compress_type, oct(info.external_attr >> 16), info.date_time[0])
    print(z.read("deep/file.txt").decode())
"#;

#[test]
fn python_zipfile_reads_our_archives_and_we_read_its_archives() {
    let directory = tempfile::tempdir().unwrap();
    let archive = directory.path().join("python.zip");
    let Some(_) = tool(
        "python3",
        &["-I", "-c", PYTHON_ZIP, archive.to_str().unwrap()],
        directory.path(),
    ) else {
        eprintln!("skipped: python3 is not installed");
        return;
    };
    let (report, names) = extract("python.zip", &std::fs::read(&archive).unwrap(), "");
    assert_eq!(report, "ok 5 4 19260 0");
    assert_eq!(
        names,
        [
            "out/",
            "out/café.txt",
            "out/dir/",
            "out/dir/deflated.txt",
            "out/stored.txt",
            "out/wide.bin",
            "python.zip",
        ]
    );

    let program = Program::new(
        r#"
mut zip = archive.zip_writer(io.cursor(std.bytes.empty()?))?
zip.add_dir(&name("deep")?, 493, 1700000000)?
zip.add_file(&name("deep/file.txt")?, &text("from plenty")?, Method.Deflate, 420, 1700000000)?
zip.add_file(&name("old.txt")?, &text("clamped")?, Method.Stored, 384, 0)?
zip.add_symlink(&name("link")?, &name("deep/file.txt")?, 1700000000)?
zip.finish()?
save(&parent, "ours.zip", zip.inner.data.copy()?)?
"#,
    );
    program.run(directory.path());
    let ours = directory.path().join("ours.zip");
    let checked = tool(
        "python3",
        &["-I", "-c", PYTHON_CHECK, ours.to_str().unwrap()],
        directory.path(),
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(checked).unwrap(),
        "deep/ 0 0 0o40755 2023\n\
         deep/file.txt 11 8 0o100644 2023\n\
         old.txt 7 0 0o100600 1980\n\
         link 13 0 0o120777 2023\n\
         from plenty\n"
    );
    if let Ok(tested) = Command::new("unzip")
        .args(["-t", ours.to_str().unwrap()])
        .output()
    {
        assert!(tested.status.success(), "{tested:?}");
    }
}

#[test]
fn zip64_entry_count_is_written_and_read() {
    // 65536 entries need the ZIP64 end records: the 16-bit count saturates.
    let directory = tempfile::tempdir().unwrap();
    let program = Program::new(
        r#"
mut zip = archive.zip_writer(io.cursor(std.bytes.empty()?))?
empty = std.bytes.empty()?
for index in range(0, 65536):
    zip.add_file(&path.from_text(&str.from(index)?)?, &empty, Method.Stored, 420, 1700000000)?
zip.finish()?
mut reader = archive.zip_reader(io.cursor(zip.inner.data.copy()?), 70000)?
listed = reader.entries()
print(reader.len(), decode_utf8(listed[65535].name.as_bytes())?)?
match archive.zip_reader(io.cursor(zip.inner.data.copy()?), 65535):
    case Ok(_):
        print("accepted")?
    case Err(error):
        print(error)?
save(&parent, "many.zip", zip.inner.data.copy()?)?
"#,
    );
    assert_eq!(program.run(directory.path()), "65536 65535\nLimit");
    let many = directory.path().join("many.zip");
    if let Some(count) = tool(
        "python3",
        &[
            "-I",
            "-c",
            "import sys, zipfile; print(len(zipfile.ZipFile(sys.argv[1]).infolist()))",
            many.to_str().unwrap(),
        ],
        directory.path(),
    ) {
        assert_eq!(count, b"65536\n");
    }
}

#[test]
fn unsafe_names_are_rejected_and_leave_nothing() {
    for bad in [
        &b"../evil"[..],
        b"/etc/passwd",
        b"a/../../b",
        b"a/..",
        b"a\\b",
        b".",
    ] {
        let tar = tar_of(&[(b"d1/d2/ok.txt", b'0', b"fine"), (bad, b'0', b"bad")]);
        let (report, names) = extract("in.tar", &tar, "");
        assert_eq!(report, "UnsafePath", "{}", bad.escape_ascii());
        assert_eq!(names, ["in.tar"]);
        let zip = zip_of(
            &[member(b"d1/d2/ok.txt", b"fine"), member(bad, b"bad")],
            None,
        );
        let (report, names) = extract("in.zip", &zip, "");
        assert_eq!(report, "UnsafePath", "{}", bad.escape_ascii());
        assert_eq!(names, ["in.zip"]);
    }
    let program = Program::new(
        r#"
mut tar = archive.tar_writer(io.cursor(std.bytes.empty()?))?
print(str.repr(tar.add_file(&name("../x")?, &text("x")?, 420, 0))?)?
print(str.repr(tar.add_file(&name("/x")?, &text("x")?, 420, 0))?)?
print(str.repr(tar.add_file(&name("x")?, &text("x")?, 4096, 0))?)?
tar.finish()?
print(str.repr(tar.add_dir(&name("late")?, 493, 0))?)?
mut zip = archive.zip_writer(io.cursor(std.bytes.empty()?))?
print(str.repr(zip.add_file(&name("a/../b")?, &text("x")?, Method.Stored, 420, 0))?)?
"#,
    );
    let directory = tempfile::tempdir().unwrap();
    assert_eq!(
        program
            .run(directory.path())
            .replace("Result[(), std.archive.ArchiveError].", ""),
        "Err(UnsafePath)\nErr(UnsafePath)\nErr(InvalidInput)\nErr(InvalidInput)\nErr(UnsafePath)"
    );
}

#[test]
fn links_and_special_files_are_refused_unless_skipped() {
    let mut tar = Vec::new();
    tar_entry(&mut tar, b"file.txt", b'0', b"data", b"");
    tar_entry(&mut tar, b"soft", b'2', b"", b"/etc/passwd");
    tar_entry(&mut tar, b"hard", b'1', b"", b"file.txt");
    tar_entry(&mut tar, b"pipe", b'6', b"", b"");
    tar_end(&mut tar);
    let (report, names) = extract("in.tar", &tar, "");
    assert_eq!(report, "Link");
    assert_eq!(names, ["in.tar"]);
    let (report, _) = extract("in.tar", &tar, "policy.skip_links = True");
    assert_eq!(report, "Special");
    let (report, names) = extract(
        "in.tar",
        &tar,
        "policy.skip_links = True\npolicy.skip_special = True",
    );
    assert_eq!(report, "ok 4 1 4 3");
    assert_eq!(names, ["in.tar", "out/", "out/file.txt"]);

    let mut link = member(b"soft", b"/etc/passwd");
    link.mode = Some(0o120777);
    let zip = zip_of(&[member(b"file.txt", b"data"), link], None);
    let (report, _) = extract("in.zip", &zip, "");
    assert_eq!(report, "Link");
    let (report, names) = extract("in.zip", &zip, "policy.skip_links = True");
    assert_eq!(report, "ok 2 1 4 1");
    assert_eq!(names, ["in.zip", "out/", "out/file.txt"]);
}

#[test]
fn limits_stop_extraction_before_writing() {
    let tar = tar_of(&[(b"a.txt", b'0', b"12345"), (b"b/c/d.txt", b'0', b"123")]);
    for (policy, expected) in [
        ("policy.max_entries = 1", "Limit"),
        ("policy.max_entry_size = 4", "Limit"),
        ("policy.max_total_size = 7", "Limit"),
        ("policy.max_depth = 2", "Limit"),
        ("policy.max_name = 8", "Limit"),
        ("policy.max_total_size = 8", "ok 2 2 8 0"),
        ("policy.max_depth = 3", "ok 2 2 8 0"),
        ("policy.max_depth = 0", "InvalidInput"),
        ("policy.max_ratio = 0", "InvalidInput"),
    ] {
        let (report, names) = extract("in.tar", &tar, policy);
        assert_eq!(report, expected, "{policy}");
        if expected == "Limit" {
            assert_eq!(names, ["in.tar"]);
        }
    }
    let zip = zip_of(&[member(b"a.txt", b"12345")], None);
    let (report, _) = extract("in.zip", &zip, "policy.max_entries = 0");
    assert_eq!(report, "Limit");

    // A 4 MiB run of zeros deflates about a thousandfold.
    let directory = tempfile::tempdir().unwrap();
    let program = Program::new(
        r#"
mut zeros = std.bytes.with_capacity(4194304)?
zeros.resize(4194304, 0u8)?
mut zip = archive.zip_writer(io.cursor(std.bytes.empty()?))?
zip.add_file(&name("bomb")?, &zeros, Method.Deflate, 420, 0)?
zip.finish()?
save(&parent, "bomb.zip", zip.inner.data.copy()?)?
mut policy = archive.policy()
unzip(&parent, "bomb.zip", &policy)?
policy.max_ratio = 10000
unzip(&parent, "bomb.zip", &policy)?
"#,
    );
    assert_eq!(program.run(directory.path()), "Limit\nok 1 1 4194304 0");
}

#[test]
fn corrupt_archives_are_reported_and_leave_nothing() {
    let tar = tar_of(&[(b"a.txt", b'0', b"12345"), (b"b.txt", b'0', b"678")]);
    let mut bad_sum = tar.clone();
    bad_sum[0] = b'z';
    let mut bad_number = tar.clone();
    bad_number[124] = b'9';
    reseal(&mut bad_number[..512]);
    let duplicate = tar_of(&[(b"a.txt", b'0', b"1"), (b"a.txt", b'0', b"2")]);
    let shadowed = tar_of(&[(b"a", b'0', b"1"), (b"a/b", b'0', b"2")]);
    // A global pax header whose base-256 size is i64::MAX: its padding
    // must not overflow (issue #241).
    let mut huge_global = tar_of(&[(b"g", b'g', b""), (b"a.txt", b'0', b"1")]);
    huge_global[124..136].fill(0);
    huge_global[124] = 0x80;
    huge_global[128..136].copy_from_slice(&i64::MAX.to_be_bytes());
    reseal(&mut huge_global[..512]);
    for (data, expected) in [
        (&tar[..600], "Truncated"),
        (&tar[..1536], "Truncated"),
        (&tar[..2048], "Truncated"),
        (&bad_sum[..], "Checksum"),
        (&bad_number[..], "Corrupt"),
        (&duplicate[..], "Duplicate"),
        (&shadowed[..], "Duplicate"),
        (&huge_global[..], "Corrupt"),
    ] {
        let (report, names) = extract("in.tar", data, "");
        assert_eq!(report, expected);
        assert_eq!(names, ["in.tar"]);
    }

    let zip = zip_of(
        &[member(b"a.txt", b"12345"), member(b"b.txt", b"678")],
        None,
    );
    let mut bad_crc = zip.clone();
    bad_crc[30 + 5] ^= 1;
    let overlap = zip_of(
        &[member(b"a.txt", b"12345"), member(b"a.txt", b"12345")],
        Some(&[0, 0]),
    );
    let mut encrypted = member(b"a.txt", b"12345");
    encrypted.flags = 1;
    let encrypted = zip_of(&[encrypted], None);
    let mut lying = zip.clone();
    lying[30] = b'x';
    for (data, expected) in [
        (&zip[..zip.len() - 1], "Corrupt"),
        (&zip[..40], "Corrupt"),
        (&bad_crc[..], "Checksum"),
        (&overlap[..], "Corrupt"),
        (&encrypted[..], "Unsupported"),
        (&lying[..], "Corrupt"),
    ] {
        let (report, names) = extract("in.zip", data, "");
        assert_eq!(report, expected);
        assert_eq!(names, ["in.zip"]);
    }
}

#[test]
fn readers_report_entry_errors_through_the_reader_protocol() {
    let directory = tempfile::tempdir().unwrap();
    let zip = zip_of(&[member(b"a.txt", b"12345")], None);
    let mut bad_crc = zip.clone();
    bad_crc[30 + 5] ^= 1;
    std::fs::write(directory.path().join("bad.zip"), &bad_crc).unwrap();
    let tar = tar_of(&[(b"a.txt", b'0', b"12345")]);
    std::fs::write(directory.path().join("cut.tar"), &tar[..515]).unwrap();
    let program = Program::new(
        r#"
mut zip = archive.zip_reader(parent.read_file(&name("bad.zip")?)?, 10)?
zip.open(0)?
mut out = std.bytes.empty()?
print(str.repr(io.read_up_to(&mut zip, &mut out, 100))?)?
print(str.repr(zip.failure())?)?
print(str.repr(io.read_up_to(&mut zip, &mut out, 100))?)?
print(str.repr(zip.open(4))?)?
mut tar = archive.tar_reader(parent.read_file(&name("cut.tar")?)?)?
drop(tar.next()?)
print(str.repr(io.read_up_to(&mut tar, &mut out, 100))?)?
print(str.repr(tar.failure())?)?
print(str.repr(tar.next())?)?
"#,
    );
    let out = program.run(directory.path());
    // Content is delivered as it is read; the CRC fails at the end.
    assert_eq!(
        out.replace("std.archive.", "")
            .replace("std.io.", "")
            .replace("StreamError.", "")
            .replace("IoError.", "")
            .replace("Result[i64, Partial].", "")
            .replace("Option[ArchiveError].", ""),
        "Err(Partial(completed=5, cause=Io(InvalidInput)))\n\
         Some(Checksum)\n\
         Err(Partial(completed=0, cause=Io(InvalidInput)))\n\
         Result[(), ArchiveError].Err(InvalidInput)\n\
         Err(Partial(completed=3, cause=UnexpectedEof))\n\
         Some(Truncated)\n\
         Result[Option[Entry], ArchiveError].Err(Truncated)"
    );
}

#[test]
fn destination_cancellation_and_raw_names() {
    let directory = tempfile::tempdir().unwrap();
    let tar = tar_of(&[(b"raw\xff", b'0', b"bytes")]);
    std::fs::write(directory.path().join("raw.tar"), &tar).unwrap();
    std::fs::create_dir(directory.path().join("taken")).unwrap();
    let program = Program::new(
        r#"
policy = archive.policy()
cancel = CancellationToken()?
mut first = archive.tar_reader(parent.read_file(&name("raw.tar")?)?)?
report(archive.extract_tar(&mut first, &parent, &name("taken")?, &policy, &cancel))?
mut second = archive.tar_reader(parent.read_file(&name("raw.tar")?)?)?
report(archive.extract_tar(&mut second, &parent, &name("a/b")?, &policy, &cancel))?
mut third = archive.tar_reader(parent.read_file(&name("raw.tar")?)?)?
report(archive.extract_tar(&mut third, &parent, &name("fresh")?, &policy, &cancel))?
cancel.cancel()
mut fourth = archive.tar_reader(parent.read_file(&name("raw.tar")?)?)?
report(archive.extract_tar(&mut fourth, &parent, &name("cancelled")?, &policy, &cancel))?
"#,
    );
    assert_eq!(
        program.run(directory.path()),
        "Exists\nInvalidInput\nok 1 1 5 0\nCancelled"
    );
    let names: Vec<Vec<u8>> = std::fs::read_dir(directory.path().join("fresh"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().as_bytes().to_vec())
        .collect();
    assert_eq!(names, [b"raw\xff".to_vec()]);
    assert_eq!(
        listing(directory.path()),
        ["fresh/", "fresh/raw\u{fffd}", "raw.tar", "taken/"]
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_during_extraction_is_reported_without_leaks() {
    // Every failure point reports Allocation (or the run completes), and the
    // exit-time balance check verifies that cleanup leaks nothing.
    let directory = tempfile::tempdir().unwrap();
    let tar = tar_of(&[(b"a/b.txt", b'0', b"12345"), (b"c.txt", b'0', b"678")]);
    std::fs::write(directory.path().join("in.tar"), &tar).unwrap();
    let zip = zip_of(
        &[member(b"a/b.txt", b"12345"), member(b"c.txt", b"678")],
        None,
    );
    std::fs::write(directory.path().join("in.zip"), &zip).unwrap();
    let program = Program::new(
        r#"
marker = decode_utf8(&argv[2])?
policy = archive.policy()
cancel = CancellationToken()?
tar_file = parent.read_file(&name("in.tar")?)?
zip_file = parent.read_file(&name("in.zip")?)?
target_tar = path.from_bytes(copy(argv[3])?)
target_zip = path.from_bytes(copy(argv[4])?)
print(marker)?
first = tar_into(&parent, tar_file, &target_tar, &policy, &cancel)
second = zip_into(&parent, zip_file, &target_zip, &policy, &cancel)
print("__test_restore_allocations__")?
report(first)?
report(second)?
"#,
    );
    let mut seen_ok = false;
    for after in 0..200 {
        let marker = format!("__test_fail_allocations_after_{after}__");
        let out = program.run_with(
            directory.path(),
            &[&marker, &format!("t{after}"), &format!("z{after}")],
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "after {after}: {out}");
        for (line, target) in lines.iter().zip(["t", "z"]) {
            assert!(
                *line == "ok 2 2 8 0" || *line == "Allocation(AllocError.OutOfMemory)",
                "after {after}: {out}"
            );
            // A destination appears only for a complete extraction. Removing
            // a failed staging tree itself allocates, so a run still out of
            // memory during cleanup may leave a `.plenty-extract-` directory.
            let created = directory.path().join(format!("{target}{after}")).exists();
            assert_eq!(created, line.starts_with("ok"), "after {after}: {out}");
        }
        if out == "ok 2 2 8 0\nok 2 2 8 0" {
            seen_ok = true;
            break;
        }
    }
    assert!(seen_ok);
}
