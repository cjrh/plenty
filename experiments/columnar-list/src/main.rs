//! Native layout experiment, not Plenty's representation or a clist implementation.
use std::hint::black_box;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Row([u64; 8]);

fn input(i: usize) -> Row {
    let x = (i as u64).wrapping_mul(6364136223846793005).wrapping_add(1);
    Row(std::array::from_fn(|f| x.rotate_left(f as u32 * 7)))
}

fn mix(row: Row) -> u64 {
    row.0.into_iter().fold(0, |a, b| (a ^ b).rotate_left(7))
}

trait Layout {
    fn len(&self) -> usize;
    fn row(&self, i: usize) -> Row;
    fn narrow(&self) -> u64;
    fn filter(&self) -> u64;
    fn all(&self) -> u64;

    #[inline(never)]
    fn random_all(&self) -> u64 {
        // An odd multiplier permutes power-of-two lengths without an index array.
        assert!(self.len().is_power_of_two());
        (0..self.len()).fold(0u64, |s, i| {
            s.wrapping_add(mix(self.row(i.wrapping_mul(15485863) & (self.len() - 1))))
        })
    }
}

struct Aos(Vec<Row>);
impl Layout for Aos {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn row(&self, i: usize) -> Row {
        self.0[i]
    }
    #[inline(never)]
    fn narrow(&self) -> u64 {
        self.0.iter().fold(0u64, |s, r| s.wrapping_add(r.0[0]))
    }
    #[inline(never)]
    fn filter(&self) -> u64 {
        self.0
            .iter()
            .filter(|r| r.0[0] & 7 == 0)
            .fold(0u64, |s, r| s.wrapping_add(r.0[1]))
    }
    #[inline(never)]
    fn all(&self) -> u64 {
        self.0.iter().fold(0u64, |s, r| s.wrapping_add(mix(*r)))
    }
}

struct Soa {
    columns: [Vec<u64>; 8],
}
impl Soa {
    fn from_rows(rows: &[Row]) -> Self {
        Self {
            columns: std::array::from_fn(|f| rows.iter().map(|r| r.0[f]).collect()),
        }
    }
}
impl Layout for Soa {
    fn len(&self) -> usize {
        self.columns[0].len()
    }
    fn row(&self, i: usize) -> Row {
        Row(std::array::from_fn(|f| self.columns[f][i]))
    }
    #[inline(never)]
    fn narrow(&self) -> u64 {
        self.columns[0].iter().fold(0u64, |s, x| s.wrapping_add(*x))
    }
    #[inline(never)]
    fn filter(&self) -> u64 {
        self.columns[0]
            .iter()
            .zip(&self.columns[1])
            .filter(|(x, _)| **x & 7 == 0)
            .fold(0u64, |s, (_, y)| s.wrapping_add(*y))
    }
    #[inline(never)]
    fn all(&self) -> u64 {
        (0..self.len()).fold(0u64, |s, i| s.wrapping_add(mix(self.row(i))))
    }
}

struct Chunked {
    chunks: Vec<Soa>,
    width: usize,
    len: usize,
}
impl Chunked {
    fn new(rows: &[Row], width: usize) -> Self {
        Self {
            chunks: rows.chunks(width).map(Soa::from_rows).collect(),
            width,
            len: rows.len(),
        }
    }
}
impl Layout for Chunked {
    fn len(&self) -> usize {
        self.len
    }
    fn row(&self, i: usize) -> Row {
        self.chunks[i / self.width].row(i % self.width)
    }
    #[inline(never)]
    fn narrow(&self) -> u64 {
        self.chunks
            .iter()
            .fold(0u64, |s, c| s.wrapping_add(c.narrow()))
    }
    #[inline(never)]
    fn filter(&self) -> u64 {
        self.chunks
            .iter()
            .fold(0u64, |s, c| s.wrapping_add(c.filter()))
    }
    #[inline(never)]
    fn all(&self) -> u64 {
        self.chunks
            .iter()
            .fold(0u64, |s, c| s.wrapping_add(c.all()))
    }
}

fn evaluate(l: &impl Layout, kind: usize) -> u64 {
    match kind {
        0 => l.narrow(),
        1 => l.filter(),
        2 => l.all(),
        _ => l.random_all(),
    }
}

fn sample(l: &impl Layout, kind: usize, expected: u64, repeats: usize) -> f64 {
    assert_eq!(evaluate(l, kind), expected);
    let start = Instant::now();
    for _ in 0..repeats {
        assert_eq!(black_box(evaluate(black_box(l), kind)), expected);
    }
    start.elapsed().as_secs_f64() * 1e9 / (l.len() * repeats) as f64
}

fn benchmark() {
    println!("rows,kernel,layout,sample,ns_per_row");
    for n in [512, 32768, 1048576] {
        let aos = Aos((0..n).map(input).collect());
        let soa = Soa::from_rows(&aos.0);
        let c1024 = Chunked::new(&aos.0, 1024);
        let c4096 = Chunked::new(&aos.0, 4096);
        for (kind, kernel) in ["sum_one", "filter_two", "all_sequential", "all_permuted"]
            .iter()
            .enumerate()
        {
            let expected = evaluate(&aos, kind);
            let repeats = (4_194_304 / n).max(1);
            for trial in 0..7 {
                // Rotate order to avoid systematically favoring one layout.
                for offset in 0..4 {
                    let layout = (trial + offset) % 4;
                    let (name, time) = match layout {
                        0 => ("aos", sample(&aos, kind, expected, repeats)),
                        1 => ("soa", sample(&soa, kind, expected, repeats)),
                        2 => ("chunk1024", sample(&c1024, kind, expected, repeats)),
                        _ => ("chunk4096", sample(&c4096, kind, expected, repeats)),
                    };
                    println!("{n},{kernel},{name},{trial},{time:.6}");
                }
            }
        }
    }
}

fn main() {
    benchmark();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn layout_equivalence_including_chunk_tails() {
        for n in [0, 1, 2, 7, 1023, 1024, 1025, 4097] {
            let aos = Aos((0..n).map(input).collect());
            let soa = Soa::from_rows(&aos.0);
            let chunk = Chunked::new(&aos.0, 1024);
            for i in 0..n {
                assert_eq!(aos.row(i), soa.row(i));
                assert_eq!(aos.row(i), chunk.row(i));
            }
            for kind in 0..3 {
                assert_eq!(evaluate(&aos, kind), evaluate(&soa, kind));
                assert_eq!(evaluate(&aos, kind), evaluate(&chunk, kind));
            }
        }
    }

    // Models a transaction boundary, not an allocator implementation. Real reserve
    // failures and an injected failure before each required reserve take one path.
    fn reserve<T>(v: &mut Vec<T>, fail_at: usize, attempts: &mut usize) -> Result<(), ()> {
        if v.len() == v.capacity() {
            let attempt = *attempts;
            *attempts += 1;
            if attempt == fail_at {
                return Err(());
            }
            v.try_reserve(1).map_err(|_| ())?;
        }
        Ok(())
    }

    struct Columns<A, B> {
        a: Vec<Option<A>>,
        b: Vec<Option<B>>,
    }
    impl<A, B> Columns<A, B> {
        fn new() -> Self {
            Self {
                a: Vec::new(),
                b: Vec::new(),
            }
        }
        fn append(&mut self, row: (A, B), fail_at: usize) -> Result<(), (A, B)> {
            let mut attempts = 0;
            if reserve(&mut self.a, fail_at, &mut attempts).is_err()
                || reserve(&mut self.b, fail_at, &mut attempts).is_err()
            {
                return Err(row);
            }
            self.a.push(Some(row.0));
            self.b.push(Some(row.1));
            Ok(())
        }
        fn delete(&mut self, i: usize) {
            drop(self.a[i].take());
            drop(self.b[i].take());
        }
        fn vacuum(&mut self) {
            let mut write = 0;
            for read in 0..self.a.len() {
                assert_eq!(self.a[read].is_some(), self.b[read].is_some());
                if self.a[read].is_some() {
                    if read != write {
                        self.a[write] = self.a[read].take();
                        self.b[write] = self.b[read].take();
                    }
                    write += 1;
                }
            }
            self.a.truncate(write);
            self.b.truncate(write);
        }
    }
    #[derive(Debug)]
    struct Ticket(Rc<Cell<usize>>);
    impl Drop for Ticket {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn append_failure_returns_both_owners_without_publishing_a_row() {
        for fail_at in 0..=2 {
            let drops = Rc::new(Cell::new(0));
            let mut c = Columns::new();
            let result = c.append((Ticket(drops.clone()), Ticket(drops.clone())), fail_at);
            assert_eq!(c.a.len(), usize::from(fail_at == 2));
            assert_eq!(c.a.len(), c.b.len());
            assert_eq!(result.is_ok(), fail_at == 2);
            assert_eq!(drops.get(), 0);
            drop(result);
            drop(c);
            assert_eq!(drops.get(), 2);
        }
    }

    #[test]
    fn tombstones_drop_now_and_vacuum_preserves_order_without_double_drop() {
        for mask in 0..256u16 {
            let drops = Rc::new(Cell::new(0));
            let mut c = Columns::new();
            for i in 0..8 {
                c.append((i, Ticket(drops.clone())), usize::MAX).unwrap();
            }
            for i in 0..8 {
                if mask & (1 << i) != 0 {
                    c.delete(i);
                    c.delete(i);
                }
            }
            assert_eq!(drops.get(), mask.count_ones() as usize);
            c.vacuum();
            let expected: Vec<_> = (0..8).filter(|i| mask & (1 << i) == 0).collect();
            assert_eq!(c.a.iter().map(|v| v.unwrap()).collect::<Vec<_>>(), expected);
            c.vacuum();
            drop(c);
            assert_eq!(drops.get(), 8);
        }
    }

    #[test]
    fn conservative_min_max_pruning_depends_on_ordering() {
        let sorted: Vec<u64> = (0..8192).collect();
        let shuffled: Vec<u64> = (0..8192).map(|i| (i * 15485863) & 8191).collect();
        for data in [&sorted, &shuffled] {
            let threshold = 64;
            let mut visited = 0;
            let mut sum = 0;
            for chunk in data.chunks(1024) {
                if *chunk.iter().min().unwrap() >= threshold {
                    continue;
                }
                visited += 1;
                sum += chunk.iter().filter(|x| **x < threshold).sum::<u64>();
            }
            assert_eq!(sum, (0..threshold).sum());
            assert_eq!(visited, if std::ptr::eq(data, &sorted) { 1 } else { 8 });
        }
    }
}
