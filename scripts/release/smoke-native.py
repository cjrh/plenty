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

The core program covers tagged results, callbacks, generators and printing.
The dataframe program checks Parquet queries, optional linkage and relocated
application distributions. Standard-library services such as SQLite,
TLS, and processes, project commands, and the missing-linker diagnostic are
not yet checked here.
"""
import hashlib
import importlib.util
import os
import platform
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
PROGRAM = '''def key(value: &i64) -> Result[i64, OverflowError[i64]]:
    Ok(-*value)

def count(stop: i64) -> Generator[i64]:
    for value in range(stop):
        yield value

def main() -> Result[(), AllocError | IoError | ParseError | OverflowError[i64]]:
    number = i64.parse("9223372036854775806")?
    mut values = [number, -7, 42]?
    values.sort_by_key(key)?
    print(values)?
    print(str.repr(i64.parse("bad"))?)?
    for value in count(3):
        print(value)?
    Ok(())
'''
EXPECTED = "[9223372036854775806, 42, -7]\nResult[i64, ParseError].Err(ParseError.Invalid)\n0\n1\n2\n"
DATAFRAME_PROGRAM = '''import std.dataframe as df

def main() -> Result[(), Failure]:
    columns = [df.column_i64("value", [1, 2, 3]?)?]?
    data = df.frame(columns)?
    data.write_parquet("smoke.parquet")?
    totals = [df.col("value")?.sum()?.alias("total")?]?
    result = df.scan_parquet("smoke.parquet")?.select(totals)?.collect_streaming()?
    print(result.get_i64("total", 0)?.unwrap())?
    Ok(())
'''


def dataframe_library(target):
    return "plenty_dataframe_v1.dll" if target == "x86_64-pc-windows-msvc" else "libplenty_dataframe_v1.dylib"


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
           if not k.upper().startswith(("CARGO", "RUSTUP", "RUST", "PLENTY", "DYLD_"))}
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
    exercise_dataframe(root, label, work, target, exe)



def exercise_macos_engine_replacement(plenty, output, project, env, source):
    """Change signed bytes and rebuild into one directory without mutating a
    previously loaded dylib's vnode. Re-sign only a fresh copy of the engine."""
    step("macOS: rebuild ordinary output with alternate and original signed engines")
    installed = output.with_name(source.name)
    with tempfile.TemporaryDirectory(prefix="signed-engine.", dir=project) as directory:
        alternate = Path(directory) / source.name
        shutil.copy2(source, alternate)
        run(["/usr/bin/codesign", "--force", "--sign", "-", "--identifier",
             "plenty-smoke-dataframe-alternate", alternate], env, project)
        run(["/usr/bin/codesign", "--verify", "--strict", alternate], env, project)
        if alternate.read_bytes() == source.read_bytes():
            fail("alternate engine re-signing did not change its bytes")
        for selected in (alternate, source):
            with installed.open("rb") as previous:
                old_inode = os.fstat(previous.fileno()).st_ino
                old_digest = hashlib.sha256(previous.read()).digest()
                selected_env = {**env, "PLENTY_DATAFRAME_ENGINE": str(selected), "PLENTY_CACHE": "off"}
                run([plenty, "--compile", "analysis.plenty", "-o", output], selected_env, project)
                if installed.stat().st_ino == old_inode:
                    fail("signed engine was overwritten in place instead of replaced")
                previous.seek(0)
                if hashlib.sha256(previous.read()).digest() != old_digest:
                    fail("rebuilding changed a previously opened engine file")
                if installed.read_bytes() != selected.read_bytes():
                    fail("rebuild did not install the selected engine")
            run(["/usr/bin/codesign", "--verify", "--strict", installed], env, project)
            if run([output], env, project) != "6\n":
                fail("rebuilt dataframe application failed after changing signed engines")

def exercise_dataframe(root, label, work, target, exe):
    step(f"{label}: dataframe engine discovery, Parquet query and --dist relocation")
    library = dataframe_library(target)
    project = work / "elsewhere" / "a project"
    (project / "analysis.plenty").write_text(DATAFRAME_PROGRAM)
    env = user_env(root, work)
    env["POLARS_MAX_THREADS"] = "2"
    plenty = root / "bin" / f"plenty{exe}"
    (project / "unused.plenty").write_text("import std.dataframe\n\n" + PROGRAM)
    plain = project / "plain app"
    plain.mkdir(exist_ok=True)
    env["PLENTY_DATAFRAME_ENGINE"] = str(project / library)
    run([plenty, "--compile", "unused.plenty", "-o", plain / f"unused{exe}"], env, project)
    if (plain / library).exists():
        fail("unused dataframe import copies the engine")
    if run([plain / f"unused{exe}"], env, project) != EXPECTED:
        fail("unused dataframe import requires the engine")
    del env["PLENTY_DATAFRAME_ENGINE"]
    if run([plenty, "analysis.plenty"], env, project) != "6\n":
        fail("dataframe Parquet query result")
    ordinary = project / "ordinary app"
    ordinary.mkdir(exist_ok=True)
    output = ordinary / f"analysis{exe}"
    run([plenty, "--compile", "analysis.plenty", "-o", output], env, project)
    if not (ordinary / library).is_file() or run([output], env, project) != "6\n":
        fail("ordinary dataframe output lacks a working sibling engine")
    if target == "aarch64-apple-darwin" and sys.platform == "darwin":
        exercise_macos_engine_replacement(plenty, output, project, env, root / "bin" / library)
    destination = project / "distribution"
    run([plenty, "--compile", "analysis.plenty", "--dist", destination], env, project)
    for entry in (f"analysis{exe}", library, "dataframe-notices/engine.json"):
        if not (destination / entry).is_file():
            fail(f"dataframe distribution lacks {entry}")
    if target == "aarch64-apple-darwin" and sys.platform == "darwin":
        spec = importlib.util.spec_from_file_location("native_audit", Path(__file__).with_name("check-native.py"))
        audit = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(audit)
        problems, _ = audit.check_macho((destination / "analysis").read_bytes(), platform.mac_ver()[0], engines=(library,))
        if problems:
            fail("; ".join(problems))
        run(["/usr/bin/codesign", "--verify", "--strict", destination / "analysis"], env, project)
        run(["/usr/bin/codesign", "--verify", "--strict", destination / library], env, project)
    moved = project / "moved dataframe app"
    destination.rename(moved)
    # Remove the compiler's bin directory from PATH: only the bundled engine may
    # satisfy the executable, including after the application directory moves.
    env["PATH"] = os.pathsep.join(system_path())
    if run([moved / f"analysis{exe}"], env, project) != "6\n":
        fail("relocated dataframe distribution result")
    engine = moved / library
    engine.rename(engine.with_suffix(".hidden"))
    result = subprocess.run([moved / f"analysis{exe}"], env=env, cwd=project,
                            capture_output=True, text=True)
    if result.returncode == 0:
        fail("dataframe application ran without its engine")
    shutil.rmtree(moved)


def main(argv):
    if len(argv) != 1:
        raise SystemExit("usage: smoke-native.py ARCHIVE")
    archive = Path(argv[0]).resolve()
    name, version, target, ext, exe = parse_name(archive)

    step("checksum")
    recorded = (archive.parent / f"{archive.name}.sha256").read_text().split()
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    # sha256sum uses a binary-mode marker on Windows (" *filename").
    if recorded not in ([digest, archive.name], [digest, f"*{archive.name}"]):
        fail("checksum file does not match the archive")

    step("layout and modes")
    entries = members(archive, ext)
    if {path.split("/", 1)[0] for path, *_ in entries} != {name}:
        fail("archive has entries outside its top-level directory")
    paths = {path: (mode, is_dir, owner) for path, mode, is_dir, owner in entries}
    executables = [f"{name}/bin/{tool}{exe}" for tool in ("plenty", "plentyc")]
    required_files = list(REQUIRED)
    if target in TARGETS:
        required_files += [f"bin/{dataframe_library(target)}", "bin/dataframe-notices/engine.json",
                           "bin/dataframe-notices/THIRD-PARTY-NOTICES.md",
                           "bin/dataframe-notices/LICENSE-MIT", "bin/dataframe-notices/LICENSE-APACHE",
                           "bin/dataframe-notices/licenses/rust/COPYRIGHT-library.html"]
    for required in [f"{name}/{entry}" for entry in required_files] + executables:
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
