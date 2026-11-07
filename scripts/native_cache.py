#!/usr/bin/env python3
"""Use an optional native compiler cache for local checks without changing cross tools."""
import os
import hashlib
from pathlib import Path
import shutil
import subprocess
import sys


def supports_namespace():
    try:
        subprocess.run(["ccache", "--get-config", "namespace"], check=True,
                       capture_output=True, env={**os.environ, "CCACHE_CONFIGPATH": os.devnull})
        return True
    except (OSError, subprocess.CalledProcessError):
        return False


def compiler_namespace():
    """Hash GCC companion tools once per check invocation, outside each C call."""
    digest = hashlib.sha256()
    for option in ("-print-prog-name=cc1", "-print-prog-name=as", "-print-file-name=specs"):
        try:
            result = subprocess.run(["cc", option], check=True, capture_output=True, text=True)
        except (OSError, subprocess.CalledProcessError):
            continue
        name = result.stdout.strip()
        path = Path(name) if Path(name).is_file() else Path(shutil.which(name) or "/nonexistent")
        if path.is_file():
            data = path.read_bytes()
            digest.update(option.encode())
            digest.update(len(data).to_bytes(8, "little"))
            digest.update(data)
    return "plenty-native-v1-" + digest.hexdigest()


def environment(repo, current):
    env = dict(current)
    # HOST_CC is used only for native builds. Target-specific compilers and
    # explicit user wrappers keep cc-rs's existing discovery/precedence.
    if (sys.platform != "linux" or env.get("PLENTY_NATIVE_CACHE") == "off"
            or env.get("RUSTC_WRAPPER")
            or not shutil.which("ccache") or not shutil.which("cc")
            or any(key in {"CC", "HOST_CC", "TARGET_CC"} or key.startswith("CC_")
                   or (key.startswith("CCACHE_") and key != "CCACHE_DIR")
                   or ("CFLAGS" in key and env[key])
                   for key in env)):
        return env
    if not supports_namespace():
        return env
    try:
        namespace = compiler_namespace()
    except OSError:
        return env
    env.update({
        "HOST_CC": "ccache cc",
        "CCACHE_DIR": env.get("CCACHE_DIR", str(Path(repo) / "target/native-ccache")),
        "CCACHE_COMPILERCHECK": "content",
        "CCACHE_CONFIGPATH": os.devnull,
        "CCACHE_NAMESPACE": namespace,
        # Preprocess each time to include SDK/system headers, newly appearing
        # headers, and compiler macros instead of trusting header manifests.
        "CCACHE_NODIRECT": "true",
        "CCACHE_NODEPEND": "true",
        "CCACHE_SLOPPINESS": "",
    })
    return env


def main():
    if len(sys.argv) < 2:
        raise SystemExit("usage: native_cache.py COMMAND [ARG...]")
    env = environment(Path(__file__).resolve().parent.parent, os.environ)
    env["PLENTY_NATIVE_CACHE_CONFIGURED"] = "1"
    os.execvpe(sys.argv[1], sys.argv[1:], env)


if __name__ == "__main__":
    main()
