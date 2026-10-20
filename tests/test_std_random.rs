//! `std.random`: caller-owned seeded generators, checked against published
//! vectors and an independent reference implementation.
mod support;
use support::check_file_error as rejected;

const HEADER: &str =
    "import std.bytes\nimport std.random\nfrom std.bytes import Bytes\nfrom std.random import Rng, RandomError\n";

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!("{HEADER}def main() -> Result[(), AllocError | IoError | std.random.RandomError | std.bytes.BytesError]:\n{body}    Ok(())\n")
}

fn output(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("std.random.RandomError", "E")
        .replace("std.random.Rng", "Rng")
}

fn native(body: &str, expected: &str) {
    assert_eq!(
        output(&program(body)),
        expected.trim_matches('\n'),
        "{body}"
    );
}

/// Independent xoshiro256** 1.0 with SplitMix64 seeding, written from the
/// reference C sources rather than from `random.plenty`.
struct Reference([u64; 4]);

impl Reference {
    fn seeded(seed: u64) -> Self {
        let mut state = seed;
        let mut words = [0u64; 4];
        for word in &mut words {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            *word = z ^ (z >> 31);
        }
        Self(words)
    }

    fn next(&mut self) -> u64 {
        let s = &mut self.0;
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    fn below(&mut self, bound: u64) -> u64 {
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let value = self.next();
            if value >= threshold {
                return value % bound;
            }
        }
    }

    fn between(&mut self, low: i64, high: i64) -> i64 {
        let span = high.wrapping_sub(low) as u64;
        (low as u64).wrapping_add(self.below(span)) as i64
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for last in (1..items.len()).rev() {
            let pick = self.below(last as u64 + 1) as usize;
            items.swap(last, pick);
        }
    }

    fn sample(&mut self, count: usize, amount: usize) -> Vec<usize> {
        let mut pool: Vec<usize> = (0..count).collect();
        for position in 0..amount {
            let pick = position + self.below((count - position) as u64) as usize;
            pool.swap(position, pick);
        }
        pool.truncate(amount);
        pool
    }

    fn fill(&mut self, target: &mut [u8]) {
        for group in target.chunks_mut(8) {
            let word = self.next().to_le_bytes();
            group.copy_from_slice(&word[..group.len()]);
        }
    }
}

fn list<T: std::fmt::Display>(items: &[T]) -> String {
    let items: Vec<String> = items.iter().map(ToString::to_string).collect();
    format!("[{}]", items.join(", "))
}

#[test]
fn published_vectors_are_reproduced() {
    // SplitMix64 from seed 1234567 and xoshiro256** from state (1, 2, 3, 4),
    // as published with the reference implementations.
    native(
        r#"
seeded = std.random.seeded(1234567u64)
print(seeded.state())?
mut exact = std.random.from_state(1u64, 2u64, 3u64, 4u64)?
for _ in range(10):
    print(exact.next_u64())?
"#,
        "(6457827717110365317, 3203168211198807973, 9817491932198370423, 4593380528125082431)
11520
0
1509978240
1215971899390074240
1216172134540287360
607988272756665600
16172922978634559625
8476171486693032832
10595114339597558777
2904607092377533576",
    );
}

#[test]
fn seeded_streams_match_the_reference() {
    let mut expected = Vec::new();
    for seed in [0u64, 1, 42, u64::MAX] {
        let mut reference = Reference::seeded(seed);
        let words: Vec<u64> = (0..6).map(|_| reference.next()).collect();
        expected.push(list(&words));
        expected.push(format!(
            "{} {} {}",
            (reference.next() >> 32) as u32,
            if reference.next() >> 63 == 1 {
                "True"
            } else {
                "False"
            },
            reference.next() >> 11
        ));
    }
    native(
        r#"
seeds = [0u64, 1u64, 42u64, u64.MAX]?
for seed in seeds:
    mut rng = std.random.seeded(seed)
    mut words = list[u64].new()?
    for _ in range(6):
        words.append(rng.next_u64())?
    print(words)?
    half = rng.next_u32()
    flag = rng.next_bool()
    unit = rng.next_f64()
    if unit < 0.0 or unit >= 1.0:
        print("fraction out of range")?
    print(half, flag, u64(unit * 9007199254740992.0))?
"#,
        &expected.join("\n"),
    );
}

#[test]
fn same_seed_same_sequence_and_state_round_trips() {
    native(
        r#"
mut first = std.random.seeded(7u64)
mut second = std.random.seeded(7u64)
mut other = std.random.seeded(8u64)
mut same = True
mut differs = False
for _ in range(100):
    value = first.next_u64()
    if value != second.next_u64():
        same = False
    if value != other.next_u64():
        differs = True
(a, b, c, d) = first.state()
mut restored = std.random.from_state(a, b, c, d)?
print(same, differs, restored.next_u64() == first.next_u64())?
print(str.repr(std.random.from_state(0u64, 0u64, 0u64, 0u64))?)?
"#,
        "True True True
Result[Rng, E].Err(E.ZeroState)",
    );
}

#[test]
fn bounded_draws_are_unbiased_reference_values() {
    // 2^63 + 1 rejects almost half of all steps, so the retry loop runs.
    let bounds = [1u64, 2, 6, 10, 1000, (1 << 63) + 1, u64::MAX];
    let mut reference = Reference::seeded(99);
    let mut expected = Vec::new();
    for bound in bounds {
        let draws: Vec<u64> = (0..8).map(|_| reference.below(bound)).collect();
        assert!(draws.iter().all(|value| *value < bound));
        expected.push(list(&draws));
    }
    expected.push("Result[u64, E].Err(E.EmptyRange)".into());
    native(
        r#"
mut rng = std.random.seeded(99u64)
bounds = [1u64, 2u64, 6u64, 10u64, 1000u64, 9223372036854775809u64, u64.MAX]?
for bound in bounds:
    mut draws = list[u64].new()?
    for _ in range(8):
        draws.append(rng.below(bound)?)?
    print(draws)?
print(str.repr(rng.below(0u64))?)?
"#,
        &expected.join("\n"),
    );
}

#[test]
fn ranges_are_half_open_and_reject_empty_bounds() {
    let ranges = [
        (0i64, 1i64),
        (-5, 5),
        (i64::MIN, i64::MAX),
        (i64::MAX - 1, i64::MAX),
        (i64::MIN, i64::MIN + 3),
    ];
    let mut reference = Reference::seeded(5);
    let mut expected = Vec::new();
    for (low, high) in ranges {
        let draws: Vec<i64> = (0..6).map(|_| reference.between(low, high)).collect();
        assert!(draws.iter().all(|value| (low..high).contains(value)));
        expected.push(list(&draws));
    }
    let after = reference.next();
    expected.push(
        "Result[i64, E].Err(E.EmptyRange)
Result[i64, E].Err(E.EmptyRange)
Result[i64, E].Err(E.EmptyRange)
Result[i64, E].Err(E.OutOfRange)"
            .into(),
    );
    expected.push(after.to_string());
    native(
        r#"
mut rng = std.random.seeded(5u64)
lows = [0, -5, i64.MIN, i64.MAX - 1, i64.MIN]?
highs = [1, 5, i64.MAX, i64.MAX, i64.MIN + 3]?
for at in range(len(lows)):
    mut draws = list[i64].new()?
    for _ in range(6):
        draws.append(rng.between(lows[at], highs[at])?)?
    print(draws)?
print(str.repr(rng.between(3, 3))?)?
print(str.repr(rng.between(4, 3))?)?
print(str.repr(rng.index(0))?)?
print(str.repr(rng.index(-1))?)?
# Rejected requests consume no steps.
print(rng.next_u64())?
"#,
        &expected.join("\n"),
    );
}

#[test]
fn index_draws_cover_every_position() {
    native(
        r#"
mut rng = std.random.seeded(3u64)
mut seen = [0, 0, 0, 0, 0]?
for _ in range(2000):
    at = rng.index(5)?
    seen[at] = seen[at] + 1
mut total = 0
mut balanced = True
for count in &seen:
    total += *count
    if *count < 300 or *count > 500:
        balanced = False
print(total, balanced)?
"#,
        "2000 True",
    );
}

#[test]
fn shuffle_is_a_reference_permutation_of_every_element() {
    let mut reference = Reference::seeded(2024);
    let mut numbers: Vec<i64> = (0..20).collect();
    reference.shuffle(&mut numbers);
    let mut words = vec!["alpha", "beta", "gamma", "delta", "epsilon"];
    reference.shuffle(&mut words);
    let quoted: Vec<String> = words.iter().map(|word| format!("\"{word}\"")).collect();
    let after = reference.next();
    let expected = format!(
        "{}\nTrue\n{}\n[] [7]\n{after}",
        list(&numbers),
        list(&quoted)
    );
    native(
        r#"
mut rng = std.random.seeded(2024u64)
mut numbers = list[i64].new()?
for value in range(20):
    numbers.append(value)?
rng.shuffle(&mut numbers)
print(numbers)?
mut ordered = copy(numbers)?
ordered.sort()
mut intact = True
for value in range(20):
    if ordered[value] != value:
        intact = False
print(intact)?
# Owned strings are exchanged in place, never copied or dropped.
mut words = ["alpha", "beta", "gamma", "delta", "epsilon"]?
rng.shuffle(&mut words)
print(words)?
mut empty = list[i64].new()?
mut single = [7]?
rng.shuffle(&mut empty)
rng.shuffle(&mut single)
print(empty, single)?
# Lists shorter than two elements consume no steps.
print(rng.next_u64())?
"#,
        &expected,
    );
}

#[test]
fn sampled_indices_are_distinct_and_bounded() {
    let mut reference = Reference::seeded(11);
    let picked = reference.sample(10, 4);
    let all = reference.sample(6, 6);
    let mut sorted = all.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, (0..6).collect::<Vec<_>>());
    let after = reference.next();
    let failure = "Result[list[i64], E].Err(E.OutOfRange)";
    let expected = format!(
        "{}\n{}\n[] []\n{failure}\n{failure}\n{failure}\n{after}",
        list(&picked),
        list(&all)
    );
    native(
        r#"
mut rng = std.random.seeded(11u64)
print(rng.sample_indices(10, 4)?)?
print(rng.sample_indices(6, 6)?)?
print(rng.sample_indices(5, 0)?, rng.sample_indices(0, 0)?)?
print(str.repr(rng.sample_indices(3, 4))?)?
print(str.repr(rng.sample_indices(-1, 0))?)?
print(str.repr(rng.sample_indices(3, -1))?)?
print(rng.next_u64())?
"#,
        &expected,
    );
}

#[test]
fn fill_writes_little_endian_words_inside_the_range_only() {
    let mut reference = Reference::seeded(77);
    let mut whole = [0u8; 16];
    reference.fill(&mut whole);
    let mut ragged = [9u8; 16];
    reference.fill(&mut ragged[3..14]);
    let after = reference.next();
    let failure = "Result[(), E].Err(E.OutOfRange)";
    let expected = format!(
        "{}\n{}\n{failure}\n{failure}\n{failure}\n{after}",
        list(&whole),
        list(&ragged)
    );
    native(
        r#"
mut rng = std.random.seeded(77u64)
mut whole = std.bytes.empty()?
whole.resize(16, 0u8)?
rng.fill(&mut whole, 0, 16)?
print(whole.to_list()?)?
mut ragged = std.bytes.empty()?
ragged.resize(16, 9u8)?
rng.fill(&mut ragged, 3, 14)?
rng.fill(&mut ragged, 5, 5)?
print(ragged.to_list()?)?
print(str.repr(rng.fill(&mut ragged, -1, 4))?)?
print(str.repr(rng.fill(&mut ragged, 5, 4))?)?
print(str.repr(rng.fill(&mut ragged, 0, 17))?)?
print(rng.next_u64())?
"#,
        &expected,
    );
}

#[test]
fn generators_move_into_workers_and_stay_deterministic() {
    let mut reference = Reference::seeded(13);
    let expected = format!("{} {}", reference.next(), reference.between(0, 100));
    let source = format!(
        "{HEADER}{}",
        r#"
def work(rng: Rng) -> ():
    mut owned = rng
    first = owned.next_u64()
    print(first, owned.between(0, 100).unwrap()).unwrap()

def main() -> Result[(), AllocError | IoError | ThreadError]:
    rng = std.random.seeded(13u64)
    with spawn(work, rng)?:
        pass
    Ok(())
"#
    );
    assert_eq!(output(&source), expected);
}

#[test]
fn a_generator_cannot_be_built_or_advanced_without_ownership() {
    let forged = rejected(&program("rng = Rng(1u64, 2u64, 3u64, 4u64)\n"));
    assert!(forged.contains("private"), "{forged}");
    let shared = rejected(&program(
        "rng = std.random.seeded(1u64)\nprint(rng.next_u64())?\n",
    ));
    assert!(shared.contains("mut"), "{shared}");
    let ambient = rejected(&program("print(std.random.next_u64())?\n"));
    assert!(ambient.contains("next_u64"), "{ambient}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn drawing_shuffling_and_filling_do_not_allocate() {
    native(
        r#"
mut rng = std.random.seeded(1u64)
mut items = [1, 2, 3, 4, 5, 6, 7, 8]?
mut buffer = std.bytes.empty()?
buffer.resize(32, 0u8)?
print("__test_fail_allocations_after_0__")?
print("__test_begin_no_allocations__")?
mut other = std.random.seeded(2u64)
a = other.next_u64()
b = rng.below(10u64)?
c = rng.between(-3, 3)?
d = rng.next_f64()
rng.shuffle(&mut items)
rng.fill(&mut buffer, 0, 32)?
print("__test_restore_allocations__")?
print("__test_end_no_allocations__")?
print(a > 0u64, b < 10u64, c < 3, d < 1.0, len(items))?
"#,
        "True True True True 8",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_sample_allocation_consumes_no_steps() {
    native(
        r#"
mut rng = std.random.seeded(21u64)
mut twin = std.random.seeded(21u64)
print("__test_fail_allocations_after_0__")?
failed = rng.sample_indices(1000, 10)
print("__test_restore_allocations__")?
print(str.repr(failed)?, rng.next_u64() == twin.next_u64())?
"#,
        "Result[list[i64], E].Err(E.Allocation(AllocError.OutOfMemory)) True",
    );
}
