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
    let plan = find(&ops, float, None).unwrap();
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
    assert_eq!(find(&ops, float, None), None);
    // The first reading what the second stores is the value from before.
    let ops = [load(2), sqrt(), store(3), load(1), sqrt(), store(2)];
    assert_eq!(
        find(&ops, float, None).unwrap().stores,
        [(3, 0, 0), (2, 0, 1)]
    );
    // Both writing one local leaves the second's value.
    let ops = [load(1), sqrt(), store(3), load(2), sqrt(), store(3)];
    assert_eq!(find(&ops, float, None).unwrap().stores, [(3, 0, 1)]);
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
    assert_eq!(find(&cheap, float, None), None);
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
    assert_eq!(find(&shapes, float, None), None);
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
    assert_eq!(find(&constants, float, None), None);
    let typed = [load(1), sqrt(), store(2), load(3), sqrt(), store(12)];
    assert_eq!(find(&typed, float, None), None);
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
    assert_eq!(find(&open, float, None), None);
    let under = [sqrt(), store(2), sqrt(), store(4)];
    assert_eq!(find(&under, float, None), None);
    // One statement, or an operation between the two.
    assert_eq!(find(&[load(1), sqrt(), store(2)], float, None), None);
    let apart = [
        load(1),
        sqrt(),
        store(2),
        Op::PushUnit,
        load(3),
        sqrt(),
        store(4),
    ];
    assert_eq!(find(&apart, float, None), None);
}

#[test]
fn integer_constant_assignments_between_statements_are_carried() {
    let lane = |count, from, to| [int(count), store(20), load(from), sqrt(), store(to)];
    let mut ops = lane(1, 1, 2).to_vec();
    ops.extend(lane(2, 3, 4));
    let plan = find(&ops, float, None).unwrap();
    assert_eq!(plan.ops, 5);
    assert_eq!(plan.integers, [0]);
    assert_eq!(plan.stores, [(2, 0, 0), (4, 0, 1)]);
    // An integer stored to an `f64` local, or used as an operand.
    ops[1] = store(5);
    ops[6] = store(5);
    assert_eq!(find(&ops, float, None), None);
    let operand = [int(1), sqrt(), store(2), int(1), sqrt(), store(4)];
    assert_eq!(find(&operand, float, None), None);
}

fn body_loop(body: Vec<Op>) -> Op {
    Op::Loop {
        condition: Vec::new().into(),
        body: body.into(),
    }
}
/// `out = a - b`.
fn difference(a: u8, b: u8, out: u8) -> [Op; 4] {
    [load(a), load(b), Op::Sub, store(out)]
}
fn all(_: u8) -> bool {
    true
}

#[test]
fn cheap_statements_pack_only_over_rows() {
    let mut ops = difference(1, 3, 5).to_vec();
    ops.extend(difference(2, 4, 6));
    assert_eq!(find(&ops, float, None), None);
    let mut seen = Vec::new();
    let mut rows = |reads: &[(u8, u8)]| {
        seen = reads.to_vec();
        true
    };
    let plan = find(&ops, float, Some(&mut rows)).unwrap();
    assert_eq!(seen, [(1, 2), (3, 4)]);
    assert_eq!((plan.costly, plan.arithmetic), (false, 1));
    assert_eq!(find(&ops, float, Some(&mut |_| false)), None);
    // One local in both lanes is no row; it is read as a splat.
    let mut scaled = vec![load(0), load(1), Op::Mul, store(5)];
    scaled.extend([load(0), load(2), Op::Mul, store(6)]);
    let mut rows = |reads: &[(u8, u8)]| reads == [(1, 2)];
    assert!(find(&scaled, float, Some(&mut rows)).is_some());
    // Copies do no arithmetic, and constants name no row.
    let copies = [load(1), store(5), load(2), store(6)];
    assert_eq!(find(&copies, float, Some(&mut |_| true)), None);
    let mut splats = vec![load(1), load(1), Op::Add, store(5)];
    splats.extend([load(1), load(1), Op::Add, store(6)]);
    assert_eq!(find(&splats, float, Some(&mut |_| true)), None);
}

#[test]
fn rows_are_the_pairs_cheap_loop_statements_read() {
    let mut body = difference(1, 3, 5).to_vec();
    body.extend(difference(2, 4, 6));
    let ops = [body_loop(body.clone())];
    assert_eq!(rows(&ops, float, all), [(1, 2), (3, 4)]);
    // Outside a loop nothing packs.
    assert!(rows(&body, float, all).is_empty());
    // A parameter is in no row, so the statements that read it stay scalar.
    assert!(rows(&ops, float, |local| local != 3).is_empty());
    // A borrowed local has an address.
    let mut borrowed = ops.to_vec();
    borrowed.push(Op::BorrowLocal(4, false));
    assert!(rows(&borrowed, float, all).is_empty());
}

#[test]
fn a_local_is_in_one_row() {
    // The second run pairs local 1 with another partner than the first.
    let mut body = difference(1, 3, 5).to_vec();
    body.extend(difference(2, 4, 6));
    body.extend([int(1), store(20)]);
    body.extend(difference(1, 3, 7));
    body.extend(difference(8, 3, 9));
    assert_eq!(rows(&[body_loop(body)], float, all), [(1, 2), (3, 4)]);
}

#[test]
fn scalar_uses_outweigh_a_row() {
    let mut body = difference(1, 3, 5).to_vec();
    body.extend(difference(2, 4, 6));
    // One scalar write of a lane costs two instructions; the run saves one.
    let mut written = body.clone();
    written.extend([load(0), store(4)]);
    assert!(rows(&[body_loop(written)], float, all).is_empty());
    // A read of the first lane is free, and one of the second costs one.
    let mut read = body.clone();
    read.extend([load(1), store(21)]);
    assert_eq!(rows(&[body_loop(read)], float, all), [(1, 2), (3, 4)]);
    let mut read = body.clone();
    read.extend([load(2), store(21)]);
    assert!(rows(&[body_loop(read)], float, all).is_empty());
    // Scalar uses outside the loop weigh an eighth of those inside.
    let mut outside = vec![load(0), store(1), load(0), store(2), load(0), store(3)];
    outside.extend([load(0), store(4), body_loop(body)]);
    assert_eq!(rows(&outside, float, all), [(1, 2), (3, 4)]);
}
