//! Compile-time dimension algebra. Normal forms contain only nonzero i8
//! exponents, ordered by nominal base identity. They never describe a value's
//! storage or emit runtime operations.
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Clone)]
pub struct Base {
    pub name: Arc<str>,
    /// The coherent unit's symbol is presentation metadata, not identity.
    pub symbol: Arc<str>,
}

impl fmt::Debug for Base {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Base")
            .field("name", &self.name)
            .field("symbol", &self.symbol)
            .finish()
    }
}

impl PartialEq for Base {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl Eq for Base {}
impl Hash for Base {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Dimension(Vec<(Arc<Base>, i8)>);

/// One compilation owns its canonical dimension forms. Handles share the
/// normalized vector, including when an alias or expression rediscovers it.
#[derive(Default)]
pub struct Interner {
    entries: crate::hash::HashSet<Arc<Dimension>>,
}

impl Interner {
    pub fn intern(&mut self, dimension: Dimension) -> Arc<Dimension> {
        if let Some(existing) = self.entries.get(&dimension) {
            return existing.clone();
        }
        let dimension = Arc::new(dimension);
        self.entries.insert(dimension.clone());
        dimension
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Overflow(Arc<str>),
    ZeroRoot,
    NonIntegralRoot {
        base: Arc<str>,
        exponent: i8,
        root: i64,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Overflow(base) => {
                write!(f, "dimension exponent for `{base}` is outside -128..127")
            }
            Self::ZeroRoot => f.write_str("a dimension root must have a nonzero degree"),
            Self::NonIntegralRoot {
                base,
                exponent,
                root,
            } => write!(
                f,
                "not a whole dimension: `{base}` has exponent {exponent}, which {root} does not divide"
            ),
        }
    }
}

impl Dimension {
    pub fn bases(&self) -> impl Iterator<Item = &Base> {
        self.0.iter().map(|(base, _)| base.as_ref())
    }
    pub fn base(name: String, symbol: String) -> Self {
        Self(vec![(
            Arc::new(Base {
                name: name.into(),
                symbol: symbol.into(),
            }),
            1,
        )])
    }

    /// Merge already normalized operands, checking the resulting exponent
    /// rather than negating an i8 (the inverse of -128 is out of range).
    pub fn product(&self, other: &Self, divide: bool) -> Result<Self, Error> {
        let mut left = self.0.iter().peekable();
        let mut right = other.0.iter().peekable();
        let mut terms = Vec::with_capacity(self.0.len() + other.0.len());
        let sign = if divide { -1i64 } else { 1 };
        while left.peek().is_some() || right.peek().is_some() {
            let (base, exponent) = match (left.peek(), right.peek()) {
                (Some((a, _)), Some((b, _))) if a.name == b.name => {
                    let (base, a) = left.next().unwrap();
                    let (_, b) = right.next().unwrap();
                    (base, i64::from(*a) + sign * i64::from(*b))
                }
                (Some((a, _)), Some((b, _))) if a.name < b.name => {
                    let (base, exponent) = left.next().unwrap();
                    (base, i64::from(*exponent))
                }
                (Some(_), None) => {
                    let (base, exponent) = left.next().unwrap();
                    (base, i64::from(*exponent))
                }
                _ => {
                    let (base, exponent) = right.next().unwrap();
                    (base, sign * i64::from(*exponent))
                }
            };
            Self::push(&mut terms, base, exponent)?;
        }
        Ok(Self(terms))
    }

    pub fn power(&self, power: i64) -> Result<Self, Error> {
        let mut terms = Vec::with_capacity(self.0.len());
        for (base, exponent) in &self.0 {
            let exponent = i64::from(*exponent)
                .checked_mul(power)
                .ok_or_else(|| Error::Overflow(base.name.clone()))?;
            Self::push(&mut terms, base, exponent)?;
        }
        Ok(Self(terms))
    }

    pub fn root(&self, root: i64) -> Result<Self, Error> {
        if root == 0 {
            return Err(Error::ZeroRoot);
        }
        let mut terms = Vec::with_capacity(self.0.len());
        for (base, exponent) in &self.0 {
            let wide = i64::from(*exponent);
            if wide % root != 0 {
                return Err(Error::NonIntegralRoot {
                    base: base.name.clone(),
                    exponent: *exponent,
                    root,
                });
            }
            Self::push(&mut terms, base, wide / root)?;
        }
        Ok(Self(terms))
    }

    fn push(
        terms: &mut Vec<(Arc<Base>, i8)>,
        base: &Arc<Base>,
        exponent: i64,
    ) -> Result<(), Error> {
        if exponent != 0 {
            terms.push((
                base.clone(),
                i8::try_from(exponent).map_err(|_| Error::Overflow(base.name.clone()))?,
            ));
        }
        Ok(())
    }

    /// Include nominal names, never symbols, in specialization/cache identity.
    /// Length prefixes keep distinct names unambiguous even in imported modules.
    pub fn identity_name(&self) -> String {
        use fmt::Write;
        let mut name = String::from("dimension[");
        for (base, exponent) in &self.0 {
            write!(name, "{}:{}:{exponent};", base.name.len(), base.name).unwrap();
        }
        name.push(']');
        name
    }
}

impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return f.write_str("One");
        }
        for (i, (base, exponent)) in self.0.iter().enumerate() {
            if i != 0 {
                f.write_str(" * ")?;
            }
            if *exponent == 1 {
                f.write_str(&base.name)?;
            } else {
                write!(f, "Pow[{}, {exponent}]", base.name)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(i: usize) -> Dimension {
        Dimension::base(format!("base{i}"), format!("u{i}"))
    }

    #[test]
    fn identity_is_nominal_and_not_the_unit_symbol() {
        let a = Dimension::base("a.Length".into(), "m".into());
        let b = Dimension::base("b.Length".into(), "m".into());
        assert_ne!(a, b);
        assert_ne!(a.identity_name(), b.identity_name());
        assert_eq!(a, Dimension::base("a.Length".into(), "metre".into()));
        assert_eq!(a.bases().next().unwrap().symbol.as_ref(), "m");
    }

    #[test]
    fn equivalent_forms_share_the_interned_type() {
        let mut types = Interner::default();
        let length = types.intern(base(0));
        let same = types.intern(
            base(0)
                .product(&base(1), true)
                .unwrap()
                .product(&base(1), false)
                .unwrap(),
        );
        assert!(Arc::ptr_eq(&length, &same));
        let one = types.intern(Dimension::default());
        let cancelled = types.intern(base(0).product(&base(0), true).unwrap());
        assert!(Arc::ptr_eq(&one, &cancelled));
    }

    #[test]
    fn exponent_boundaries_do_not_wrap_or_overflow_the_host() {
        let d = base(0);
        for exponent in [-128, -127, 0, 1, 126, 127] {
            assert!(d.power(exponent).is_ok());
        }
        for exponent in [i64::MIN, -129, 128, i64::MAX] {
            assert!(matches!(d.power(exponent), Err(Error::Overflow(_))));
        }
        let low = d.power(-128).unwrap();
        assert!(low.root(-1).is_err());
        assert!(Dimension::default().product(&low, true).is_err());
        assert_eq!(low.product(&low, true).unwrap(), Dimension::default());
        assert_eq!(low.root(2).unwrap(), d.power(-64).unwrap());
        assert_eq!(Dimension::default().root(0), Err(Error::ZeroRoot));
        assert_eq!(
            Dimension::default().root(i64::MIN).unwrap(),
            Dimension::default()
        );
        assert!(d.power(2).unwrap().power(i64::MAX).is_err());
        assert!(d.power(-2).unwrap().power(i64::MIN).is_err());
    }

    #[test]
    fn roots_check_every_exponent() {
        for a in -128..=127 {
            for b in -128..=127 {
                let d = base(0)
                    .power(a)
                    .unwrap()
                    .product(&base(1).power(b).unwrap(), false)
                    .unwrap();
                for n in [-7, -3, 2, 3, 7] {
                    let root = d.root(n);
                    assert_eq!(root.is_ok(), a % n == 0 && b % n == 0);
                    if let Ok(root) = root {
                        assert_eq!(root.power(n).unwrap(), d);
                    }
                }
            }
        }
    }

    // The oracle uses fixed dense vectors and wide arithmetic, independently
    // of the production sorted sparse merge. Check errors at each expression
    // node: a later cancellation cannot rescue an overflowing subexpression.
    enum Expr {
        Base(usize),
        Product(Box<Self>, Box<Self>, bool),
        Power(Box<Self>, i64),
        Root(Box<Self>, i64),
    }

    impl Expr {
        fn reference(&self) -> Option<[i64; 4]> {
            let result = match self {
                Self::Base(i) => {
                    let mut v = [0; 4];
                    v[*i] = 1;
                    v
                }
                Self::Product(a, b, divide) => {
                    let a = a.reference()?;
                    let b = b.reference()?;
                    std::array::from_fn(|i| a[i] + if *divide { -b[i] } else { b[i] })
                }
                Self::Power(a, n) => a.reference()?.map(|e| e * n),
                Self::Root(a, n) => {
                    let a = a.reference()?;
                    if *n == 0 || a.iter().any(|e| e % n != 0) {
                        return None;
                    }
                    a.map(|e| e / n)
                }
            };
            result
                .iter()
                .all(|e| (-128..=127).contains(e))
                .then_some(result)
        }

        fn actual(&self) -> Result<Dimension, Error> {
            match self {
                Self::Base(i) => Ok(base(*i)),
                Self::Product(a, b, divide) => a.actual()?.product(&b.actual()?, *divide),
                Self::Power(a, n) => a.actual()?.power(*n),
                Self::Root(a, n) => a.actual()?.root(*n),
            }
        }
    }

    fn random(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    fn expression(seed: &mut u64, depth: u8) -> Expr {
        if depth == 0 {
            return Expr::Base((random(seed) % 4) as usize);
        }
        match random(seed) % 4 {
            0 | 1 => Expr::Product(
                Box::new(expression(seed, depth - 1)),
                Box::new(expression(seed, depth - 1)),
                random(seed) & 1 == 0,
            ),
            2 => Expr::Power(
                Box::new(expression(seed, depth - 1)),
                (random(seed) % 17) as i64 - 8,
            ),
            _ => Expr::Root(
                Box::new(expression(seed, depth - 1)),
                (random(seed) % 7) as i64 - 3,
            ),
        }
    }

    #[test]
    fn random_expressions_match_dense_reference_evaluator() {
        let mut seed = 701;
        let mut successes = 0;
        let mut rejections = 0;
        for _ in 0..10_000 {
            let e = expression(&mut seed, 4);
            let expected = e.reference();
            let actual = e.actual().ok();
            assert_eq!(actual.is_some(), expected.is_some());
            if let (Some(actual), Some(expected)) = (actual, expected) {
                successes += 1;
                let terms: Vec<_> = expected
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| **n != 0)
                    .map(|(i, n)| (format!("base{i}"), *n))
                    .collect();
                let actual: Vec<_> = actual
                    .0
                    .iter()
                    .map(|(base, n)| (base.name.to_string(), i64::from(*n)))
                    .collect();
                assert_eq!(actual, terms);
            } else {
                rejections += 1;
            }
        }
        assert!(successes > 100 && rejections > 100);
    }

    #[test]
    fn products_commute_associate_and_cancel() {
        let mut seed = 701;
        for _ in 0..1000 {
            let dimensions: Vec<_> = (0..3)
                .map(|_| {
                    let mut d = Dimension::default();
                    for i in 0..4 {
                        d = d
                            .product(
                                &base(i).power((random(&mut seed) % 21) as i64 - 10).unwrap(),
                                false,
                            )
                            .unwrap();
                    }
                    d
                })
                .collect();
            let (a, b, c) = (&dimensions[0], &dimensions[1], &dimensions[2]);
            assert_eq!(a.product(b, false), b.product(a, false));
            assert_eq!(
                a.product(b, false).unwrap().product(c, false),
                a.product(&b.product(c, false).unwrap(), false)
            );
            assert_eq!(a.product(b, false).unwrap().product(b, true).unwrap(), *a);
            assert_eq!(a.product(a, true).unwrap(), Dimension::default());
        }
    }
}
