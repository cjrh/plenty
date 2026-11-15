use malachite_base::num::arithmetic::traits::DivMod;
use malachite_base::num::conversion::traits::{FromStringBase, RoundingFrom, ToStringBase};
use malachite_base::num::logic::traits::SignificantBits;
use malachite_base::rounding_modes::RoundingMode::Nearest;
use malachite_nz::integer::Integer;
use std::hint::black_box;

fn parse(text: &str) -> Integer {
    Integer::from_string_base(16, text).unwrap()
}

fn emit(value: &Integer) {
    println!("{}", value.to_string_base(16));
}

fn oracle(path: &str) {
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let fields: Vec<_> = line.split('\t').collect();
        let a = parse(fields[0]);
        let b = parse(fields[1]);
        let shift: u64 = fields[2].parse().unwrap();
        emit(&a);
        emit(&(&a + &b));
        emit(&(&a - &b));
        emit(&(&a * &b));
        if b != 0 {
            let (q, r) = (&a).div_mod(&b); // Floor, unlike Rust's / and %.
            emit(&q);
            emit(&r);
        }
        emit(&(&a & &b));
        emit(&(&a | &b));
        emit(&(&a ^ &b));
        emit(&(&a << shift));
        emit(&(&a >> shift));
        println!("float {}", f64::rounding_from(&a, Nearest).0);
        println!(
            "{}",
            i64::try_from(&a).map_or("range".into(), |n| n.to_string())
        );
        println!(
            "{}",
            u64::try_from(&a).map_or("range".into(), |n| n.to_string())
        );
        for radix in 2..=36 {
            let text = a.to_string_base(radix);
            println!("{text}");
            emit(&Integer::from_string_base(radix, &text).unwrap());
        }
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args[1] == "oracle" {
        oracle(&args[2]);
        return;
    }
    let op = args[1].as_str();
    let a = parse(&args[2]);
    let b = parse(&args[3]);
    let count: u64 = args[4].parse().unwrap();
    let text = a.to_string_base(10);
    let mut checksum = 0u64;
    let start = std::time::Instant::now();
    macro_rules! batch {
        ($value:expr) => {
            for _ in 0..count {
                checksum += $value;
            }
        };
    }
    match op {
        "add" => batch!(black_box(black_box(&a) + black_box(&b)).significant_bits()),
        "mul" => batch!(black_box(black_box(&a) * black_box(&b)).significant_bits()),
        "div" => batch!(black_box(black_box(&a).div_mod(black_box(&b)).0).significant_bits()),
        "parse" => batch!(
            black_box(Integer::from_string_base(10, black_box(&text)).unwrap()).significant_bits()
        ),
        "render" => batch!(black_box(black_box(&a).to_string_base(10)).len() as u64),
        _ => panic!("unknown operation"),
    }
    println!("{} {checksum}", start.elapsed().as_nanos());
}
