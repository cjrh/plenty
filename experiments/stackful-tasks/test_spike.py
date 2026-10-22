"""Compile/run the isolated Linux/glibc stackful-task experiment."""
import os
from pathlib import Path
import shlex
import subprocess
import tempfile


def main():
    source = Path(__file__).with_name("spike.c")
    flags = ["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", "-pedantic"]
    # AddressSanitizer has limitations with makecontext/swapcontext. UBSan checks
    # this experiment without claiming complete alternate-stack instrumentation.
    if os.environ.get("SANITIZE") == "1":
        flags += ["-fsanitize=undefined", "-fno-sanitize-recover=all"]
    with tempfile.TemporaryDirectory(prefix="plenty-stackful-") as directory:
        binary = Path(directory) / "spike"
        subprocess.run(shlex.split(os.environ.get("CC", "cc")) + flags
                       + [str(source), "-o", str(binary)], check=True, timeout=30)
        subprocess.run([str(binary)], check=True, timeout=10)


if __name__ == "__main__":
    main()
