//! Pure-Plenty plotting: grammar contracts, deterministic SVG and failures.
mod support;

fn program(body: &str) -> String {
    format!(
        "import std.plot as gg\nimport std.io\nimport std.bytes\nfrom std.plot import Column, PlotError\nfrom std.io import StreamError\nfrom std.bytes import BytesError\ndef main() -> Result[(), AllocError | IoError | PlotError | BytesError]:\n{}    Ok(())\n",
        body.lines()
            .map(|line| format!("    {line}\n"))
            .collect::<String>()
    )
}

fn render(body: &str) -> String {
    support::run_file_stdout(&program(&format!(
        "{body}\nmut output = std.io.cursor(std.bytes.empty()?)\nnotes = p.write_svg(&mut output)?\nprint(output.data.to_text()?)?"
    )))
}

fn snapshot(name: &str, actual: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/plot")
        .join(format!("{name}.svg"));
    if std::env::var_os("UPDATE_PLOT_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("{}\n", actual.trim_end())).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(path).unwrap().trim_end(),
        actual.trim_end()
    );
}

#[test]
fn scatter_line_and_escaping_snapshot() {
    let svg = render(
        r##"
data = gg.table([("x", Column.Num([3.0, 1.0, 2.0]?)), ("y", Column.Num([4.0, 2.0, 1.0]?))]?)?
p = gg.ggplot(data, gg.aes("x", "y"), [gg.geom_line(color="#123456"), gg.geom_point(size=3), gg.labs(title="A < B & \"quoted\"", x="Distance", y="Value"), gg.theme_minimal()]?)?
"##,
    );
    assert_eq!(svg.matches("<circle ").count(), 3);
    assert!(svg.contains("A &lt; B &amp; &quot;quoted&quot;"));
    snapshot("scatter-line", &svg);
}

#[test]
fn manual_palettes_follow_names_and_keep_independent_legends() {
    let svg = render(
        r##"
data = gg.table([("x", Column.Text(["B", "A"]?)), ("y", Column.Num([2.0, 3.0]?))]?)?
p = gg.ggplot(data, gg.aes("x", "y", colour="x", fill="x"), [gg.scale_color_manual({"B": "#112233", "A": "#445566"}?), gg.geom_col(position=gg.position_identity()), gg.scale_fill_manual({"A": "#abcdef", "B": "#fedcba"}?)]?)?
"##,
    );
    assert!(svg.contains("fill=\"#fedcba\" stroke=\"#112233\""));
    assert!(svg.contains("fill=\"#abcdef\" stroke=\"#445566\""));
    // Separate scales on the same column must both appear in the guides.
    for colour in ["#112233", "#445566", "#abcdef", "#fedcba"] {
        assert!(svg.contains(&format!("fill=\"{colour}\" stroke=\"none\"")));
    }
    snapshot("manual-palettes", &svg);
}

#[test]
fn gradients_map_endpoints_midpoint_and_constant_values() {
    let svg = render(
        r##"
data = gg.table([("x", Column.Num([0.0, 1.0, 2.0]?)), ("y", Column.Num([1.0, 1.0, 1.0]?))]?)?
p = gg.ggplot(data, gg.aes("x", "y", colour="x"), [gg.geom_point(), gg.scale_color_gradient(low="#000000", high="#FE8040")]?)?
"##,
    );
    let circles: Vec<_> = svg
        .lines()
        .filter(|line| line.contains("<circle "))
        .collect();
    for (circle, colour) in circles.iter().zip(["#000000", "#7f4020", "#fe8040"]) {
        assert!(circle.contains(colour), "{circle}");
        assert!(svg.contains(&format!("fill=\"{colour}\" stroke=\"none\"")));
    }
    assert_eq!(circles.len(), 3);
    snapshot("custom-gradient", &svg);
    let constant = render(
        r##"
data = gg.table([("x", Column.Num([0.0]?)), ("y", Column.Num([1.0]?))]?)?
p = gg.ggplot(data, gg.aes("x", "y", fill="y"), [gg.geom_col(), gg.scale_fill_gradient(low="#000000", high="#FE8040")]?)?
"##,
    );
    assert_eq!(constant.matches("fill=\"#7f4020\"").count(), 2);
}

#[test]
fn scale_replacement_is_immutable_and_constants_override_mappings() {
    let output = support::run_file_stdout(&program(
        r##"
data = gg.table([("x", Column.Text(["A"]?)), ("y", Column.Num([1.0]?))]?)?
base = gg.ggplot(data, gg.aes("x", "y", colour="x"), [gg.geom_point(), gg.scale_colour_manual({"A": "red"}?)]?)?
changed = base.plus([gg.scale_colour_manual({"A": "blue"}?)]?)?
mut first = std.io.cursor(std.bytes.empty()?)
mut second = std.io.cursor(std.bytes.empty()?)
drop(base.write_svg(&mut first)?)
drop(changed.write_svg(&mut second)?)
print("fill=\"red\"" in first.data.to_text()?, "fill=\"blue\"" in second.data.to_text()?)?
"##,
    ));
    assert_eq!(output.trim(), "True True");
    let svg = render(
        r##"
data = gg.table([("x", Column.Text(["A"]?)), ("y", Column.Num([1.0]?))]?)?
p = gg.ggplot(data, gg.aes("x", "y", colour="x"), [gg.scale_colour_manual({"A": "red"}?), gg.geom_point(colour="navy")]?)?
"##,
    );
    assert!(svg.contains("fill=\"navy\""));
    assert!(!svg.contains("fill=\"red\""));
}

#[test]
fn invalid_scales_fail_at_construction() {
    for (mapping, scale, expected) in [
        (
            "colour=\"x\"",
            "gg.scale_colour_manual({\"B\": \"red\"}?)",
            "InvalidSetting",
        ),
        (
            "colour=\"y\"",
            "gg.scale_colour_manual({\"1\": \"red\"}?)",
            "WrongKind",
        ),
        ("fill=\"x\"", "gg.scale_fill_gradient()", "WrongKind"),
        (
            "colour=\"x\"",
            "gg.scale_colour_manual({\"A\": \"\"}?)",
            "InvalidSetting",
        ),
        (
            "colour=\"y\"",
            "gg.scale_colour_gradient(low=\"#12zz00\")",
            "InvalidSetting",
        ),
        (
            "fill=\"y\"",
            "gg.scale_fill_gradient(high=\"#ffffff80\")",
            "InvalidSetting",
        ),
    ] {
        let source = program(&format!(
            "data = gg.table([(\"x\", Column.Text([\"A\"]?)), (\"y\", Column.Num([1.0]?))]?)?\nmatch gg.ggplot(data, gg.aes(\"x\", \"y\", {mapping}), [gg.geom_point(), {scale}]?):\n    case Err(error):\n        print(error)?\n    case Ok(_):\n        print(\"unexpected success\")?"
        ));
        assert!(support::run_file_stdout(&source).contains(expected));
    }
}

#[test]
fn stacked_signed_bars_and_flipped_counts_snapshots() {
    let data = r#"data = gg.table([("category", Column.Text(["B", "A", "A", "B", "B"]?)), ("value", Column.Num([3.0, 2.0, 4.0, -2.0, -1.0]?)), ("group", Column.Text(["first", "first", "second", "first", "second"]?))]?)?"#;
    let svg = render(&format!(
        "{data}\np = gg.ggplot(data, gg.aes(\"category\", \"value\", fill=\"group\"), [gg.geom_col(), gg.labs(title=\"Signed stacks\")]?)?"
    ));
    snapshot("signed-stacks", &svg);
    let svg = render(&format!(
        "{data}\np = gg.ggplot(data, gg.aes(\"category\", fill=\"group\"), [gg.geom_bar(), gg.coord_flip(), gg.theme_classic()]?)?"
    ));
    snapshot("flipped-counts", &svg);
}

#[test]
fn histogram_endpoint_missing_values_and_zoom() {
    let source = program(
        r#"
data = gg.table([("x", Column.MaybeNum([Some(0.0), Some(1.0), Some(2.0), Some(3.0), Some(4.0), Nothing, Some(f64.NAN), Some(f64.INF)]?))]?)?
p = gg.ggplot(data, gg.aes("x"), [gg.geom_histogram(bins=2), gg.coord_cartesian(xlim=(1.0, 3.0))]?)?
mut output = std.io.cursor(std.bytes.empty()?)
notes = p.write_svg(&mut output)?
print(notes)?
print(output.data.to_text()?)?
"#,
    );
    let output = support::run_file_stdout(&source);
    let (notes, svg) = output.split_once('\n').unwrap();
    assert!(notes.contains("Removed 3 rows"));
    assert!(notes.contains("uses 2 equal-width bins"));
    // Coordinate zoom preserves the counts, including the maximum endpoint.
    snapshot("histogram-zoom", svg);
}

#[test]
fn record_accessors_columns_shared_tables_and_immutable_plus() {
    let source = format!(
        "class Record:\n    x: i64\n    y: f64\n    name: str\n    active: bool\n{}",
        program(
            r#"
records = [Record(10, 2.5, "ten", True), Record(2, 1.5, "two", False)]?
t = gg.rows(&records, [gg.integer("x", def(r: &Record) -> i64: r.x)?, gg.num("y", def(r: &Record) -> f64: r.y)?, gg.text("name", def(r: &Record) -> str: r.name)?, gg.flag("active", def(r: &Record) -> bool: r.active)?]?)?
print(t.height(), t.names()?)?
base = gg.ggplot(t.share(), gg.aes(gg.factor("x"), "y", colour="active"), []?)?
first = base.plus([gg.geom_point()]?)?
second = base.plus([gg.geom_col(fill="blue")]?)?
mut empty = std.io.cursor(std.bytes.empty()?)
mut dots = std.io.cursor(std.bytes.empty()?)
mut bars = std.io.cursor(std.bytes.empty()?)
drop(base.write_svg(&mut empty)?)
drop(first.write_svg(&mut dots)?)
drop(second.write_svg(&mut bars)?)
print("<circle " in empty.data.to_text()?, "<circle " in dots.data.to_text()?, "data-geom=\"col\"" in bars.data.to_text()?)?
print(len(records), t.height())?
source = {"a": [1.0, 2.0]?}?
copied = gg.columns(&source)?
print(copied.height(), copied.names()?)?
"#
        )
    );
    assert_eq!(
        support::run_file_stdout(&source),
        "2 [\"x\", \"y\", \"name\", \"active\"]\nFalse True True\n2 2\n2 [\"a\"]"
    );
}

#[test]
fn validation_precedes_rendering() {
    let output = support::run_file_stdout(&program(
        r#"
match gg.table([("x", Column.Num([1.0]?)), ("x", Column.Num([1.0]?))]?):
    case Err(error):
        print(error)?
    case Ok(_):
        print("wrong")?
match gg.table([("x", Column.Num([1.0]?)), ("y", Column.Num([]?))]?):
    case Err(error):
        print(error)?
    case Ok(_):
        print("wrong")?
t = gg.table([("x", Column.Num([1.0]?)), ("y", Column.Num([2.0]?)), ("name", Column.Text(["A"]?))]?)?
maps = [gg.aes("typo", "y"), gg.aes("x"), gg.aes("x", "y", colour="name", color="name")]?
for mapping in maps:
    match gg.ggplot(t.share(), mapping, [gg.geom_point()]?):
        case Err(error):
            print(error)?
        case Ok(_):
            print("wrong")?
components = [gg.geom_point(size=-1), gg.geom_point(alpha=2), gg.geom_point(colour="red", color="blue"), gg.geom_point(colour="url(evil)"), gg.geom_histogram(bins=0), gg.coord_cartesian(xlim=(2.0, 1.0))]?
for component in components:
    match gg.ggplot(t.share(), gg.aes("x", "y"), [component]?):
        case Err(error):
            print(error)?
        case Ok(_):
            print("wrong")?
match gg.ggplot(t, gg.aes("name"), [gg.geom_histogram()]?):
    case Err(error):
        print(error)?
    case Ok(_):
        print("wrong")?
"#,
    ));
    assert!(!output.contains("wrong"), "{output}");
    assert_eq!(output.lines().count(), 12);
    for expected in [
        "DuplicateColumn",
        "ShapeMismatch",
        "UnknownColumn",
        "MissingAesthetic",
        "Conflict",
        "InvalidSetting",
        "WrongKind",
    ] {
        assert!(output.contains(expected), "{output}");
    }
}

#[test]
fn continuous_colour_discrete_factor_order_and_determinism() {
    let svg = render(
        r#"
t = gg.table([("x", Column.Int([10, 2, -1]?)), ("y", Column.Num([1.0, 2.0, 3.0]?))]?)?
p = gg.ggplot(t, gg.aes(gg.factor("x"), "y", colour="y"), [gg.geom_point(), gg.theme_minimal()]?)?
"#,
    );
    assert!(svg.find(">-1</text>").unwrap() < svg.find(">2</text>").unwrap());
    assert!(svg.find(">2</text>").unwrap() < svg.find(">10</text>").unwrap());
    assert!(svg.contains("fill=\"#132b43\""));
    assert!(svg.contains("fill=\"#56b1f7\""));
    let output = support::run_file_stdout(&program(
        r#"
t = gg.table([("x", Column.Num([1.0, 2.0]?)), ("y", Column.Num([2.0, 1.0]?))]?)?
p = gg.ggplot(t, gg.aes("x", "y"), [gg.geom_point()]?)?
mut a = std.io.cursor(std.bytes.empty()?)
mut b = std.io.cursor(std.bytes.empty()?)
drop(p.write_svg(&mut a)?)
drop(p.write_svg(&mut b)?)
print(a.data.to_text()? == b.data.to_text()?)?
"#,
    ));
    assert_eq!(output, "True");
}

#[test]
fn labels_distinguish_large_offsets_and_constant_colour_has_one_key() {
    let svg = render(
        r#"
t = gg.table([("x", Column.Num([1000000000.0, 1000000001.0, 1000000002.0]?)), ("y", Column.Num([1.0, 2.0, 3.0]?))]?)?
p = gg.ggplot(t, gg.aes("x", "y"), [gg.geom_point()]?)?
"#,
    );
    assert!(svg.contains(">1000000001</text>"), "{svg}");
    let svg = render(
        r#"
t = gg.table([("x", Column.Num([1.0]?)), ("y", Column.Num([1.0]?))]?)?
p = gg.ggplot(t, gg.aes("x", "y", colour="y"), [gg.geom_point()]?)?
"#,
    );
    // Canvas, panel, clip rectangle, and one legend key for the constant scale.
    assert_eq!(svg.matches("<rect ").count(), 4, "{svg}");
}

#[test]
fn paths_and_lines_break_at_missing_y() {
    for geom in ["geom_path", "geom_line"] {
        let svg = render(&format!(
            r#"
t = gg.table([("x", Column.Num([0.0, 1.0, 2.0, 3.0]?)), ("y", Column.MaybeNum([Some(0.0), Nothing, Some(2.0), Some(3.0)]?))]?)?
p = gg.ggplot(t, gg.aes("x", "y"), [gg.{geom}(), gg.theme_classic()]?)?
"#
        ));
        let geometry = svg
            .split("data-geom=")
            .nth(1)
            .unwrap()
            .split("</g>")
            .next()
            .unwrap();
        assert_eq!(geometry.matches("<line ").count(), 1, "{geometry}");
    }
}

#[test]
fn writer_failure_and_invalid_output_dimensions() {
    let source = format!(
        r#"from std.bytes import ByteSource
pub class Stalled:
    remaining: i64
    pub def write[S: ByteSource](self: &mut Stalled, data: &S, start: i64, stop: i64) -> Result[i64, StreamError]:
        count = self.remaining
        self.remaining = 0
        Ok(count)
    pub def flush(self: &mut Stalled) -> Result[(), StreamError]:
        Ok(())
{}"#,
        program(
            r#"
t = gg.table([("x", Column.Num([1.0]?)), ("y", Column.Num([2.0]?))]?)?
p = gg.ggplot(t, gg.aes("x", "y"), [gg.geom_point()]?)?
mut stalled = Stalled(7)
match p.write_svg(&mut stalled):
    case Err(error):
        print(error)?
    case Ok(_):
        print("wrong")?
match p.write_svg(&mut stalled, width=-1.0):
    case Err(PlotError.InvalidSetting(_)):
        print("dimensions")?
    case _:
        print("wrong")?
match p.save("invalid.png"):
    case Err(PlotError.InvalidSetting(_)):
        print("format")?
    case _:
        print("wrong")?
"#
        )
    );
    assert_eq!(
        support::run_file_stdout(&source),
        "std.plot.PlotError.Write(7, std.io.StreamError.WriteZero)\ndimensions\nformat"
    );
}

#[test]
fn dodge_and_proportion_stacks_snapshots() {
    let data = r#"t = gg.table([("x", Column.Text(["B", "A", "A", "B"]?)), ("y", Column.Num([3.0, 2.0, 4.0, 1.0]?)), ("g", Column.Text(["first", "first", "second", "second"]?))]?)?"#;
    for (position, name) in [
        ("position_dodge", "dodged"),
        ("position_fill", "proportions"),
    ] {
        let svg = render(&format!(
            "{data}\np = gg.ggplot(t, gg.aes(\"x\", \"y\", fill=\"g\"), [gg.geom_col(position=gg.{position}()), gg.theme_minimal()]?)?"
        ));
        snapshot(name, &svg);
    }
}

#[test]
fn empty_missing_and_extreme_data_return_errors_without_panics() {
    let output = support::run_file_stdout(&program(
        r#"
empty = gg.table([("x", Column.Num([]?)), ("y", Column.Num([]?))]?)?
p = gg.ggplot(empty, gg.aes("x", "y"), [gg.geom_point()]?)?
mut out = std.io.cursor(std.bytes.empty()?)
print(p.write_svg(&mut out)?)?
missing = gg.table([("x", Column.Num([1.0]?)), ("y", Column.MaybeNum([Nothing]?))]?)?
quiet = gg.ggplot(missing, gg.aes("x", "y"), [gg.geom_point(na_rm=True)]?)?
print(quiet.write_svg(&mut out)?)?
huge = gg.table([("x", Column.Num([1.0e200]?)), ("y", Column.Num([1.0]?))]?)?
bad = gg.ggplot(huge, gg.aes("x", "y"), [gg.geom_point()]?)?
match bad.write_svg(&mut out):
    case Err(PlotError.InvalidSetting(_)):
        print("magnitude")?
    case _:
        print("wrong")?
normal = gg.table([("x", Column.Num([0.0, 1.0]?)), ("y", Column.Num([0.0, 1.0]?))]?)?
zoom = gg.ggplot(normal, gg.aes("x", "y"), [gg.geom_point(), gg.coord_cartesian(xlim=(0.0, 1.0e-200))]?)?
match zoom.write_svg(&mut out):
    case Err(PlotError.InvalidSetting(_)):
        print("zoom")?
    case _:
        print("wrong")?
"#,
    ));
    assert_eq!(output, "[]\n[]\nmagnitude\nzoom");
}

#[test]
fn aggregated_aesthetics_do_not_silently_choose_the_first_colour() {
    let output = support::run_file_stdout(&program(
        r#"
t = gg.table([("x", Column.Num([1.0, 1.0]?)), ("c", Column.Num([1.0, 2.0]?))]?)?
match gg.ggplot(t.share(), gg.aes("x", colour="c"), [gg.geom_bar()]?):
    case Err(PlotError.WrongKind(_)):
        print("discrete required")?
    case _:
        print("wrong")?
p = gg.ggplot(t, gg.aes("x", colour=gg.factor("c"), group="x"), [gg.geom_bar()]?)?
mut output = std.io.cursor(std.bytes.empty()?)
match p.write_svg(&mut output):
    case Err(PlotError.Conflict(_)):
        print("ambiguous group")?
    case _:
        print("wrong")?
"#,
    ));
    assert_eq!(output, "discrete required\nambiguous group");
}

#[test]
fn units_convert_without_rasterising_and_private_state_is_protected() {
    let output = support::run_file_stdout(&program(
        r#"
t = gg.table([("x", Column.Num([1.0]?)), ("y", Column.Num([2.0]?))]?)?
p = gg.ggplot(t, gg.aes("x", "y"), [gg.geom_point()]?)?
mut output = std.io.cursor(std.bytes.empty()?)
drop(p.write_svg(&mut output, width=600.0, height=400.0, units=gg.Units.Px, dpi=100.0)?)
print("width=\"432.00pt\"" in output.data.to_text()?)?
"#,
    ));
    assert_eq!(output, "True");
    let diagnostic = support::check_diagnostic(&program(
        r#"
t = gg.table([]?)?
p = gg.Plot(t, gg.aes(), []?)
"#,
    ));
    assert!(diagnostic.contains("private"), "{diagnostic}");
}

#[test]
fn saved_svg_matches_streamed_output() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("chart.svg");
    let body = format!(
        r#"
t = gg.table([("x", Column.Num([1.0, 2.0]?)), ("y", Column.Num([2.0, 1.0]?))]?)?
p = gg.ggplot(t, gg.aes("x", "y"), [gg.geom_line(), gg.geom_point()]?)?
drop(p.save({:?})?)
"#,
        path.to_str().unwrap()
    );
    let svg = render(&body);
    assert_eq!(
        std::fs::read_to_string(path).unwrap().trim_end(),
        svg.trim_end()
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_preserves_the_plot_for_retry() {
    let mut body = String::from(
        r#"
t = gg.table([("x", Column.Num([0.0, 1.0, 2.0]?)), ("y", Column.Num([1.0, 2.0, 3.0]?)), ("group", Column.Text(["A", "A", "A"]?))]?)?
p = gg.ggplot(t, gg.aes("x", "y", colour="group", fill="y"), [gg.geom_line(), gg.geom_point(), gg.scale_colour_manual({"A": "navy"}?), gg.scale_fill_gradient()]?)?
"#,
    );
    let mut helpers = String::new();
    let budgets: Vec<_> = (0..256).step_by(8).chain([512, 1024, 4096]).collect();
    for budget in &budgets {
        body.push_str(&format!("check{budget}(&p)?\n"));
        helpers.push_str(&format!(
            r#"
def check{budget}(p: &gg.Plot) -> Result[(), PlotError | BytesError | IoError]:
    mut output = std.io.cursor(std.bytes.empty()?)
    print("__test_fail_allocations_after_{budget}__")?
    attempted = p.write_svg(&mut output)
    print("__test_restore_allocations__")?
    match attempted:
        case Ok(_):
            print("rendered")?
        case Err(error):
            print(error)?
    mut retry = std.io.cursor(std.bytes.empty()?)
    drop(p.write_svg(&mut retry)?)
    Ok(())
"#
        ));
    }
    let output = support::run_file_stdout(&format!("{}\n{helpers}", program(&body)));
    assert_eq!(output.lines().count(), budgets.len());
    assert!(output.contains("OutOfMemory"), "{output}");
    assert!(output.contains("rendered"), "{output}");
    assert!(
        output
            .lines()
            .all(|line| line == "rendered" || line.contains("OutOfMemory")),
        "{output}"
    );
}
