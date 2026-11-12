use super::*;

const D2: u8 = 9;

fn load(i: u8) -> Op {
    Op::LoadLocal(i)
}
fn store(i: u8) -> Op {
    Op::StoreLocal(i)
}
fn sqrt() -> Op {
    Op::Numeric(NumericOp::Sqrt)
}
/// `d2 = x * x; out = scale / (d2 * d2.sqrt())`, as n-body's magnitudes.
fn magnitude(x: u8, out: u8) -> Vec<Op> {
    vec![
        load(x),
        load(x),
        Op::Mul,
        store(D2),
        load(0),
        load(D2),
        load(D2),
        sqrt(),
        Op::Mul,
        Op::Div,
        store(out),
    ]
}
fn int(value: i64) -> Op {
    Op::PushInt(crate::value::Value::I64(value))
}
fn float(local: u8) -> bool {
    local < 10
}

#[test]
fn statements_of_one_shape_pack() {
    let mut ops = magnitude(1, 3);
    ops.extend(magnitude(2, 4));
    ops.push(Op::Return);
    let plan = find(&ops, float).unwrap();
    assert_eq!(plan.ops, 11);
    // The first lane's temporary is overwritten by the second's.
    assert_eq!(plan.stores, [(3, 1, 0), (D2, 0, 1), (4, 1, 1)]);
    assert_eq!(plan.steps[4], Step::Read(0, 0));
    assert_eq!(plan.steps[5], Step::Result(0));
}

#[test]
fn a_lane_that_reads_the_other_lanes_result_stays_scalar() {
    // The second statement reads what the first stores.
    let ops = [load(1), sqrt(), store(2), load(2), sqrt(), store(3)];
    assert_eq!(find(&ops, float), None);
    // The first reading what the second stores is the value from before.
    let ops = [load(2), sqrt(), store(3), load(1), sqrt(), store(2)];
    assert_eq!(find(&ops, float).unwrap().stores, [(3, 0, 0), (2, 0, 1)]);
    // Both writing one local leaves the second's value.
    let ops = [load(1), sqrt(), store(3), load(2), sqrt(), store(3)];
    assert_eq!(find(&ops, float).unwrap().stores, [(3, 0, 1)]);
}

#[test]
fn other_statements_stay_scalar() {
    // Nothing costly.
    let cheap = [
        load(1),
        load(1),
        Op::Add,
        store(2),
        load(3),
        load(3),
        Op::Add,
        store(4),
    ];
    assert_eq!(find(&cheap, float), None);
    // Different shapes, constants, or a local that is no `f64`.
    let shapes = [
        load(1),
        sqrt(),
        store(2),
        load(3),
        load(3),
        Op::Div,
        store(4),
    ];
    assert_eq!(find(&shapes, float), None);
    let constant = |bits| Op::PushFloat { bits, ty: Ty::F64 };
    let constants = [
        constant(1),
        load(1),
        Op::Div,
        store(2),
        constant(2),
        load(3),
        Op::Div,
        store(4),
    ];
    assert_eq!(find(&constants, float), None);
    let typed = [load(1), sqrt(), store(2), load(3), sqrt(), store(12)];
    assert_eq!(find(&typed, float), None);
    // An operand left on the stack, or one taken from it.
    let open = [
        load(1),
        load(1),
        sqrt(),
        store(2),
        load(3),
        load(3),
        sqrt(),
        store(4),
    ];
    assert_eq!(find(&open, float), None);
    let under = [sqrt(), store(2), sqrt(), store(4)];
    assert_eq!(find(&under, float), None);
    // One statement, or an operation between the two.
    assert_eq!(find(&[load(1), sqrt(), store(2)], float), None);
    let apart = [
        load(1),
        sqrt(),
        store(2),
        Op::PushUnit,
        load(3),
        sqrt(),
        store(4),
    ];
    assert_eq!(find(&apart, float), None);
}

#[test]
fn integer_constant_assignments_between_statements_are_carried() {
    let lane = |count, from, to| [int(count), store(20), load(from), sqrt(), store(to)];
    let mut ops = lane(1, 1, 2).to_vec();
    ops.extend(lane(2, 3, 4));
    let plan = find(&ops, float).unwrap();
    assert_eq!(plan.ops, 5);
    assert_eq!(plan.integers, [0]);
    assert_eq!(plan.stores, [(2, 0, 0), (4, 0, 1)]);
    // An integer stored to an `f64` local, or used as an operand.
    ops[1] = store(5);
    ops[6] = store(5);
    assert_eq!(find(&ops, float), None);
    let operand = [int(1), sqrt(), store(2), int(1), sqrt(), store(4)];
    assert_eq!(find(&operand, float), None);
}
