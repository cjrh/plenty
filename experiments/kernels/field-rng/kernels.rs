// The kernels of kernels.c in Rust, for the comparison in README.md.
// Build: rustc -C opt-level=3 -C target-cpu=x86-64 kernels.rs
use std::hint::black_box;

const IM: i64 = 139968;
const IA: i64 = 3877;
const IC: i64 = 29573;

struct Rng {
    seed: i64,
}

#[inline(never)]
fn fill(rng: &mut Rng, out: &mut [i64]) {
    for slot in out.iter_mut() {
        rng.seed = (rng.seed * IA + IC) % IM;
        *slot = rng.seed;
    }
}

// `fill` with the range stated by an unsigned field.
struct Urng {
    seed: u64,
}

#[inline(never)]
fn fill_bounded(rng: &mut Urng, out: &mut [u64]) {
    for slot in out.iter_mut() {
        rng.seed = (rng.seed * IA as u64 + IC as u64) % IM as u64;
        *slot = rng.seed;
    }
}

#[inline(never)]
fn fill_local(seed: i64, out: &mut [i64]) -> i64 {
    let mut s = seed % IM;
    for slot in out.iter_mut() {
        s = (s * IA + IC) % IM;
        *slot = s;
    }
    s
}

#[inline(never)]
fn fill_arms(seed: i64, out: &mut [i64]) -> i64 {
    let mut s = seed % IM;
    for (i, slot) in out.iter_mut().enumerate() {
        if i % 2 == 0 {
            s = (s * IA + IC) % IM;
        } else {
            s = (s * 3 + 1) % 1000;
        }
        *slot = s;
    }
    s
}

#[inline(never)]
fn fill_inner(seed: i64, out: &mut [i64], rounds: i64) -> i64 {
    let mut s = seed % IM;
    for _ in 0..rounds {
        for slot in out.iter_mut() {
            s = (s * IA + IC) % IM;
            *slot = s;
        }
    }
    s
}

#[inline(never)]
fn fill_unbounded(seed: i64, out: &mut [i64]) -> i64 {
    let mut s = seed % IM;
    for (i, slot) in out.iter_mut().enumerate() {
        s = (s * IA + IC) % IM;
        if s == 7 {
            s += i as i64;
        }
        *slot = s;
    }
    s
}

#[inline(never)]
fn fill_product(seed: i64, out: &mut [i64]) -> i64 {
    let mut s = seed % IM;
    for slot in out.iter_mut() {
        s = (s * IA) % IM;
        *slot = s;
    }
    s
}

// `kernels KERNEL ROUNDS`: run one kernel ROUNDS times over 4096 elements.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        std::process::exit(2);
    }
    let kernel = args[1].as_str();
    let rounds: i64 = args[2].parse().unwrap();
    let mut out = vec![0i64; 4096];
    let mut last = 42i64;
    match kernel {
        "fill" => {
            let mut rng = Rng { seed: 42 };
            for _ in 0..rounds {
                fill(&mut rng, black_box(&mut out));
            }
            last = rng.seed;
        }
        "fill_bounded" => {
            let mut rng = Urng { seed: 42 };
            let mut out = vec![0u64; 4096];
            for _ in 0..rounds {
                fill_bounded(&mut rng, black_box(&mut out));
            }
            last = rng.seed as i64;
        }
        "fill_local" => {
            for _ in 0..rounds {
                last = fill_local(last, black_box(&mut out));
            }
        }
        "fill_arms" => {
            for _ in 0..rounds {
                last = fill_arms(last, black_box(&mut out));
            }
        }
        "fill_inner" => last = fill_inner(last, black_box(&mut out), rounds),
        "fill_unbounded" => {
            for _ in 0..rounds {
                last = fill_unbounded(last, black_box(&mut out));
            }
        }
        "fill_product" => {
            for _ in 0..rounds {
                last = fill_product(last, black_box(&mut out));
            }
        }
        _ => {}
    }
    println!("{kernel} {last}");
}
