#!/usr/bin/env python3
"""Run the current-language evidence for the protocol-body feasibility sketch.

These are handwritten lowerings, not an implementation of protocol defaults.
Pass a compiler built from this checkout. No third-party Python packages needed.
"""

import argparse
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap


LIBRARY = """
pub protocol Storage:
    def write(self: &mut Storage, amount: i64) -> i64:
        pass

def valid(amount: i64) -> bool:
    amount >= 0

pub def save[S: Storage](storage: &mut S, amount: i64) -> i64:
    if not valid(amount):
        return -1
    storage.write(amount)

pub def save_twice[S: Storage](storage: &mut S, amount: i64) -> i64:
    first = save(storage, amount)
    save(storage, first)
"""

BACKEND = """
pub class Memory:
    pub total: i64

    pub def write(self: &mut Memory, amount: i64) -> i64:
        self.total += amount
        self.total

    pub def save(self: &mut Memory, amount: i64) -> i64:
        -999
"""

MAIN = """
from library import save, save_twice
from backend import Memory

def main() -> Result[(), IoError]:
    mut memory = Memory(0)
    print(save(&mut memory, -4))?
    print(save(&mut memory, 3))?
    print(save_twice(&mut memory, 2))?
    print(memory.save(2))?
    Ok(())
"""


def cases():
    yield (
        "shared-body-cross-module-and-inherent-name-collision",
        {"library.plenty": LIBRARY, "backend.plenty": BACKEND, "main.plenty": MAIN},
        "-1\n3\n10\n-999\n",
        None,
    )
    yield (
        "private-hook-does-not-grant-library-access",
        {
            "library.plenty": LIBRARY,
            "backend.plenty": BACKEND.replace("pub def write", "def write"),
            "main.plenty": MAIN,
        },
        None,
        "private to its defining module",
    )
    yield (
        "application-cannot-call-private-library-helper",
        {
            "library.plenty": LIBRARY,
            "main.plenty": """
                from library import valid
                def main() -> ():
                    valid(1)
            """,
        },
        None,
        "private",
    )
    yield (
        "non-pass-protocol-body-rejected",
        {"main.plenty": """
            protocol Size:
                def size(self) -> i64:
                    10
            def main() -> ():
                pass
        """},
        None,
        "default implementations are not supported",
    )
    yield (
        "helper-does-not-inject-object-method",
        {
            "library.plenty": LIBRARY,
            "backend.plenty": BACKEND.split("    pub def save")[0],
            "main.plenty": """
                from library import save
                from backend import Memory
                def main() -> ():
                    mut memory = Memory(0)
                    memory.save(1)
            """,
        },
        None,
        "unknown method `save`",
    )
    yield (
        "parameterized-protocol-error-inference-and-enum",
        {"main.plenty": """
            enum ReadError:
                Empty
            protocol Source[T, E]:
                def read(self) -> Result[T, E]:
                    pass
            enum Answer:
                Ready
                def read(self) -> Result[i64, ReadError]:
                    Ok(42)
            def read_again[T, E, S: Source[T, E]](source: &S) -> Result[T, E]:
                value = source.read()?
                Ok(value)
            def main() -> Result[(), IoError]:
                answer = Answer.Ready
                match read_again(&answer):
                    case Ok(value):
                        print(value)?
                    case Err(_):
                        print(-1)?
                Ok(())
        """},
        "42\n",
        None,
    )
    yield (
        "method-local-type-parameters",
        {"main.plenty": """
            protocol Sized:
                def size(self) -> i64:
                    pass
            protocol Sink:
                def put[T: Sized](self, value: &T) -> i64:
                    pass
            class Measure:
                def put[T: Sized](self, value: &T) -> i64:
                    value.size()
            class Item:
                def size(self) -> i64:
                    7
            def put_twice[S: Sink, T: Sized](sink: &S, value: &T) -> i64:
                sink.put(value) + sink.put(value)
            def main() -> Result[(), IoError]:
                sink = Measure()
                item = Item()
                print(put_twice(&sink, &item))?
                Ok(())
        """},
        "14\n",
        None,
    )
    references = """
        protocol View:
            def view(self) -> &i64:
                pass
        class Cell:
            value: i64
            def view(self) -> &i64:
                &self.value
        def shared_view[C: View](cell: &C) -> &i64:
            cell.view()
    """
    yield (
        "returned-reference-through-shared-body",
        {"main.plenty": textwrap.dedent(references) + """
def main() -> Result[(), IoError]:
    cell = Cell(8)
    value = shared_view(&cell)
    print(*value)?
    Ok(())
"""},
        "8\n",
        None,
    )
    yield (
        "returned-reference-retains-loan",
        {"main.plenty": textwrap.dedent(references) + """
def main() -> Result[(), IoError]:
    mut cell = Cell(8)
    value = shared_view(&cell)
    cell.value = 9
    print(*value)?
    Ok(())
"""},
        None,
        "borrow",
    )
    yield (
        "concrete-body-can-use-undeclared-method",
        {"main.plenty": """
            protocol HasValue:
                def value(self) -> i64:
                    pass
            class Concrete:
                def value(self) -> i64:
                    1
                def extra(self) -> i64:
                    99
            def body[T: HasValue](value: &T) -> i64:
                value.extra()
            def main() -> Result[(), IoError]:
                value = Concrete()
                print(body(&value))?
                Ok(())
        """},
        "99\n",
        None,
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compiler", type=Path)
    args = parser.parse_args()
    compiler = args.compiler.resolve(strict=True)
    passed = 0
    for name, files, expected, diagnostic in cases():
        with tempfile.TemporaryDirectory(prefix="plenty-protocol-probe-") as temp:
            root = Path(temp)
            for filename, source in files.items():
                (root / filename).write_text(textwrap.dedent(source).lstrip())
            command = [str(compiler)]
            if diagnostic:
                command.append("--check")
            command.append(str(root / "main.plenty"))
            result = subprocess.run(
                command, capture_output=True, text=True, timeout=60,
                env={**os.environ, "PLENTY_CACHE": "off"},
            )
            if diagnostic:
                ok = result.returncode != 0 and diagnostic in result.stderr
            else:
                ok = result.returncode == 0 and result.stdout == expected
            if not ok:
                raise SystemExit(
                    f"FAIL {name} (exit {result.returncode})\n"
                    f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
                )
            passed += 1
            print(f"PASS {name}")
    print(f"{passed} probes passed")


if __name__ == "__main__":
    main()
