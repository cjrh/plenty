//! Inclusive integer intervals the checker derives for integer expressions.
//! An interval is a proof: every value the expression can produce lies in
//! it. Arithmetic whose interval fits its type cannot overflow, so it adds
//! no `OverflowError` and native lowering emits no check for it. The rules
//! are in `book/src/design/language/03-types/intervals.md`.
//!
//! Bounds are `i128`, which holds every value of every integer type. The
//! computation itself uses checked `i128` operations: a bound that does not
//! fit `i128` means the operation is not proved.
use crate::numeric::Divisor;
use crate::op::Ty;

/// `lo..hi`, both inclusive, with `lo <= hi`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Interval {
    pub lo: i128,
    pub hi: i128,
}

impl std::fmt::Display for Interval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}..{}", self.lo, self.hi)
    }
}

/// An integer operator with interval rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operator {
    Add,
    Sub,
    Mul,
    FloorDiv,
    Modulo,
    And,
    ShiftLeft,
    ShiftRight,
}

impl Operator {
    pub fn named(op: &str) -> Option<Self> {
        Some(match op {
            "+" => Self::Add,
            "-" => Self::Sub,
            "*" => Self::Mul,
            "//" => Self::FloorDiv,
            "%" => Self::Modulo,
            "&" => Self::And,
            "<<" => Self::ShiftLeft,
            ">>" => Self::ShiftRight,
            _ => return None,
        })
    }
}

/// What the operand intervals prove about one operation on a type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// Every result the operation produces when it does not fail; inside
    /// the type's range.
    pub value: Interval,
    /// The exact result can lie outside the type.
    pub overflows: bool,
    /// The divisor of `//` or `%` can be zero.
    pub divides_by_zero: bool,
    /// The count of a shift can lie outside `0..width - 1`.
    pub bad_count: bool,
}

impl Outcome {
    /// No operand value makes the operation fail.
    pub fn proved(self) -> bool {
        !(self.overflows || self.divides_by_zero || self.bad_count)
    }
}

fn floor_div(a: i128, b: i128) -> Option<i128> {
    let quotient = a.checked_div(b)?;
    let remainder = a.checked_rem(b)?;
    if remainder != 0 && ((remainder < 0) != (b < 0)) {
        quotient.checked_sub(1)
    } else {
        Some(quotient)
    }
}

/// The smallest interval holding every value in `values`, or `None` when
/// one of them does not fit `i128`.
fn hull(values: impl IntoIterator<Item = Option<i128>>) -> Option<Interval> {
    let mut bounds: Option<Interval> = None;
    for value in values {
        let value = value?;
        bounds = Some(match bounds {
            Some(Interval { lo, hi }) => Interval {
                lo: lo.min(value),
                hi: hi.max(value),
            },
            None => Interval::point(value),
        });
    }
    bounds
}

impl Interval {
    pub fn point(value: i128) -> Self {
        Self {
            lo: value,
            hi: value,
        }
    }

    pub fn new(lo: i128, hi: i128) -> Option<Self> {
        (lo <= hi).then_some(Self { lo, hi })
    }

    /// Every value of the integer type `ty`.
    pub fn of(ty: &Ty) -> Option<Self> {
        let (min, end) = ty.int_range()?;
        Some(Self {
            lo: min,
            hi: end - 1,
        })
    }

    pub fn contains(self, value: i128) -> bool {
        self.lo <= value && value <= self.hi
    }

    pub fn within(self, outer: Self) -> bool {
        outer.lo <= self.lo && self.hi <= outer.hi
    }

    pub fn meet(self, other: Self) -> Option<Self> {
        Self::new(self.lo.max(other.lo), self.hi.min(other.hi))
    }

    fn join(self, other: Self) -> Self {
        Self {
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
        }
    }

    fn corners(self, other: Self, f: impl Fn(i128, i128) -> Option<i128>) -> Option<Self> {
        hull([
            f(self.lo, other.lo),
            f(self.lo, other.hi),
            f(self.hi, other.lo),
            f(self.hi, other.hi),
        ])
    }

    /// The parts of a divisor interval below and above zero.
    fn nonzero_parts(self) -> [Option<Self>; 2] {
        [
            Self::new(self.lo, self.hi.min(-1)),
            Self::new(self.lo.max(1), self.hi),
        ]
    }

    /// The exact quotients over the nonzero divisors. `x // y` is monotonic
    /// in each operand while `y` keeps its sign, so the corners of each
    /// part bound it.
    fn floor_div(self, divisor: Self) -> Option<Self> {
        let mut result: Option<Self> = None;
        for part in divisor.nonzero_parts().into_iter().flatten() {
            let quotients = self.corners(part, floor_div)?;
            result = Some(result.map_or(quotients, |earlier| earlier.join(quotients)));
        }
        result
    }

    /// The floored remainders over the nonzero divisors: the remainder has
    /// the divisor's sign and a smaller magnitude, and a dividend with the
    /// same sign also bounds it.
    fn modulo(self, divisor: Self) -> Option<Self> {
        let [negative, positive] = divisor.nonzero_parts();
        let positive = positive.map(|part| Self {
            lo: 0,
            hi: if self.lo >= 0 {
                self.hi.min(part.hi - 1)
            } else {
                part.hi - 1
            },
        });
        let negative = negative.map(|part| Self {
            lo: if self.hi <= 0 {
                self.lo.max(part.lo + 1)
            } else {
                part.lo + 1
            },
            hi: 0,
        });
        match (negative, positive) {
            (Some(a), Some(b)) => Some(a.join(b)),
            (a, b) => a.or(b),
        }
    }

    /// `a & b` has no bit that a nonnegative operand lacks, so it is at
    /// least zero and at most the smallest nonnegative operand's bound.
    fn and(self, other: Self) -> Option<Self> {
        let hi = [self, other]
            .into_iter()
            .filter(|operand| operand.lo >= 0)
            .map(|operand| operand.hi)
            .min()?;
        Some(Self { lo: 0, hi })
    }

    /// What `a op b` on `ty` can do, for `a` and `b` inside `ty`'s range.
    pub fn operation(operator: Operator, ty: &Ty, a: Self, b: Self) -> Outcome {
        let full = Self::of(ty).expect("interval arithmetic requires an integer type");
        debug_assert!(a.within(full) && b.within(full));
        let width = i128::from((full.hi - full.lo + 1).trailing_zeros());
        let counts = Self {
            lo: 0,
            hi: width - 1,
        };
        let shifted = |f: fn(i128, i128) -> Option<i128>| {
            // A count outside `0..width - 1` fails, so only valid counts
            // produce a value.
            a.corners(b.meet(counts)?, f)
        };
        let exact = match operator {
            Operator::Add => a.corners(b, i128::checked_add),
            Operator::Sub => a.corners(b, i128::checked_sub),
            Operator::Mul => a.corners(b, i128::checked_mul),
            Operator::FloorDiv => a.floor_div(b),
            Operator::Modulo => a.modulo(b),
            Operator::And => a.and(b),
            // `<<` discards high bits, so a product outside the type says
            // nothing about the result.
            Operator::ShiftLeft => shifted(|x, count| x.checked_mul(1i128 << count))
                .filter(|product| product.within(full)),
            Operator::ShiftRight => shifted(|x, count| floor_div(x, 1i128 << count)),
        };
        let overflows = match operator {
            Operator::Add | Operator::Sub | Operator::Mul | Operator::FloorDiv => {
                exact.is_none_or(|exact| !exact.within(full))
            }
            Operator::Modulo | Operator::And | Operator::ShiftLeft | Operator::ShiftRight => false,
        };
        let division = matches!(operator, Operator::FloorDiv | Operator::Modulo);
        let shift = matches!(operator, Operator::ShiftLeft | Operator::ShiftRight);
        Outcome {
            value: exact.and_then(|exact| exact.meet(full)).unwrap_or(full),
            // A division whose divisor is only ever zero has no quotient to
            // overflow.
            overflows: overflows && !(division && b == Self::point(0)),
            divides_by_zero: division && b.contains(0),
            bad_count: shift && !b.within(counts),
        }
    }

    /// The proof a `//` or `%` with dividend `self` and `divisor` on `ty`
    /// hands to native lowering.
    pub fn divisor(self, ty: &Ty, divisor: Self) -> Divisor {
        let full = Self::of(ty).expect("interval arithmetic requires an integer type");
        // `MIN // -1` is the one quotient outside a signed type, and the one
        // pair on which the hardware division faults.
        let fits = full.lo < 0 && (self.lo > full.lo || !divisor.contains(-1));
        if divisor.lo > 0 {
            Divisor::Positive
        } else if divisor.hi < -1 {
            Divisor::BelowMinusOne
        } else if !divisor.contains(0) {
            if fits {
                Divisor::NonZeroFits
            } else {
                Divisor::NonZero
            }
        } else if fits {
            Divisor::Fits
        } else {
            Divisor::Any
        }
    }

    /// The interval of an integer conversion to `target` of a value in
    /// `self`, and whether every such value is representable.
    pub fn cast(self, target: &Ty) -> (Self, bool) {
        let full = Self::of(target).expect("interval arithmetic requires an integer type");
        (self.meet(full).unwrap_or(full), self.within(full))
    }

    /// [`Interval::cast`] for any target: a float target proves nothing.
    pub fn cast_or_full(self, target: &Ty) -> (Self, bool) {
        if Self::of(target).is_some() {
            self.cast(target)
        } else {
            (self, false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Interval, Operator, Outcome};
    use crate::numeric::{Checked, Divisor};
    use crate::op::Ty;

    const OPERATORS: [Operator; 8] = [
        Operator::Add,
        Operator::Sub,
        Operator::Mul,
        Operator::FloorDiv,
        Operator::Modulo,
        Operator::And,
        Operator::ShiftLeft,
        Operator::ShiftRight,
    ];

    /// What the operation does on values of an 8-bit type: the result, or
    /// which failure.
    #[derive(Debug, PartialEq)]
    enum Actual {
        Value(i128),
        Overflow,
        ZeroDivision,
        BadCount,
    }

    fn floor(a: i128, b: i128) -> i128 {
        (a as f64 / b as f64).floor() as i128
    }

    fn actual(operator: Operator, signed: bool, a: i128, b: i128) -> Actual {
        let full = if signed { -128..=127 } else { 0..=255 };
        let wrap = |value: i128| {
            if signed {
                i128::from(value as i8)
            } else {
                i128::from(value as u8)
            }
        };
        let exact = match operator {
            Operator::Add => a + b,
            Operator::Sub => a - b,
            Operator::Mul => a * b,
            Operator::FloorDiv | Operator::Modulo if b == 0 => return Actual::ZeroDivision,
            Operator::FloorDiv => floor(a, b),
            Operator::Modulo => a - floor(a, b) * b,
            Operator::And => return Actual::Value(wrap(i128::from(a as u8 & b as u8))),
            Operator::ShiftLeft | Operator::ShiftRight if !(0..8).contains(&b) => {
                return Actual::BadCount;
            }
            Operator::ShiftLeft => return Actual::Value(wrap(a << b)),
            Operator::ShiftRight => a >> b,
        };
        if full.contains(&exact) {
            Actual::Value(exact)
        } else {
            Actual::Overflow
        }
    }

    fn intervals(signed: bool) -> Vec<Interval> {
        let points: &[i128] = if signed {
            &[-128, -127, -9, -2, -1, 0, 1, 2, 7, 8, 100, 126, 127]
        } else {
            &[0, 1, 2, 7, 8, 9, 100, 127, 128, 254, 255]
        };
        let mut all = Vec::new();
        for (i, &lo) in points.iter().enumerate() {
            for &hi in &points[i..] {
                all.push(Interval { lo, hi });
            }
        }
        all
    }

    /// Every pair of 8-bit values in every pair of sampled intervals: the
    /// outcome names each failure that happens and holds each result.
    #[test]
    fn outcomes_hold_for_every_value_of_the_eight_bit_types() {
        for (ty, signed) in [(Ty::I8, true), (Ty::U8, false)] {
            let all = intervals(signed);
            for operator in OPERATORS {
                for &a in &all {
                    for &b in &all {
                        let outcome = Interval::operation(operator, &ty, a, b);
                        check(operator, signed, a, b, outcome);
                    }
                }
            }
        }
    }

    fn check(operator: Operator, signed: bool, a: Interval, b: Interval, outcome: Outcome) {
        let full = Interval::of(if signed { &Ty::I8 } else { &Ty::U8 }).unwrap();
        assert!(outcome.value.within(full));
        let (mut overflowed, mut divided, mut miscounted) = (false, false, false);
        for x in a.lo..=a.hi {
            for y in b.lo..=b.hi {
                match actual(operator, signed, x, y) {
                    Actual::Value(value) => assert!(
                        outcome.value.contains(value),
                        "{x} {operator:?} {y} = {value} is outside {} for {a} and {b}",
                        outcome.value
                    ),
                    Actual::Overflow => overflowed = true,
                    Actual::ZeroDivision => divided = true,
                    Actual::BadCount => miscounted = true,
                }
            }
        }
        // A failure that happens is always reported, and the failures
        // decided exactly are reported only when they happen.
        assert!(!overflowed || outcome.overflows, "{operator:?} {a} {b}");
        assert_eq!(divided, outcome.divides_by_zero, "{operator:?} {a} {b}");
        assert_eq!(miscounted, outcome.bad_count, "{operator:?} {a} {b}");
        if matches!(operator, Operator::Add | Operator::Sub | Operator::FloorDiv) {
            assert_eq!(overflowed, outcome.overflows, "{operator:?} {a} {b}");
        }
    }

    /// The divisor proof never claims more than the intervals hold, and a
    /// division it leaves without a failure has none.
    #[test]
    fn divisor_proofs_hold_for_every_value_of_the_eight_bit_types() {
        for (ty, signed) in [(Ty::I8, true), (Ty::U8, false)] {
            let all = intervals(signed);
            for &a in &all {
                for &b in &all {
                    let proof = a.divisor(&ty, b);
                    match proof {
                        Divisor::Positive => assert!(b.lo > 0),
                        Divisor::BelowMinusOne => assert!(b.hi < -1),
                        Divisor::NonZero | Divisor::NonZeroFits => assert!(!b.contains(0)),
                        Divisor::Any | Divisor::Fits => {}
                    }
                    let pair = signed && a.contains(-128) && b.contains(-1);
                    assert!(proof.may_be_minus_one() || !pair, "{a} {b}");
                    for (operator, checked) in [
                        (Operator::FloorDiv, Checked::FloorDiv(proof)),
                        (Operator::Modulo, Checked::Modulo(proof)),
                    ] {
                        let outcome = Interval::operation(operator, &ty, a, b);
                        assert!(checked.divides() || !outcome.divides_by_zero);
                        assert!(checked.overflows(&ty) || !outcome.overflows, "{a} {b}");
                    }
                }
            }
        }
    }

    /// The widest operands: bounds that do not fit `i128` prove nothing.
    #[test]
    fn wide_operands_do_not_overflow_the_computation() {
        let full = Interval::of(&Ty::U64).unwrap();
        for operator in OPERATORS {
            let outcome = Interval::operation(operator, &Ty::U64, full, full);
            assert!(outcome.value.within(full), "{operator:?}");
        }
        assert!(Interval::operation(Operator::Mul, &Ty::U64, full, full).overflows);
        let signed = Interval::of(&Ty::I64).unwrap();
        for operator in OPERATORS {
            let outcome = Interval::operation(operator, &Ty::I64, signed, signed);
            assert!(outcome.value.within(signed), "{operator:?}");
        }
        let min = Interval::point(i128::from(i64::MIN));
        let minus_one = Interval::point(-1);
        assert!(Interval::operation(Operator::FloorDiv, &Ty::I64, min, minus_one).overflows);
        assert!(!Interval::operation(Operator::Modulo, &Ty::I64, min, minus_one).overflows);
        assert!(Interval::operation(Operator::Sub, &Ty::I64, Interval::point(0), min).overflows);
        let above = Interval {
            lo: min.lo + 1,
            hi: signed.hi,
        };
        let negated = Interval::operation(Operator::Sub, &Ty::I64, Interval::point(0), above);
        assert!(negated.proved());
        assert_eq!(negated.value, above);
    }

    #[test]
    fn casts_keep_the_part_of_the_source_inside_the_target() {
        let byte = Interval::of(&Ty::U8).unwrap();
        assert_eq!(byte.cast(&Ty::I64), (byte, true));
        let signed = Interval::of(&Ty::I8).unwrap();
        assert_eq!(signed.cast(&Ty::U8), (Interval { lo: 0, hi: 127 }, false));
        let high = Interval { lo: 300, hi: 400 };
        assert_eq!(high.cast(&Ty::U8), (byte, false));
    }
}
