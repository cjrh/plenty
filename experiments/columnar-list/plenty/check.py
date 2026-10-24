#!/usr/bin/env python3
"""Compile and execute the generated clist facade using a runtime-checks compiler."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

from generate import generate

HERE = Path(__file__).resolve().parent


def run(compiler, directory, name, body, expected=None, rejection=None):
    source = directory / f"{name}.plenty"
    source.write_text("from columns import Particle, ClistParticle, AppendParticleError\n" + body)
    command = [str(compiler), *( ["--check"] if rejection else []), str(source)]
    result = subprocess.run(command, text=True, capture_output=True,
                            env={**os.environ, "PLENTY_CACHE": "off", "PLENTY_LINKER_THREADS": "1"})
    if rejection:
        # Ignore the filename (which itself may contain e.g. "borrow").
        diagnostic = result.stderr.split(": ", 2)[-1]
        assert result.returncode != 0 and rejection in diagnostic, (name, result.stderr)
    else:
        assert result.returncode == 0, (name, result.stderr, result.stdout)
        output = "\n".join(line for line in result.stdout.splitlines() if not line.startswith("__test_"))
        assert output == expected, (name, output, expected)
    print(f"PASS {name}")


def check(compiler, directory):
    (directory / "columns.plenty").write_text(generate((HERE / "particle.plenty").read_text()))
    run(compiler, directory, "operations", """
def accumulate(total: i64, x: &i64, y: &i64) -> i64:
    total + *y if *x >= 2 else total
def main() -> Result[(), AllocError | IoError | AppendParticleError]:
    mut items = ClistParticle()?
    for i in range(5):
        items.append(Particle(i, i * 10, 2, 3, 4, 5, 6, 7))?
    print("__test_begin_no_allocations__")?
    xs = items.column_x()
    ys = items.column_y()
    total = iter(xs).zip(ys).fold(0, accumulate)
    print("__test_end_no_allocations__")?
    print(total)?
    print(items.len(), items.get_x(-1))?
    items.set_y(-1, 99)
    row = items.copy_row(-1).unwrap()
    print(row.x, row.y, items.copy_row(5).is_some(), items.copy_row(-6).is_some())?
    print(items.copy_row(i64.MIN).is_some(), items.copy_row(i64.MAX).is_some())?
    items.reserve(20)?
    print("__test_begin_no_allocations__")?
    items.append(Particle(5, 50, 2, 3, 4, 5, 6, 7)).unwrap()
    copied = items.copy_row(-1).unwrap()
    print("__test_end_no_allocations__")?
    print(copied.x, items.len())?
    moved = items
    print(moved.get_y(-1))?
    Ok(())
""", "90\n5 4\n4 99 False False\nFalse False\n5 6\n50")
    # Every constructor handle allocation may fail. Already-created columns
    # must drop on early return; the instrumented runtime checks live ownership.
    for budget in range(9):
        run(compiler, directory, f"construct_{budget}", f"""
def main() -> ():
    print("__test_fail_allocations_after_{budget}__").unwrap()
    result = ClistParticle()
    print("__test_restore_allocations__").unwrap()
    print(result.is_ok()).unwrap()
""", "True" if budget == 8 else "False")
    # First append grows eight independent columns. Repeat after filling a
    # reserved capacity to exercise growth while existing rows stay intact.
    for existing in [0, 8]:
        for budget in range(9):
            run(compiler, directory, f"append_{existing}_{budget}", f"""
def main() -> Result[(), AllocError | IoError | AppendParticleError]:
    mut items = ClistParticle()?
    items.reserve({existing})?
    for i in range({existing}):
        items.append(Particle(i, 10, 20, 30, 40, 50, 60, 70))?
    row = Particle(99, 101, 102, 103, 104, 105, 106, 107)
    print("__test_fail_allocations_after_{budget}__")?
    result = items.append(row)
    print("__test_restore_allocations__")?
    match result:
        case Ok(done):
            print("ok", items.len())?
        case Err(AppendParticleError.Allocation(error, recovered)):
            print("failed", items.len(), recovered.x, recovered.y, recovered.z, recovered.vx, recovered.vy, recovered.vz, recovered.mass, recovered.tag)?
            items.append(recovered)?
    print(items.len(), items.get_x(-1), items.get_tag(-1))?
    print(items.get_x(0))?
    Ok(())
""", (f"ok {existing + 1}" if budget == 8 else f"failed {existing} 99 101 102 103 104 105 106 107") +
                f"\n{existing + 1} 99 107\n" + ("0" if existing else "99"))
    run(compiler, directory, "reserve_overflow", """
def main() -> Result[(), AllocError | IoError]:
    mut items = ClistParticle()?
    print(items.reserve(-1).is_err(), items.reserve(i64.MAX).is_err(), items.len())?
    Ok(())
""", "True True 0")
    run(compiler, directory, "differential", """
def main() -> Result[(), AllocError | IoError | AppendParticleError]:
    for count in [0, 1, 2, 7, 8, 9, 33, 1025]?:
        mut items = ClistParticle()?
        mut rows = list[Particle]()?
        for i in range(count):
            items.append(Particle(i, i * 2, i * 3, i * 4, i * 5, i * 6, i * 7, i * 8))?
            rows.append(Particle(i, i * 2, i * 3, i * 4, i * 5, i * 6, i * 7, i * 8))?
        mut equal = items.len() == len(rows)
        for i in range(count):
            items.set_y(i, i * 9)
            rows[i].y = i * 9
            positive = items.copy_row(i).unwrap()
            negative = items.copy_row(i - count).unwrap()
            equal = equal and positive == rows[i] and negative == rows[i]
        print(equal)?
    Ok(())
""", "\n".join(["True"] * 8))
    run(compiler, directory, "borrow_blocks_append", """
def main() -> Result[(), AllocError]:
    mut items = ClistParticle()?
    column = items.column_x()
    drop(items.append(Particle(1, 2, 3, 4, 5, 6, 7, 8)))
    drop(len(column))
    Ok(())
""", rejection="borrow")
    run(compiler, directory, "pipeline_blocks_set", """
def main() -> Result[(), AllocError]:
    mut items = ClistParticle()?
    values = iter(items.column_x())
    items.set_x(0, 10)
    drop(values.count())
    Ok(())
""", rejection="borrow")
    run(compiler, directory, "direct_column", """
def main() -> Result[(), AllocError | IoError | AppendParticleError]:
    mut items = ClistParticle()?
    items.append(Particle(1, 2, 3, 4, 5, 6, 7, 8))?
    print(iter(items.column_x()).count())?
    Ok(())
""", "1")
    run(compiler, directory, "private_storage", """
def main() -> Result[(), AllocError]:
    mut items = ClistParticle()?
    items._x.append(1)?
    Ok(())
""", rejection="private")
    run(compiler, directory, "moved_owner", """
def main() -> Result[(), AllocError]:
    items = ClistParticle()?
    moved = items
    drop(items.len())
    Ok(())
""", rejection="moved")
    # All primitive widths retain their values through storage and
    # reconstruction, including signed values and floating negative zero.
    scalar_types = ["bool", "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64"]
    scalar_values = ["True", "-128i8", "-32768i16", "-123456i32", "i64.MIN", "255u8", "65535u16",
                     "123456u32", "u64.MAX", "-0.0f32", "1.25"]
    scalar_schema = "pub class Scalars:\n" + "".join(f"    pub f{i}: {ty}\n" for i, ty in enumerate(scalar_types))
    (directory / "scalars.plenty").write_text(generate(scalar_schema))
    run(compiler, directory, "scalar_widths", """
from scalars import Scalars, ClistScalars, AppendScalarsError
def main() -> Result[(), AllocError | IoError | AppendScalarsError]:
    mut items = ClistScalars()?
    row = Scalars(""" + ", ".join(scalar_values) + """)
    items.append(row)?
    copied = items.copy_row(0).unwrap()
    expected = Scalars(""" + ", ".join(scalar_values) + """)
    print(copied == expected, 1.0f32 / copied.f9 < 0.0f32)?
    Ok(())
""", "True True")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compiler", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="plenty-clist-check-") as tmp:
        check(args.compiler.resolve(), Path(tmp))


if __name__ == "__main__":
    main()
