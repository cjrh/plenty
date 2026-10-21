use super::*;

fn substituted(source: &str, optimize: bool) -> Vec<Op> {
    let mut heap = Heap::default();
    let mut ops = crate::frontend::compile(source, &mut heap).unwrap().ops;
    substitute(&mut ops, optimize, 2);
    crate::op::check(&ops).unwrap();
    ops
}

fn function<'a>(ops: &'a [Op], name: &str) -> Option<&'a CompiledFn> {
    ops.iter().find_map(|op| match op {
        Op::DefineFn(n, f) if n == name => Some(&**f),
        _ => None,
    })
}

fn calls(ops: &[Op]) -> Vec<String> {
    let mut names = Vec::new();
    for op in ops {
        match op {
            Op::Call(name) | Op::TailCall(name) => names.push(name.clone()),
            Op::Match(arms) => arms.iter().for_each(|arm| names.extend(calls(&arm.body))),
            Op::If {
                then,
                elifs,
                otherwise,
            } => {
                names.extend(calls(then));
                for arm in elifs.iter() {
                    names.extend(calls(&arm.condition));
                    names.extend(calls(&arm.body));
                }
                names.extend(calls(otherwise));
            }
            Op::Loop { condition, body } => {
                names.extend(calls(condition));
                names.extend(calls(body));
            }
            _ => {}
        }
    }
    names
}

const CONSTANTS: &str = "\
const lf: u8 = 10
const big: u64 = 18446744073709551615
const low: i8 = -128
const pi: f64 = f64.PI
const cold: f32 = -f32.INF
def f(x: u8) -> bool:
    return x == lf
def g() -> u64:
    big
def h() -> i8:
    return low
def k() -> f64:
    pi
def m() -> f32:
    cold
def main() -> ():
    f(lf)
    m()
    g()
    h()
    k()
    ()
";

#[test]
fn constants_become_typed_literals_and_their_getters_are_removed() {
    for optimize in [false, true] {
        let ops = substituted(CONSTANTS, optimize);
        assert!(
            !ops.iter()
                .any(|op| matches!(op, Op::DefineFn(name, _) if name.starts_with("$const."))),
            "getters remain"
        );
        for name in ["f", "g", "h", "k", "m", "main"] {
            let f = function(&ops, name).unwrap();
            assert!(
                calls(&f.body).iter().all(|c| !c.starts_with("$const.")),
                "{name} still calls a getter"
            );
        }
        let body = |name| function(&ops, name).unwrap().body.to_vec();
        assert!(body("f").contains(&Op::PushInt(Value::U8(10))));
        assert!(body("g").contains(&Op::PushInt(Value::U64(u64::MAX))));
        assert!(body("h").contains(&Op::PushInt(Value::I8(-128))));
        assert!(body("k").iter().any(|op| matches!(
            op,
            Op::PushFloat { bits, ty } if *bits == std::f64::consts::PI.to_bits() && matches!(ty, Ty::F64)
        )));
        assert!(body("m").iter().any(|op| matches!(
            op,
            Op::PushFloat { bits, ty } if *bits == u64::from(f32::NEG_INFINITY.to_bits()) && matches!(ty, Ty::F32)
        )));
    }
}

const PREDICATES: &str = "\
const space: u8 = 32
const tab: u8 = 9
def blank(value: u8) -> bool:
    value == space or value == tab
def digit(code: i64) -> bool:
    code >= 48 and code <= 57
def bump(code: i64) -> i64:
    code + 1
def forever(code: i64) -> bool:
    if code > 0:
        return forever(code - 1)
    False
def count(text: i64, other: u8) -> bool:
    digit(text) and blank(other) or digit(text + 1)
def last(code: i64) -> bool:
    digit(code)
def main() -> ():
    count(1, 2)
    bump(1)
    forever(3)
    last(4)
    ()
";

#[test]
fn leaf_predicates_are_substituted_only_in_optimized_builds() {
    let ops = substituted(PREDICATES, false);
    let count = function(&ops, "count").unwrap();
    assert_eq!(calls(&count.body), ["digit", "blank", "digit"]);
    let before = count.locals.len();

    let ops = substituted(PREDICATES, true);
    let count = function(&ops, "count").unwrap();
    assert!(calls(&count.body).is_empty(), "{:?}", count.body);
    // Two calls to `digit` share one argument slot; `blank` has its own.
    assert_eq!(count.locals.len(), before + 2);
    // A tail call to a predicate returns the predicate's result.
    let last = function(&ops, "last").unwrap();
    assert!(calls(&last.body).is_empty());
    assert_eq!(last.body.last(), Some(&Op::Return));
    // Definitions stay: a function value may still name them.
    assert!(function(&ops, "digit").is_some());
    assert!(function(&ops, "blank").is_some());
    assert!(calls(&function(&ops, "blank").unwrap().body).is_empty());
}

#[test]
fn trapping_and_recursive_functions_stay_calls() {
    let ops = substituted(PREDICATES, true);
    let main = function(&ops, "main").unwrap();
    assert_eq!(calls(&main.body), ["count", "bump", "forever", "last"]);
    let forever = function(&ops, "forever").unwrap();
    assert_eq!(calls(&forever.body), ["forever"]);
}

#[test]
fn generators_keep_predicate_calls_but_lose_constant_calls() {
    let ops = substituted(
        "\
const one: i64 = 1
def digit(code: i64) -> bool:
    code >= 48 and code <= 57
def values() -> Generator[bool]:
    yield digit(one)
def main() -> ():
    for v in values():
        v
",
        true,
    );
    let values = function(&ops, "values").unwrap();
    assert_eq!(calls(&values.body), ["digit"]);
}

#[test]
fn predicates_are_substituted_in_programs_without_constants() {
    let ops = substituted(
        "\
def small(n: i64) -> bool:
    n < 9
def label(n: i64) -> i64:
    if small(n):
        return 1
    2
def main() -> ():
    label(5)
    ()
",
        true,
    );
    assert!(calls(&function(&ops, "label").unwrap().body).is_empty());
}
