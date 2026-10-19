# Setting up

Install Plenty once, then use the `plenty` command throughout this tutorial.
You do not need Rust, Cargo, or a copy of the compiler's source repository.

## Check your system

The current binary release supports **x86-64 Linux with glibc 2.28 or later**.
There are no release binaries for macOS, Windows, ARM, or musl-based Linux
distributions such as Alpine yet. See the
[installation requirements](../design/toolchain/01-building/installing.md#requirements)
for supported systems and troubleshooting.

Plenty also needs a C linker and development libraries to run or compile a
program. Install the packages for your distribution before continuing:

```sh
# Debian or Ubuntu
sudo apt-get update && sudo apt-get install gcc libc6-dev

# Fedora, RHEL, AlmaLinux, or Rocky Linux
sudo dnf install gcc tar gzip

# Arch Linux
sudo pacman -Syu --needed gcc
```

## Download and extract Plenty

Open [Plenty's GitHub Releases](https://github.com/cjrh/plenty/releases) and
choose a published release. Under **Assets**, download both:

- `plenty-VERSION-x86_64-unknown-linux-gnu.tar.gz`
- `plenty-VERSION-x86_64-unknown-linux-gnu.tar.gz.sha256`

Choose the binary archive, rather than GitHub's **Source code (zip)** download.
Plenty currently ships as a compressed tar archive, so extract it with `tar`.
In every command below, replace `VERSION` with the version you downloaded;
keep the same version in the archive name and directory name.

Assuming both files are in `~/Downloads`, verify and extract them:

```sh
cd "$HOME/Downloads"
sha256sum -c plenty-VERSION-x86_64-unknown-linux-gnu.tar.gz.sha256
mkdir -p "$HOME/.local/opt"
tar -xzf plenty-VERSION-x86_64-unknown-linux-gnu.tar.gz -C "$HOME/.local/opt"
```

Check that the checksum command reports `OK` before extracting. The resulting
directory contains `bin/plenty` and `bin/plentyc`. Keep the extracted directory;
the shell setup below points to its `bin` directory.

## Add Plenty to your PATH

`PATH` is the list of directories your shell searches for commands. Choose the
instructions for your shell. These paths are quoted so they also work when
your home directory contains spaces.

### Bash

Add this line to `~/.bashrc`, replacing `VERSION`:

```bash
export PATH="$HOME/.local/opt/plenty-VERSION-x86_64-unknown-linux-gnu/bin:$PATH"
```

Run that same line in your current terminal to use Plenty immediately. New
interactive terminals read `.bashrc`. If you use a Bash login shell, ensure
your login startup file also sources `.bashrc`; see
[Bash startup files](https://www.gnu.org/software/bash/manual/html_node/Bash-Startup-Files.html).

### Zsh

Add this line to `~/.zshrc`, replacing `VERSION`:

```zsh
export PATH="$HOME/.local/opt/plenty-VERSION-x86_64-unknown-linux-gnu/bin:$PATH"
```

Run that same line in your current terminal. New interactive Zsh terminals
read `.zshrc`. If you configured `ZDOTDIR`, use `$ZDOTDIR/.zshrc` instead;
see [Zsh startup files](https://zsh.sourceforge.io/Doc/Release/Files.html).

### Fish

Run this command once in Fish, replacing `VERSION`:

```fish
fish_add_path --universal --prepend --move "$HOME/.local/opt/plenty-VERSION-x86_64-unknown-linux-gnu/bin"
```

[`fish_add_path`](https://fishshell.com/docs/current/cmds/fish_add_path.html)
updates the current shell and remembers the directory for future Fish sessions.
The directory must already exist, so extract the archive first. There is no
need to edit `config.fish` for this universal setting.

## Check the installation

In any of these shells, run:

```sh
command -v plenty
plenty --version
cc --version
```

The first command should name `plenty` inside your extracted archive; the second
prints the installed version and target. If `plenty` is not found, check that
the PATH entry matches the actual extracted directory. If another installation
appears, put the new directory first on PATH. If `cc` is missing, install the
linking packages above.

You are ready for [Your first program](01-first-program/index.md). Save your
programs in any working directory you choose. When installing a newer release,
update the PATH entry to its new directory; see
[upgrading and switching versions](../design/toolchain/01-building/installing.md#upgrade-and-switch-versions).
