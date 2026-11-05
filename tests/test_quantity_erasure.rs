//! Native Quantity values use their storage ABI; runtime slots preserve all bits.
mod support;
const BASES: &str = "dimension Length(\"m\")\ndimension Time(\"s\")\n";
#[test]
fn release_arithmetic_matches_plain_numeric_instructions() {
    for storage in ["f32", "f64"] {
        let dir = tempfile::tempdir().unwrap();
        let source = format!("{BASES}
def quantity_add(a: Quantity[f64, Length], b: Quantity[f64, Length]) -> Quantity[f64, Length]:
    a + b
def scalar_add(a: f64, b: f64) -> f64:
    a + b
def quantity_mul(a: Quantity[f64, Length], b: Quantity[f64, Time]) -> Quantity[f64, Length * Time]:
    a * b
def scalar_mul(a: f64, b: f64) -> f64:
    a * b
def quantity_to(a: Quantity[f64, Length], b: Quantity[f64, Length]) -> f64:
    a.to(b)
def scalar_div(a: f64, b: f64) -> f64:
    a / b
const km: Quantity[f64, Length] = Quantity[f64, Length](1000.0)
def quantity_km(a: Quantity[f64, Length]) -> f64:
    a.to(km)
def scalar_km(a: f64) -> f64:
    a / 1000.0
def quantity_new(a: f64) -> Quantity[f64, Length]:
    Quantity[f64, Length](a)
def scalar_identity(a: f64) -> f64:
    a
def main() -> Result[(), IoError]:
    a = Quantity[f64, Length](6.0)
    b = Quantity[f64, Length](2.0)
    t = Quantity[f64, Time](2.0)
    print(quantity_add(a, b).get(), scalar_add(6.0, 2.0), quantity_mul(a, t).get(), scalar_mul(6.0, 2.0), quantity_to(a, b), scalar_div(6.0, 2.0))?
    print(quantity_km(a), scalar_km(6.0), quantity_new(1.0).get(), scalar_identity(1.0))?
    Ok(())
").replace("f64", storage);
        std::fs::write(dir.path().join("q.plenty"), source).unwrap();
        let output = std::process::Command::new(support::plenty_bin())
            .current_dir(dir.path())
            .env("PLENTY_CACHE", "off")
            .args(["--release", "--dump", "asm", "q.plenty", "-o", "out"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let asm = std::fs::read_to_string(dir.path().join("out/asm.txt")).unwrap();
        let instructions = |name: &str| -> Vec<String> {
            support::asm_body(&asm, name)
                .lines()
                .filter(|line| !line.trim().is_empty() && !line.trim().starts_with('#'))
                .map(str::to_owned)
                .collect()
        };
        for (quantity, scalar) in [
            ("quantity_add", "scalar_add"),
            ("quantity_mul", "scalar_mul"),
            ("quantity_to", "scalar_div"),
            ("quantity_km", "scalar_km"),
            // Construction retags the storage value without a call.
            ("quantity_new", "scalar_identity"),
        ] {
            assert_eq!(
                instructions(quantity),
                instructions(scalar),
                "{storage}: {quantity}"
            );
        }
    }
}

fn run_modes(source: &str, expected: &str) {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("main.plenty");
    std::fs::write(&entry, source).unwrap();
    for optimize in [false, true] {
        let executable = dir.path().join(if optimize { "release" } else { "debug" });
        plenty::compile_file_to_executable_with_options(
            &entry,
            &executable,
            None,
            &plenty::CompileOptions {
                optimize,
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("optimize={optimize}: {error}"));
        let output = std::process::Command::new(executable).output().unwrap();
        assert!(
            output.status.success(),
            "optimize={optimize}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            expected,
            "optimize={optimize}"
        );
    }
}

#[test]
fn numeric_bits_survive_native_and_runtime_slot_boundaries() {
    let mut source = BASES.to_owned();
    let mut calls = String::new();
    let mut expected = String::new();
    let storages = [
        ("i8", "-128"),
        ("i16", "-32768"),
        ("i32", "-2147483648"),
        ("i64", "i64.MIN"),
        ("u8", "255"),
        ("u16", "65535"),
        ("u32", "4294967295"),
        ("u64", "u64.MAX"),
        ("f32", "-123.25"),
        ("f64", "-123.25"),
        ("Positive[i8]", "127"),
        ("NonNegative[i32]", "0"),
        ("NonZero[u64]", "18446744073709551615"),
    ];
    for (index, (storage, literal)) in storages.into_iter().enumerate() {
        source.push_str(&format!(r#"
type Q{index} = Quantity[{storage}, Length]
@copy
class Single{index}:
    pub value: Q{index}
@copy
class Pair{index}:
    pub first: Q{index}
    pub last: Q{index}
def identity{index}(q: Q{index}) -> Q{index}:
    q
def tail{index}(q: Q{index}, n: i64) -> Q{index}:
    if n == 0:
        return q
    # Q stays the direct return type, so the counter traps instead.
    unchecked:
        tail{index}(q, n - 1)
def indirect{index}(f: Callable[[Q{index}], Q{index}], q: Q{index}) -> Q{index}:
    f(q)
def read{index}(q: &Q{index}) -> Q{index}:
    *q
def write{index}(q: &mut Q{index}, replacement: Q{index}) -> ():
    *q = replacement
def generate{index}(q: Q{index}) -> Generator[Q{index}]:
    yield q
    yield q
def check{index}() -> Result[(), AllocError | IoError]:
    raw: {storage} = {literal}
    ctor: Callable[[{storage}], Q{index}] = Q{index}
    q = ctor(raw)
    mut changed = Q{index}(raw)
    write{index}(&mut changed, q)
    values = [q, changed]?
    single = Single{index}(q)
    pair = Pair{index}(q, changed)
    option: Option[Q{index}] = Some(q)
    tuple = (q, changed)
    captured = def [q](other: Q{index}) -> Q{index}:
        if other == q:
            return q
        other
    generated = list(generate{index}(q))?
    print(identity{index}(q) == q, tail{index}(q, 200) == q, indirect{index}(identity{index}, q) == q, read{index}(&q) == q, changed == q)?
    print(values[1] == q, single.value == q, pair.last == q, option.unwrap() == q, tuple[0] == q, captured(q) == q, generated[1] == q, q.get() == raw)?
    Ok(())
"#));
        calls.push_str(&format!("    check{index}()?\n"));
        expected.push_str("True True True True True\nTrue True True True True True True True\n");
    }
    source.push_str(&format!(
        "def main() -> Result[(), AllocError | IoError]:\n{calls}    Ok(())\n"
    ));
    run_modes(&source, &expected);
}

#[test]
fn float_special_values_and_worker_transfers_preserve_storage() {
    for storage in ["f32", "f64"] {
        run_modes(
            &format!(
                r#"{BASES}
type Q = Quantity[{storage}, Length]
def identity(q: Q) -> Q:
    q
def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    zero = Q(-0.0)
    boxed = Box(zero)?
    values = [zero, Q({storage}.NAN), Q({storage}.INF)]?
    print(1.0 / boxed.get(), values[1] == values[1], values[2].get())?
    with spawn(identity, zero).drop_job()? as task:
        print(1.0 / task.join().get())?
    wide = Quantity[u64, Length](u64.MAX)
    job = def [wide]() -> Quantity[u64, Length]:
        wide
    with spawn(job).drop_job()? as task:
        print(task.join().get())?
    Ok(())
"#
            ),
            "-inf False inf\n-inf\n18446744073709551615\n",
        );
    }
}

#[test]
fn direct_methods_keep_argument_checks_borrows_and_evaluation_order() {
    let prelude = format!(
        r#"{BASES}
type Q = Quantity[f64, Length]
def changed(q: &mut Q) -> Q:
    *q = Q(2.0)
    Q(1.0)
"#
    );
    for expression in [
        "q.to(changed(&mut q))",
        "q.to(unit=changed(&mut q))",
        "q.to(Quantity[f64, Time](1.0))",
        "q.to(Quantity[f32, Length](1.0))",
        "q.get(1)",
        "q.to()",
        "q.to(other=q)",
        "Q(other=1.0)",
    ] {
        let source =
            format!("{prelude}\ndef main() -> ():\n    mut q = Q(1.0)\n    drop({expression})\n");
        assert!(plenty::check_source(&source).is_err(), "{expression}");
    }
    run_modes(
        &format!(
            r#"{BASES}
type Q = Quantity[f64, Length]
def traced(value: Q, digit: i64, order: &mut i64) -> Q:
    unchecked:
        *order = *order * 10 + digit
    value
def main() -> Result[(), IoError]:
    mut order = 0
    result = traced(Q(value=6.0), 1, &mut order).to(unit=traced(Q(2.0), 2, &mut order))
    print(result, order)?
    Ok(())
"#
        ),
        "3.0 12\n",
    );
}
