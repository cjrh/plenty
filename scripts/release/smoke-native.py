#!/usr/bin/env python3
"""Check an experimental macOS or Windows release archive as a user would install it.

Usage: smoke-native.py ARCHIVE

The macOS and Windows counterpart of smoke-test.sh, run on a fresh GitHub
runner with no source build. ARCHIVE.sha256 must sit next to ARCHIVE. The
script checks the checksum, layout, and modes; extracts into a directory
whose path contains spaces; and, with only its bin/ and the system
directories on PATH and no Rust toolchain reachable, reports both versions,
type-checks, compiles, and runs a program, and runs it again through
`plenty FILE`. It then moves and renames the installation, extracts a second
copy, repeats, and requires the installation to be unchanged.

The program is the one native CI qualifies on these platforms (tagged results,
callbacks, generators, printing). Standard-library services such as SQLite,
TLS, and processes, project commands, and the missing-linker diagnostic are
not yet checked here.
"""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import zipfile

REQUIRED = ("README.md", "LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-NOTICES.md",
            "licenses/rust/COPYRIGHT-library.html")
TARGETS = {"aarch64-apple-darwin": ("tar.gz", ""), "x86_64-pc-windows-msvc": ("zip", ".exe")}
# Same program and output as codegen.rs native_runtime_tagged_results_and_callbacks_round_trip.
PROGRAM = '''def key(value: &i64) -> i64:
    -*value

def count(stop: i64) -> Generator[i64]:
    for value in range(stop):
        yield value

def main() -> Result[(), AllocError | IoError | ParseError]:
    number = i64.parse("9223372036854775806")?
    mut values = [number, -7, 42]?
    values.sort_by_key(key)
    print(values)?
    print(str.repr(i64.parse("bad"))?)?
    for value in count(3):
        print(value)?
    Ok(())
'''
EXPECTED = "[9223372036854775806, 42, -7]\nResult[i64, ParseError].Err(ParseError.Invalid)\n0\n1\n2\n"
RUST_TOOLS = ("cargo", "rustc", "rustup")


def fail(message):
    raise SystemExit(f"smoke-native: FAIL: {message}")


def step(message):
    print(f"smoke-native: {message}", flush=True)


def parse_name(archive):
    for target, (ext, exe) in TARGETS.items():
        suffix = f"-{target}.{ext}"
        if archive.name.startswith("plenty-") and archive.name.endswith(suffix):
            name = archive.name[:-len(ext) - 1]
            return name, name[len("plenty-"):-len(target) - 1], target, ext, exe
    fail(f"unsupported archive {archive.name}")


def members(archive, ext):
    """(path, mode, is_dir, owner) for each entry; owner is None for zip."""
    if ext == "zip":
        with zipfile.ZipFile(archive) as package:
            return [(i.filename.rstrip("/"), (i.external_attr >> 16) & 0o7777, i.is_dir(), None)
                    for i in package.infolist()]
    with tarfile.open(archive) as package:
        return [(m.name.rstrip("/"), m.mode, m.isdir(), (m.uid, m.gid)) for m in package.getmembers()]


def extract(archive, ext, destination):
    destination.mkdir(parents=True, exist_ok=True)
    if ext == "zip":
        with zipfile.ZipFile(archive) as package:
            package.extractall(destination)
    else:
        with tarfile.open(archive) as package:
            package.extractall(destination, filter="data")


def system_path():
    if os.name == "nt":
        root = os.environ.get("SystemRoot", r"C:\Windows")
        return [root + r"\System32", root, root + r"\System32\Wbem"]
    return ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]


def user_env(root, work):
    """The runner's environment without Rust or Cargo, PATH reduced to bin/ and
    system directories, and HOME and temporary directories inside WORK."""
    env = {k: v for k, v in os.environ.items()
           if not k.upper().startswith(("CARGO", "RUSTUP", "RUST", "PLENTY"))}
    env["PATH"] = os.pathsep.join([str(root / "bin")] + system_path())
    for key in ("HOME", "TMPDIR", "TEMP", "TMP"):
        env[key] = str(work / ("home" if key == "HOME" else "tmp"))
    env.pop("XDG_CACHE_HOME", None)
    if os.name == "nt":
        env["USERPROFILE"] = env["HOME"]
    return env


def run(args, env, cwd, expect=0):
    result = subprocess.run(args, env=env, cwd=cwd, capture_output=True, text=True)
    if result.returncode != expect:
        fail(f"{' '.join(map(str, args))} exited {result.returncode}\n{result.stdout}{result.stderr}")
    return result.stdout.replace("\r\n", "\n")


def snapshot(root):
    entries = []
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root).as_posix()
        if path.is_file():
            entries.append((relative, hashlib.sha256(path.read_bytes()).hexdigest()))
        else:
            entries.append((relative, "dir"))
    return entries


def exercise(root, label, version, target, exe, work):
    env = user_env(root, work)
    project = work / "elsewhere" / "a project"
    for tool in ("plenty", "plentyc"):
        step(f"{label}: {tool} --version")
        reported = run([root / "bin" / f"{tool}{exe}", "--version"], env, project).strip()
        if reported != f"{tool} {version} ({target})":
            fail(f"{tool} --version: {reported}")
    found = shutil.which("plenty", path=env["PATH"])
    if not found or Path(found).resolve().parent != (root / "bin").resolve():
        fail(f"PATH selects {found}, not {root / 'bin'}")
    plenty = found
    step(f"{label}: check, compile, and run from another directory")
    run([plenty, "--check", "smoke.plenty"], env, project)
    output = project / f"smoke program{exe}"
    output.unlink(missing_ok=True)
    run([plenty, "--compile", "smoke.plenty", "-o", output.name], env, project)
    printed = run([output], env, project)
    if printed != EXPECTED:
        fail(f"compiled program printed {printed!r}")
    printed = run([plenty, "smoke.plenty"], env, project)
    if printed != EXPECTED:
        fail(f"plenty FILE printed {printed!r}")


def main(argv):
    if len(argv) != 1:
        raise SystemExit("usage: smoke-native.py ARCHIVE")
    archive = Path(argv[0]).resolve()
    name, version, target, ext, exe = parse_name(archive)

    step("checksum")
    recorded = (archive.parent / f"{archive.name}.sha256").read_text().split()
    if recorded != [hashlib.sha256(archive.read_bytes()).hexdigest(), archive.name]:
        fail("checksum file does not match the archive")

    step("layout and modes")
    entries = members(archive, ext)
    if {path.split("/", 1)[0] for path, *_ in entries} != {name}:
        fail("archive has entries outside its top-level directory")
    paths = {path: (mode, is_dir, owner) for path, mode, is_dir, owner in entries}
    executables = [f"{name}/bin/{tool}{exe}" for tool in ("plenty", "plentyc")]
    for required in [f"{name}/{entry}" for entry in REQUIRED] + executables:
        if required not in paths:
            fail(f"missing {required}")
    for path, (mode, is_dir, owner) in paths.items():
        want = 0o755 if is_dir or path in executables else 0o644
        if mode != want or owner not in (None, (0, 0)):
            fail(f"{path} has mode {mode:o} owner {owner}")

    # Windows can hold a just-run executable open briefly; leave such files.
    with tempfile.TemporaryDirectory(prefix="plenty smoke.", ignore_cleanup_errors=True) as temporary:
        work = Path(temporary)
        for directory in ("home", "tmp", "elsewhere/a project", "Downloads/my releases"):
            (work / directory).mkdir(parents=True)
        extract(archive, ext, work / "Downloads" / "my releases")
        install = work / "Downloads" / "my releases" / name
        if os.name != "nt" and not os.access(install / "bin" / "plenty", os.X_OK):
            fail("bin/plenty is not executable after extraction")

        step("clean environment")
        env = user_env(install, work)
        for tool in RUST_TOOLS:
            if shutil.which(tool, path=env["PATH"]):
                fail(f"{tool} is on PATH")
        if sys.platform == "darwin" and not shutil.which("cc", path=env["PATH"]):
            fail("the documented prerequisite cc is missing")

        (work / "elsewhere" / "a project" / "smoke.plenty").write_text(PROGRAM)
        before = snapshot(install)
        exercise(install, "extracted", version, target, exe, work)

        step("move and rename")
        moved = work / "moved elsewhere" / "plenty renamed"
        moved.parent.mkdir()
        shutil.move(install, moved)
        exercise(moved, "moved", version, target, exe, work)

        step("side by side")
        extract(archive, ext, work / "second copy")
        exercise(work / "second copy" / name, "second copy", version, target, exe, work)
        exercise(moved, "first copy again", version, target, exe, work)

        step("installation unchanged")
        if snapshot(moved) != before:
            fail("the installation directory changed")
    step(f"PASS {name}")


if __name__ == "__main__":
    main(sys.argv[1:])
