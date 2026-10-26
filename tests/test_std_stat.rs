//! Statistical contracts, independently tabulated distributions and RNG draws.
mod support;

fn native(body: &str, expected: &str) {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    let source = format!(
        "import std.stat as stat\nimport std.random as random\nimport std.math as math\n\ndef close(a: f64, b: f64) -> bool:\n    (a-b).fabs() <= 2e-13 * math.max(1e-300, b.fabs())\n\ndef main() -> Result[(), AllocError | IoError | stat.StatError]:\n{body}    Ok(())\n"
    );
    let actual = support::run_file_stdout(&source);
    let actual = actual
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(actual, expected.trim_matches('\n'), "{source}");
}

#[test]
fn descriptive_statistics_and_quantile_conventions() {
    native(
        r#"
data = [4.0, 1.0, 3.0, 2.0]?
print(stat.mean(&data)?, stat.median(&data)?, stat.quantiles(&data)?)?
print(close(stat.pvariance(&data)?, 1.25), close(stat.variance(&data)?, 1.6666666666666667))?
print(close(stat.pstdev(&data)?, 1.118033988749895), close(stat.stdev(&data)?, 1.2909944487358056))?
print(stat.quantile(&data, 0.0)?, stat.quantile(&data, 1.0)?, data)?
singleton = [7.0]?
print(stat.quantiles(&singleton)?, stat.quantiles(&data, 1)?, stat.pvariance(&singleton)?)?
cancel = [1e16, 1.0, -1e16]?
print(close(stat.mean(&cancel)?, 1.0 / 3.0))?
large = [1e308, 1e308]?
wide = [-1e308, 1e308]?
offset = [1e16, 10000000000000002.0]?
print(stat.mean(&large)? == 1e308, stat.median(&wide)? == 0.0, stat.pstdev(&wide)? == 1e308, close(stat.pvariance(&offset)?, 1.0))?
print(str.repr(stat.pvariance(&wide))?)?
"#,
        "2.5 2.5 [1.75, 2.5, 3.25]\nTrue True\nTrue True\n1.0 4.0 [4.0, 1.0, 3.0, 2.0]\n[7.0, 7.0, 7.0] [] 0.0\nTrue\nTrue True True True\nResult[f64, std.stat.StatError].Err(std.stat.StatError.Overflow)",
    );
}

#[test]
fn invalid_data_and_parameters_return_errors() {
    native(
        r#"
empty: list[f64] = []?
one = [1.0]?
bad = [f64.NAN]?
print(str.repr(stat.mean(&empty))?, str.repr(stat.variance(&one))?, str.repr(stat.mean(&bad))?)?
print(str.repr(stat.quantile(&one, -0.1))?, str.repr(stat.quantile(&one, f64.NAN))?, str.repr(stat.quantiles(&one, 0))?)?
print(str.repr(stat.normal(0.0, 0.0))?, str.repr(stat.exponential(-1.0))?, str.repr(stat.uniform(2.0, 1.0))?)?
"#,
        "Result[f64, std.stat.StatError].Err(std.stat.StatError.Empty) Result[f64, std.stat.StatError].Err(std.stat.StatError.InsufficientData) Result[f64, std.stat.StatError].Err(std.stat.StatError.NonFinite)\nResult[f64, std.stat.StatError].Err(std.stat.StatError.Domain) Result[f64, std.stat.StatError].Err(std.stat.StatError.Domain) Result[list[f64], std.stat.StatError].Err(std.stat.StatError.Domain)\nResult[std.stat.Normal, std.stat.StatError].Err(std.stat.StatError.Domain) Result[std.stat.Exponential, std.stat.StatError].Err(std.stat.StatError.Domain) Result[std.stat.Uniform, std.stat.StatError].Err(std.stat.StatError.Domain)",
    );
}

#[test]
fn distribution_values_tails_and_inverses() {
    native(
        r#"
n = stat.normal()?
print(close(n.pdf(0.0)?, 0.3989422804014327), n.cdf(0.0)? == 0.5, close(n.cdf(-1.0)?, 0.15865525393145705))?
print(close(n.cdf(-8.0)?, 6.220960574271784e-16), close(n.cdf(-20.0)?, 2.7536241186063314e-89), close(n.inverse_cdf(0.975)?, 1.959963984540054))?
print(n.cdf(-f64.INF)?, n.cdf(f64.INF)?, n.inverse_cdf(0.0)?, n.inverse_cdf(1.0)?)?
for p in [1e-100, 1e-20, 0.01, 0.2, 0.5, 0.8, 0.99]?:
    print(close(n.cdf(n.inverse_cdf(p)?)?, p))?
u = stat.uniform(2.0, 6.0)?
e = stat.exponential(2.0)?
print(u.pdf(3.0)?, u.cdf(3.0)?, u.inverse_cdf(0.25)?, u.pdf(7.0)?)?
print(close(e.pdf(1.0)?, 0.2706705664732254), close(e.cdf(1.0)?, 0.8646647167633873), close(e.inverse_cdf(0.5)?, 0.34657359027997265))?
print(e.cdf(-1.0)?, e.cdf(f64.INF)?, e.inverse_cdf(1.0)?)?
huge = stat.normal(-1e308, 1e308)?
print(close(huge.cdf(1e308)?, 0.9772498680518208) and n.inverse_cdf(0.5)? == 0.0)?
"#,
        "True True True\nTrue True True\n0.0 1.0 -inf inf\nTrue\nTrue\nTrue\nTrue\nTrue\nTrue\nTrue\n0.25 0.25 3.0 0.0\nTrue True True\n0.0 1.0 inf\nTrue",
    );
}

#[test]
fn sampling_has_fixed_draw_counts_and_atomic_overflow() {
    native(
        r#"
mut actual = random.seeded(42u64)
mut oracle = random.seeded(42u64)
u = stat.uniform()?
n = stat.normal()?
e = stat.exponential()?
print(u.sample(&mut actual) == oracle.next_f64())?
first = oracle.next_f64()
second = oracle.next_f64()
print(close(n.sample(&mut actual)?, (-2.0 * (-first).log1p()).sqrt() * (f64.TAU * second).cos()))?
print(close(e.sample(&mut actual)?, -(-oracle.next_f64()).log1p()), actual.state() == oracle.state())?
tiny = stat.exponential(5e-324)?
saved = actual.state()
match tiny.sample(&mut actual):
    case Err(error):
        print(error, actual.state() == saved)?
    case Ok(_):
        print("unexpected success")?
"#,
        "True\nTrue\nTrue True\nstd.stat.StatError.Overflow True",
    );
}

#[test]
fn normal_cdf_matches_independent_erfc_table_across_algorithm_boundary() {
    // Values generated independently with Python math.erfc(z / sqrt(2))/2.
    native(
        r#"
positions = [0.001, 0.1, 0.5, 0.9, 0.999, 1.0, 1.001, 1.1, 1.5, 2.0, 3.0, 4.0, 6.0, 10.0, 15.0, 25.0, 30.0, 35.0, 37.0]?
expected = [0.49960105778608893, 0.460172162722971, 0.30853753872598694, 0.1840601253467595, 0.15889734564131833, 0.15865525393145707, 0.15841340419228006, 0.13566606094638267, 0.06680720126885809, 0.022750131948179216, 0.0013498980316300957, 3.167124183311997e-5, 9.865876450377014e-10, 7.619853024160595e-24, 3.6709661993128594e-51, 3.056696706382875e-138, 4.906713927148764e-198, 1.1249107064725532e-268, 5.725571222525139e-300]?
n = stat.normal()?
mut correct = True
for index in range(len(positions)):
    correct = correct and close(n.cdf(-positions[index])?, expected[index])
print(correct)?
"#,
        "True",
    );
}

#[test]
fn seeded_sampling_has_expected_empirical_moments() {
    native(
        r#"
mut rng = random.seeded(12345u64)
n = stat.normal()?
e = stat.exponential()?
mut normal_sum = 0.0
mut normal_squares = 0.0
mut exponential_sum = 0.0
for index in range(20000):
    x = n.sample(&mut rng)?
    normal_sum += x
    normal_squares += x*x
    exponential_sum += e.sample(&mut rng)?
print((normal_sum / 20000.0).fabs() < 0.05, (normal_squares / 20000.0 - 1.0).fabs() < 0.06, (exponential_sum / 20000.0 - 1.0).fabs() < 0.05)?
"#,
        "True True True",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn scalar_statistics_allocate_nothing_and_sorting_is_fallible() {
    native(
        r#"
data = [1.0, 2.0, 3.0]?
mut rng = random.seeded(7u64)
n = stat.normal()?
print("__test_fail_allocations_after_0__")?
print("__test_begin_no_allocations__")?
average = stat.mean(&data)?
spread = stat.stdev(&data)?
sample = n.sample(&mut rng)?
middle = stat.median(&data)
print("__test_restore_allocations__")?
print("__test_end_no_allocations__")?
print(average, spread, math.isfinite(sample))?
match middle:
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected success")?
"#,
        "2.0 1.0 True\nstd.stat.StatError.Allocation(AllocError.OutOfMemory)",
    );
}
