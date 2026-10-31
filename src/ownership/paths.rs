//! Loan path steps beyond fields and literal list indices: elements at an
//! unknown index and slice view ranges (issue #593), and the overlap test
//! between two loan paths into one root. See "Disjoint slice views" in
//! book/src/design/language/05-ownership/02-borrowing.md.
use super::ELEMENT;

/// An element whose index is not known statically: a borrowed loop's
/// element, or a view row at a computed index. Loans derived from one such
/// loan share its place; any other loan of an element may be the same one.
pub const ANY_ELEMENT: usize = ELEMENT | 1 << (usize::BITS - 3);

/// Marks an element step as a run of elements, the range of a slice view.
/// Later steps index the view, not its owner.
const RANGE: usize = ELEMENT | 1 << (usize::BITS - 2);

/// Bits per encoded bound: a 2-bit kind above a 28-bit value.
const BOUND_BITS: u32 = 30;
const BOUND_VALUE: usize = (1 << 28) - 1;
const LITERAL_LIMIT: i64 = 1 << 27;

/// Literal list indices above this have no element step of their own.
pub const ELEMENT_LIMIT: usize = 1 << 32;

/// One bound of a slice view's range in a loan path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bound {
    /// Omitted: the start or the end of the viewed run.
    Open,
    /// An integer literal, possibly negative.
    Literal(i64),
    /// A non-`mut` `i64` binding, read when the view is created. The checker
    /// rejects assigning it while the view is live, so two views bounded by
    /// it read one value.
    Binding(u8),
    /// A `mut` binding, named in diagnostics: its value is unknown.
    Mutable(u8),
    /// Any other expression: its value is unknown.
    Computed,
}

impl Bound {
    fn encode(self) -> usize {
        match self {
            Bound::Open => 0,
            Bound::Literal(v) if (-LITERAL_LIMIT..LITERAL_LIMIT).contains(&v) => {
                1 << 28 | (v + LITERAL_LIMIT) as usize
            }
            Bound::Binding(slot) => 2 << 28 | slot as usize,
            Bound::Mutable(slot) => 3 << 28 | (slot as usize + 1),
            Bound::Literal(_) | Bound::Computed => 3 << 28,
        }
    }

    fn decode(bits: usize) -> Self {
        let value = bits & BOUND_VALUE;
        match bits >> 28 & 3 {
            0 => Bound::Open,
            1 => Bound::Literal(value as i64 - LITERAL_LIMIT),
            2 => Bound::Binding(value as u8),
            _ if value == 0 => Bound::Computed,
            _ => Bound::Mutable((value - 1) as u8),
        }
    }

    /// Whether two reads of the bound give one value.
    fn fixed(self) -> bool {
        !matches!(self, Bound::Mutable(_) | Bound::Computed)
    }

    /// The bound's position before clamping and its sign class, when it is a
    /// literal or an omitted start. Bounds clamp to the owner's length
    /// monotonically within one sign class: a nonnegative value `v` to
    /// `min(v, len)` and a negative one to `max(len + v, 0)`.
    fn literal(self, start: bool) -> Option<i64> {
        match self {
            Bound::Open if start => Some(0),
            Bound::Literal(v) => Some(v),
            _ => None,
        }
    }
}

/// The loan path step of a view `[start:stop]`.
pub fn range_step(start: Bound, stop: Bound) -> usize {
    RANGE | start.encode() << BOUND_BITS | stop.encode()
}

/// The bounds of a range step.
pub fn range_bounds(step: usize) -> Option<(Bound, Bound)> {
    (step & RANGE == RANGE).then(|| {
        (
            Bound::decode(step >> BOUND_BITS),
            Bound::decode(step & ((1 << BOUND_BITS) - 1)),
        )
    })
}

/// Whether a run that stops at `stop` ends at or before the position where a
/// run starting at `start` begins, after both clamp to one length. A view
/// whose stop precedes its start is empty, so its rows lie before its stop.
fn ends_before(stop: Bound, start: Bound) -> bool {
    match (stop, start) {
        (Bound::Binding(a), Bound::Binding(b)) => a == b,
        _ => match (stop.literal(false), start.literal(true)) {
            (Some(a), Some(b)) => a == b || ((a < 0) == (b < 0) && a <= b),
            _ => false,
        },
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Relation {
    Same,
    Disjoint,
    Overlap,
}

fn relation(x: usize, y: usize) -> Relation {
    let element = |s: usize| s & ELEMENT != 0;
    if x == ANY_ELEMENT || y == ANY_ELEMENT {
        return Relation::Overlap;
    }
    match (range_bounds(x), range_bounds(y)) {
        (Some((a, b)), Some((c, d))) => {
            if ends_before(b, c) || ends_before(d, a) || ends_before(b, a) || ends_before(d, c) {
                Relation::Disjoint
            } else if x == y && a.fixed() && b.fixed() {
                Relation::Same
            } else {
                Relation::Overlap
            }
        }
        (Some((start, stop)), None) | (None, Some((start, stop))) => {
            let index = if range_bounds(x).is_some() { y } else { x };
            if !element(index) {
                return Relation::Overlap;
            }
            // The element exists, so `index < len`: it precedes a clamped
            // nonnegative start greater than it, and follows a clamped
            // nonnegative stop at or below it.
            let index = (index & !ELEMENT) as i64;
            let before = start.literal(true).is_some_and(|s| s >= 0 && index < s);
            let after = matches!(stop, Bound::Literal(s) if s >= 0 && index >= s);
            if before || after || ends_before(stop, start) {
                Relation::Disjoint
            } else {
                Relation::Overlap
            }
        }
        (None, None) if element(x) != element(y) => Relation::Overlap,
        (None, None) if x == y => Relation::Same,
        (None, None) => Relation::Disjoint,
    }
}

/// Whether loan paths `a` and `b` into one root may overlap. Their first
/// `shared` steps come from a common ancestor loan, so they denote one place
/// even where a step is not static, such as a loop element.
pub fn overlap(a: &[usize], b: &[usize], shared: usize) -> bool {
    for (&x, &y) in a.iter().zip(b).skip(shared) {
        match relation(x, y) {
            Relation::Same => {}
            Relation::Disjoint => return false,
            Relation::Overlap => return true,
        }
    }
    true
}

/// The rule that two conflicting views of one owner did not meet.
pub const DISJOINT_VIEWS: &str = "views of one owner are disjoint when one stops where the other starts, at one literal or non-`mut` `i64` name such as `[:m]` and `[m:]`, or when both have nonnegative literal bounds that do not overlap";

/// Whether `path` holds a view range.
pub fn has_range(path: &[usize]) -> bool {
    path.iter().any(|&step| range_bounds(step).is_some())
}

/// Whether `path` holds a view range bounded by binding `slot`.
pub fn bounded_by(path: &[usize], slot: u8) -> bool {
    path.iter().any(|&step| {
        range_bounds(step)
            .is_some_and(|(a, b)| a == Bound::Binding(slot) || b == Bound::Binding(slot))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use Bound::*;

    fn r(a: Bound, b: Bound) -> usize {
        range_step(a, b)
    }

    fn disjoint(x: usize, y: usize) -> bool {
        !overlap(&[x], &[y], 0) && !overlap(&[y], &[x], 0)
    }

    #[test]
    fn ranges() {
        assert!(disjoint(r(Open, Binding(3)), r(Binding(3), Open)));
        assert!(disjoint(r(Open, Literal(2)), r(Literal(2), Open)));
        assert!(disjoint(r(Open, Literal(-2)), r(Literal(-2), Open)));
        assert!(disjoint(
            r(Literal(0), Literal(3)),
            r(Literal(5), Literal(8))
        ));
        assert!(disjoint(r(Literal(-5), Literal(-3)), r(Literal(-2), Open)));
        assert!(disjoint(r(Computed, Literal(4)), r(Literal(4), Computed)));
        // An empty run overlaps nothing.
        assert!(disjoint(r(Literal(4), Literal(2)), r(Open, Open)));
        assert!(disjoint(r(Binding(1), Binding(1)), r(Open, Open)));

        assert!(!disjoint(r(Open, Binding(3)), r(Binding(4), Open)));
        assert!(!disjoint(r(Open, Literal(3)), r(Literal(2), Open)));
        // Signs differ, so the clamped order depends on the length.
        assert!(!disjoint(r(Open, Literal(-2)), r(Literal(2), Open)));
        assert!(!disjoint(r(Open, Literal(2)), r(Literal(-2), Open)));
        assert!(!disjoint(r(Computed, Computed), r(Computed, Computed)));
        assert!(!disjoint(r(Open, Mutable(3)), r(Mutable(3), Open)));
        assert_eq!(
            range_bounds(r(Mutable(0), Computed)),
            Some((Mutable(0), Computed))
        );
        assert!(!disjoint(r(Open, Open), r(Open, Open)));
    }

    #[test]
    fn elements() {
        let e = |k: usize| ELEMENT | k;
        assert!(disjoint(e(1), r(Literal(2), Open)));
        assert!(disjoint(e(4), r(Open, Literal(4))));
        assert!(!disjoint(e(2), r(Literal(2), Open)));
        assert!(!disjoint(e(1), r(Literal(-3), Open)));
        assert!(!disjoint(e(9), r(Open, Binding(2))));
        assert!(!disjoint(ANY_ELEMENT, e(0)));
        assert!(!disjoint(ANY_ELEMENT, ANY_ELEMENT));
        assert!(!disjoint(ANY_ELEMENT, r(Open, Literal(1))));
    }

    #[test]
    fn nested_paths() {
        let e = |k: usize| ELEMENT | k;
        let left = r(Open, Binding(0));
        // Rows of one view, and narrower views of it.
        assert!(!overlap(&[left, e(0)], &[left, e(1)], 0));
        assert!(!overlap(
            &[left, r(Open, Literal(1))],
            &[left, r(Literal(1), Open)],
            0
        ));
        // Overlapping views index their owner differently.
        let other = r(Literal(1), Open);
        assert!(overlap(&[r(Open, Open), e(1)], &[other, e(0)], 0));
        // Loop element fields: distinct below a common ancestor only.
        assert!(overlap(&[ANY_ELEMENT, 0], &[ANY_ELEMENT, 1], 0));
        assert!(!overlap(&[ANY_ELEMENT, 0], &[ANY_ELEMENT, 1], 1));
        assert!(overlap(&[ANY_ELEMENT, 0], &[ANY_ELEMENT, 0], 1));
        // Fields and elements never share a position; stay conservative.
        assert!(overlap(&[0], &[e(0)], 0));
    }
}
