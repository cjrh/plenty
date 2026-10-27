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
    render_size(body, 6.0, 4.0)
}

fn render_size(body: &str, width: f64, height: f64) -> String {
    support::run_file_stdout(&program(&format!(
        "{body}\nmut output = std.io.cursor(std.bytes.empty()?)\nnotes = p.write_svg(&mut output, width={width:.1}, height={height:.1})?\nprint(output.data.to_text()?)?"
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
fn intervals_and_ribbons_include_their_bounds_in_axis_training() {
    let svg = render(
        r#"
data = gg.table([("x", Column.Num([1.0, 2.0, 3.0]?)), ("mean", Column.Num([2.0, 4.0, 3.0]?)), ("low", Column.Num([1.0, 2.0, 1.5]?)), ("high", Column.Num([3.0, 6.0, 4.5]?))]?)?
p = gg.ggplot(data, gg.aes("x", "mean", ymin="low", ymax="high"), [gg.geom_ribbon(fill="blue", alpha=0.2), gg.geom_errorbar(), gg.geom_line(), gg.geom_point(), gg.theme_minimal()]?)?
"#,
    );
    assert_eq!(svg.matches("<polygon ").count(), 1);
    assert_eq!(svg.matches("<circle ").count(), 3);
    let bars = svg
        .split("data-geom=\"errorbar\"")
        .nth(1)
        .unwrap()
        .split("</g>")
        .next()
        .unwrap();
    assert_eq!(bars.matches("<line ").count(), 9);
    assert!(svg.contains(">6</text>"));
    snapshot("intervals-ribbon", &svg);
}

#[test]
fn ribbons_break_at_missing_bounds_and_ranges_flip() {
    let svg = render(
        r#"
data = gg.table([("x", Column.Num([0.0, 1.0, 2.0, 3.0, 4.0]?)), ("low", Column.Num([0.0, 1.0, 2.0, 1.0, 0.0]?)), ("high", Column.MaybeNum([Some(2.0), Some(3.0), Nothing, Some(3.0), Some(2.0)]?))]?)?
p = gg.ggplot(data, gg.aes("x", ymin="low", ymax="high"), [gg.geom_ribbon(), gg.geom_linerange(), gg.coord_flip()]?)?
"#,
    );
    assert_eq!(svg.matches("<polygon ").count(), 2);
    let ranges = svg
        .split("data-geom=\"linerange\"")
        .nth(1)
        .unwrap()
        .split("</g>")
        .next()
        .unwrap();
    assert_eq!(ranges.matches("<line ").count(), 4);
}

#[test]
fn wrapped_facets_share_colours_and_repeat_reference_layers() {
    let svg = render_size(
        r#"
data = gg.table([("x", Column.Num([1.0, 2.0, 1.0, 2.0]?)), ("y", Column.Num([1.0, 2.0, 10.0, 20.0]?)), ("site", Column.Text(["North", "North", "South", "South"]?)), ("kind", Column.Text(["B", "A", "A", "B"]?))]?)?
reference = gg.table([("x", Column.Num([1.0, 2.0]?)), ("y", Column.Num([5.0, 5.0]?))]?)?
p = gg.ggplot(data, gg.aes("x", "y", colour="kind"), [gg.geom_point(), gg.geom_line(data=reference, mapping=gg.aes("x", "y"), inherit_aes=False, colour="navy"), gg.facet_wrap("site", ncol=2), gg.labs(title="Results by site"), gg.theme_minimal()]?)?
"#,
        10.0,
        4.0,
    );
    assert_eq!(svg.matches("data-panel=").count(), 2);
    assert_eq!(svg.matches("data-geom=\"line\"").count(), 2);
    assert_eq!(svg.matches("<circle ").count(), 4);
    assert!(svg.contains("id=\"panel-0\"") && svg.contains("id=\"panel-1\""));
    // One shared legend, not one guide per panel.
    assert_eq!(svg.matches(">kind</text>").count(), 1);
    snapshot("facet-wrap", &svg);
}

#[test]
fn facet_counts_are_computed_per_panel_and_free_y_changes_ranges() {
    let body = r#"
data = gg.table([("x", Column.Text(["A", "A", "A"]?)), ("site", Column.Text(["North", "North", "South"]?))]?)?
p = gg.ggplot(data, gg.aes("x"), [gg.geom_bar(), gg.facet_wrap("site", ncol=2, scales=SCALES)]?)?
"#;
    let heights = |svg: &str| -> Vec<f64> {
        svg.split("data-geom=\"bar\"")
            .skip(1)
            .map(|part| {
                part.split("<rect ")
                    .nth(1)
                    .unwrap()
                    .split("height=\"")
                    .nth(1)
                    .unwrap()
                    .split('"')
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap()
            })
            .collect()
    };
    let fixed = render_size(&body.replace("SCALES", "gg.Scales.Fixed"), 8.0, 4.0);
    let free = render_size(&body.replace("SCALES", "gg.Scales.FreeY"), 8.0, 4.0);
    let fixed = heights(&fixed);
    let free = heights(&free);
    assert!((fixed[0] - 2.0 * fixed[1]).abs() < 0.02);
    assert!((free[0] - free[1]).abs() < 0.02);
}

#[test]
fn facet_grid_retains_empty_combinations_and_supports_free_scales() {
    let svg = render_size(
        r#"
data = gg.table([("x", Column.Num([1.0, 2.0, 100.0]?)), ("y", Column.Num([1.0, 20.0, 30.0]?)), ("region", Column.Text(["North", "South", "South"]?)), ("period", Column.Text(["Before", "Before", "After"]?))]?)?
p = gg.ggplot(data, gg.aes("x", "y"), [gg.geom_point(), gg.facet_grid(rows="region", cols="period", scales=gg.Scales.Free), gg.theme_minimal()]?)?
"#,
        10.0,
        8.0,
    );
    assert_eq!(svg.matches("data-panel=").count(), 4);
    assert_eq!(svg.matches("<circle ").count(), 3);
    snapshot("facet-grid", &svg);
}

#[test]
fn facet_and_interval_validation_is_eager() {
    for (mapping, component, expected) in [
        (
            "gg.aes(\"x\", \"y\")",
            "gg.geom_errorbar()",
            "MissingAesthetic",
        ),
        (
            "gg.aes(\"x\", ymin=\"high\", ymax=\"low\")",
            "gg.geom_ribbon()",
            "InvalidSetting",
        ),
        (
            "gg.aes(\"site\", ymin=\"low\", ymax=\"high\")",
            "gg.geom_ribbon()",
            "WrongKind",
        ),
        (
            "gg.aes(\"x\", \"y\")",
            "gg.geom_point(), gg.facet_wrap(\"missing\")",
            "InvalidSetting",
        ),
        (
            "gg.aes(\"x\", \"y\")",
            "gg.geom_point(), gg.facet_wrap(\"site\", ncol=1, nrow=1)",
            "InvalidSetting",
        ),
    ] {
        let source = program(&format!(
            "data = gg.table([(\"x\", Column.Num([1.0, 2.0]?)), (\"y\", Column.Num([2.0, 3.0]?)), (\"low\", Column.Num([1.0, 2.0]?)), (\"high\", Column.Num([3.0, 4.0]?)), (\"site\", Column.Text([\"A\", \"B\"]?))]?)?\nmatch gg.ggplot(data, {mapping}, [{component}]?):\n    case Err(error):\n        print(error)?\n    case Ok(_):\n        print(\"unexpected\")?"
        ));
        assert!(support::run_file_stdout(&source).contains(expected));
    }
}

#[test]
fn facets_keep_missing_categories_and_allow_immutable_replacement() {
    let output = support::run_file_stdout(&program(
        r#"
data = gg.table([("x", Column.Num([1.0, 2.0, 3.0]?)), ("y", Column.Num([1.0, 2.0, 3.0]?)), ("site", Column.MaybeText([Some("A"), Nothing, Some("B")]?))]?)?
base = gg.ggplot(data, gg.aes("x", "y"), [gg.geom_point()]?)?
changed = base.plus([gg.facet_wrap("site", ncol=1)]?)?
mut first = std.io.cursor(std.bytes.empty()?)
mut second = std.io.cursor(std.bytes.empty()?)
drop(base.write_svg(&mut first)?)
drop(changed.write_svg(&mut second, width=6.0, height=10.0)?)
print("data-panel" in first.data.to_text()?, "site = (missing)" in second.data.to_text()?)?
"#,
    ));
    assert_eq!(output, "False True");
}

#[test]
fn facet_layout_preflights_every_panel_before_writing() {
    let output = support::run_file_stdout(&program(
        r#"
data = gg.table([("x", Column.Num([1.0, 2.0]?)), ("y", Column.Num([1.0, 2.0]?)), ("site", Column.Text(["A", "B"]?))]?)?
p = gg.ggplot(data, gg.aes("x", "y"), [gg.geom_point(), gg.facet_wrap("site", ncol=2)]?)?
mut output = std.io.cursor(std.bytes.empty()?)
match p.write_svg(&mut output, width=2.0, height=2.0):
    case Err(PlotError.InvalidSetting(_)):
        print(output.data.len())?
    case _:
        print("wrong")?
"#,
    ));
    assert_eq!(output, "0");
}

#[test]
fn transformed_intervals_keep_bound_order_and_report_invalid_domains() {
    let output = support::run_file_stdout(&program(
        r#"
data = gg.table([("x", Column.Num([1.0, 2.0]?)), ("low", Column.Num([0.0, 10.0]?)), ("high", Column.Num([10.0, 100.0]?))]?)?
p = gg.ggplot(data, gg.aes("x", ymin="low", ymax="high"), [gg.geom_errorbar(), gg.scale_y_log10()]?)?
mut output = std.io.cursor(std.bytes.empty()?)
print(p.write_svg(&mut output)?)?
print(output.data.to_text()?)?
"#,
    ));
    assert!(output.contains("Removed 1 rows"));
    let geometry = output
        .split("data-geom=\"errorbar\"")
        .nth(1)
        .unwrap()
        .split("</g>")
        .next()
        .unwrap();
    assert_eq!(geometry.matches("<line ").count(), 3);
    let svg = render(
        r#"
data = gg.table([("x", Column.Num([1.0]?)), ("low", Column.Num([1.0]?)), ("high", Column.Num([3.0]?))]?)?
p = gg.ggplot(data, gg.aes("x", ymin="low", ymax="high"), [gg.geom_errorbar(), gg.scale_y_reverse(), gg.coord_flip()]?)?
"#,
    );
    assert!(svg.contains("data-geom=\"errorbar\""));
}

#[test]
fn grid_reference_layers_repeat_only_across_the_missing_dimension() {
    let svg = render_size(
        r#"
data = gg.table([("x", Column.Num([1.0, 2.0, 3.0]?)), ("y", Column.Num([1.0, 2.0, 3.0]?)), ("row", Column.Text(["N", "N", "S"]?)), ("col", Column.Text(["A", "B", "A"]?))]?)?
reference = gg.table([("x", Column.Num([1.5]?)), ("y", Column.Num([2.5]?)), ("row", Column.Text(["N"]?))]?)?
p = gg.ggplot(data, gg.aes("x", "y"), [gg.geom_point(), gg.geom_point(data=reference, colour="red"), gg.facet_grid(rows="row", cols="col")]?)?
"#,
        10.0,
        8.0,
    );
    let counts: Vec<_> = svg
        .split("data-panel=")
        .skip(1)
        .map(|panel| panel.matches("<circle ").count())
        .collect();
    assert_eq!(counts, [2, 2, 1, 0]);
}

#[test]
fn ribbons_reject_ambiguous_shapes_and_styles() {
    for (x, fill) in [("[1.0, 1.0]?", ""), ("[1.0, 2.0]?", ", fill=\"x\"")] {
        let output = support::run_file_stdout(&program(&format!(
            "data = gg.table([(\"x\", Column.Num({x})), (\"low\", Column.Num([1.0, 2.0]?)), (\"high\", Column.Num([3.0, 4.0]?))]?)?\np = gg.ggplot(data, gg.aes(\"x\", ymin=\"low\", ymax=\"high\"{fill}), [gg.geom_ribbon()]?)?\nmut output = std.io.cursor(std.bytes.empty()?)\nmatch p.write_svg(&mut output):\n    case Err(PlotError.Conflict(_)):\n        print(\"ambiguous\")?\n    case _:\n        print(\"wrong\")?"
        )));
        assert_eq!(output, "ambiguous");
    }
}

#[test]
fn wrap_layout_handles_fractional_cell_dimensions_and_unused_cells() {
    let svg = render_size(
        r#"
data = gg.table([("x", Column.Text(["A", "A", "A", "A", "A"]?)), ("site", Column.Text(["One", "Two", "Three", "Four", "Five"]?))]?)?
p = gg.ggplot(data, gg.aes("x"), [gg.geom_bar(), gg.facet_wrap("site", ncol=3)]?)?
"#,
        10.0,
        7.0,
    );
    assert_eq!(svg.matches("data-panel=").count(), 5);
    assert_eq!(svg.matches("data-geom=\"bar\"").count(), 5);
}

#[test]
fn layer_data_and_mapping_train_shared_scales() {
    let svg = render(
        r#"
data = gg.table([("x", Column.Num([1.0, 2.0]?)), ("y", Column.Num([2.0, 3.0]?)), ("g", Column.Text(["B", "A"]?))]?)?
summary = gg.table([("at", Column.Num([1.0, 3.0]?)), ("mean", Column.Num([2.5, 4.0]?)), ("series", Column.Text(["A", "C"]?))]?)?
p = gg.ggplot(data, gg.aes("x", "y", colour="g"), [gg.geom_point(), gg.geom_point(data=summary, mapping=gg.aes("at", "mean", colour="series"), inherit_aes=False)]?)?
"#,
    );
    let circles: Vec<_> = svg
        .lines()
        .filter(|line| line.contains("<circle "))
        .collect();
    assert_eq!(circles.len(), 4);
    for (circle, colour) in circles
        .iter()
        .zip(["#00BFC4", "#F8766D", "#F8766D", "#7CAE00"])
    {
        assert!(circle.contains(colour), "{circle}");
    }
    snapshot("layer-data", &svg);
}

#[test]
fn layers_override_individual_aesthetics_and_can_disable_inheritance() {
    let svg = render(
        r#"
data = gg.table([("x", Column.Num([1.0, 2.0]?)), ("y", Column.Num([2.0, 3.0]?)), ("other", Column.Num([4.0, 5.0]?))]?)?
counts = gg.table([("category", Column.Text(["a", "a", "b"]?))]?)?
p = gg.ggplot(data, gg.aes("x", "y"), [gg.geom_point(), gg.geom_line(mapping=gg.aes(y="other")), gg.scale_y_continuous(breaks=Some([2.0, 5.0]?), labels=Some(["low", "high"]?))]?)?
"#,
    );
    assert_eq!(svg.matches("<circle ").count(), 2);
    assert!(svg.contains(">low</text>"));
    assert!(svg.contains(">high</text>"));
    let svg = render(
        r#"
data = gg.table([("unused", Column.Num([1.0]?))]?)?
counts = gg.table([("category", Column.Text(["a", "a", "b"]?))]?)?
p = gg.ggplot(data, gg.aes("missing", "also_missing"), [gg.geom_bar(data=counts, mapping=gg.aes("category"), inherit_aes=False)]?)?
"#,
    );
    assert!(svg.contains("data-geom=\"bar\""));
}

#[test]
fn continuous_scales_transform_positions_and_label_original_units() {
    let svg = render(
        r#"
data = gg.table([("x", Column.Num([1.0, 10.0, 100.0]?)), ("y", Column.Num([1.0, 4.0, 9.0]?))]?)?
p = gg.ggplot(data, gg.aes("x", "y"), [gg.geom_point(), gg.scale_x_log10(breaks=Some([1.0, 10.0, 100.0]?), labels=Some(["one", "ten", "hundred"]?)), gg.scale_y_continuous(transform=gg.Transform.Sqrt, breaks=Some([1.0, 4.0, 9.0]?))]?)?
"#,
    );
    let circles: Vec<_> = svg
        .lines()
        .filter(|line| line.contains("<circle "))
        .collect();
    assert_eq!(circles.len(), 3);
    let coords: Vec<_> = circles
        .iter()
        .map(|line| {
            line.split("cx=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap()
                .parse::<f64>()
                .unwrap()
        })
        .collect();
    assert!(((coords[1] - coords[0]) - (coords[2] - coords[1])).abs() < 0.02);
    assert!(svg.contains(">hundred</text>"));
    snapshot("transformed-axes", &svg);
}

#[test]
fn discrete_limits_order_filter_and_label_categories_across_layers() {
    let svg = render(
        r#"
data = gg.table([("x", Column.Text(["a", "b", "c"]?)), ("y", Column.Num([1.0, 2.0, 3.0]?))]?)?
p = gg.ggplot(data, gg.aes("x", "y"), [gg.geom_point(), gg.scale_x_discrete(limits=Some(["c", "a", "unused"]?), labels=Some(["Third", "First", "Empty"]?))]?)?
"#,
    );
    let circles: Vec<_> = svg
        .lines()
        .filter(|line| line.contains("<circle "))
        .collect();
    assert_eq!(circles.len(), 2);
    assert!(svg.contains(">Third</text>") && svg.contains(">Empty</text>"));
    snapshot("ordered-axis", &svg);
}

#[test]
fn scale_limits_filter_histograms_but_coordinate_limits_only_zoom() {
    let data = r#"data = gg.table([("x", Column.Num([0.0, 1.0, 2.0, 3.0, 4.0]?))]?)?"#;
    for (component, removed) in [
        ("gg.scale_x_continuous(limits=(1.0, 3.0))", true),
        ("gg.coord_cartesian(xlim=(1.0, 3.0))", false),
    ] {
        let output = support::run_file_stdout(&program(&format!(
            "{data}\np = gg.ggplot(data, gg.aes(\"x\"), [gg.geom_histogram(bins=2), {component}]?)?\nmut output = std.io.cursor(std.bytes.empty()?)\nprint(p.write_svg(&mut output)?)?"
        )));
        assert_eq!(output.contains("Removed 2 rows"), removed, "{output}");
    }
}

#[test]
fn layer_and_position_scale_errors_are_eager() {
    for (components, expected) in [
        (
            "gg.geom_point(mapping=gg.aes(y=\"missing\"))",
            "UnknownColumn",
        ),
        ("gg.geom_point(inherit_aes=False)", "MissingAesthetic"),
        (
            "gg.geom_point(), gg.scale_x_log10(limits=(0.0, 10.0))",
            "InvalidSetting",
        ),
        (
            "gg.geom_point(), gg.scale_x_continuous(breaks=Some([1.0, 1.0]?))",
            "InvalidSetting",
        ),
        (
            "gg.geom_point(), gg.scale_x_continuous(labels=Some([\"a\"]?))",
            "InvalidSetting",
        ),
        (
            "gg.geom_point(), gg.scale_x_continuous(breaks=Some([1.0]?), labels=Some([]?))",
            "ShapeMismatch",
        ),
        ("gg.geom_point(), gg.scale_x_discrete()", "WrongKind"),
        (
            "gg.geom_point(), gg.geom_point(mapping=gg.aes(x=\"category\"))",
            "WrongKind",
        ),
    ] {
        let output = support::run_file_stdout(&program(&format!(
            "data = gg.table([(\"x\", Column.Num([1.0]?)), (\"y\", Column.Num([2.0]?)), (\"category\", Column.Text([\"A\"]?))]?)?\nmatch gg.ggplot(data, gg.aes(\"x\", \"y\"), [{components}]?):\n    case Err(error):\n        print(error)?\n    case Ok(_):\n        print(\"unexpected success\")?"
        )));
        assert!(output.contains(expected), "{output}");
    }
}

#[test]
fn plus_preserves_layer_tables_and_replaces_axis_scales() {
    let output = support::run_file_stdout(&program(
        r#"
empty = gg.table([]?)?
data = gg.table([("x", Column.Num([1.0, 10.0]?)), ("y", Column.Num([1.0, 2.0]?))]?)?
base = gg.ggplot(empty, gg.aes(), [gg.geom_point(data=data, mapping=gg.aes("x", "y"))]?)?
changed = base.plus([gg.scale_x_log10(breaks=Some([1.0, 10.0]?), labels=Some(["First", "Last"]?))]?)?
mut first = std.io.cursor(std.bytes.empty()?)
mut second = std.io.cursor(std.bytes.empty()?)
drop(base.write_svg(&mut first)?)
drop(changed.write_svg(&mut second)?)
print("First" in first.data.to_text()?, "First" in second.data.to_text()?)?
"#,
    ));
    assert_eq!(output, "False True");
}

#[test]
fn shared_continuous_colour_range_and_reversed_axis() {
    let svg = render(
        r##"
a = gg.table([("x", Column.Num([1.0]?)), ("y", Column.Num([1.0]?)), ("c", Column.Num([0.0]?))]?)?
b = gg.table([("x", Column.Num([2.0]?)), ("y", Column.Num([2.0]?)), ("c", Column.Num([10.0]?))]?)?
p = gg.ggplot(a, gg.aes("x", "y", colour="c"), [gg.geom_point(), gg.geom_point(data=b), gg.scale_colour_gradient(low="#000000", high="#ffffff"), gg.scale_x_reverse()]?)?
"##,
    );
    let circles: Vec<_> = svg
        .lines()
        .filter(|line| line.contains("<circle "))
        .collect();
    assert!(circles[0].contains("#000000"));
    assert!(circles[1].contains("#ffffff"));
    let x = |s: &str| {
        s.split("cx=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .parse::<f64>()
            .unwrap()
    };
    assert!(x(circles[0]) > x(circles[1]));
}

#[test]
fn long_labels_wrap_within_the_page_and_expand_margins() {
    let svg = render(
        r#"
data = gg.table([("category", Column.Text(["Northern coastal district", "Southern mountain district"]?)), ("value", Column.Num([2.0, 4.0]?)), ("group", Column.Text(["Observations from primary survey", "Observations from follow-up survey"]?))]?)?
p = gg.ggplot(data, gg.aes("category", "value", fill="group"), [gg.geom_col(), gg.labs(title="A comparison of regional survey totals with deliberately long descriptive labels", x="Survey region", y="Recorded observations", fill="Survey collection method"), gg.theme_minimal()]?)?
"#,
    );
    assert!(svg.contains(">Northern</text>") || svg.contains(">Northern coastal</text>"));
    assert!(!svg.contains(">Northern coastal district</text>"));
    // SVG textLength is the layout contract, including with fallback fonts.
    // Check every unrotated text box against the physical canvas bounds.
    let mut rotated = false;
    for line in svg.lines() {
        if line.contains("rotate(-90)") {
            rotated = true;
        } else if line == "</g>" {
            rotated = false;
        } else if line.starts_with("<text ") && !rotated {
            let attr = |name: &str| -> f64 {
                line.split(&format!("{name}=\""))
                    .nth(1)
                    .unwrap()
                    .split('"')
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap()
            };
            let x = attr("x");
            let width = attr("textLength");
            let start = if line.contains("text-anchor=\"middle\"") {
                x - width / 2.0
            } else if line.contains("text-anchor=\"end\"") {
                x - width
            } else {
                x
            };
            assert!(start >= -0.02 && start + width <= 432.02, "{line}");
            assert!(attr("y") >= 0.0 && attr("y") <= 288.0, "{line}");
        }
    }
    snapshot("wrapped-labels", &svg);
}

#[test]
fn layout_failure_precedes_stream_output_and_larger_dimensions_recover() {
    let output = support::run_file_stdout(&program(
        r#"
data = gg.table([("x", Column.Text(["North", "South"]?)), ("y", Column.Num([2.0, 4.0]?))]?)?
p = gg.ggplot(data, gg.aes("x", "y", colour="x"), [gg.geom_point(), gg.labs(title="A detailed description that needs more room"), gg.theme_minimal(base_size=20)]?)?
mut small = std.io.cursor(std.bytes.empty()?)
match p.write_svg(&mut small, width=2.0, height=2.0):
    case Err(PlotError.InvalidSetting(_)):
        print("layout", small.data.len())?
    case _:
        print("wrong")?
mut large = std.io.cursor(std.bytes.empty()?)
print(p.write_svg(&mut large, width=10.0, height=6.0)?)?
"#,
    ));
    assert_eq!(output, "layout 0\n[]");
}

#[test]
fn flipped_multiline_labels_and_explicit_line_breaks() {
    let svg = render(
        r#"
data = gg.table([("x", Column.Text(["First region with a long name", "Second region with a long name"]?)), ("y", Column.Num([2.0, 4.0]?))]?)?
p = gg.ggplot(data, gg.aes("x", "y"), [gg.geom_col(), gg.coord_flip(), gg.labs(title="Regional results\nSecond title line"), gg.theme_classic()]?)?
"#,
    );
    assert!(svg.contains(">Regional results</text>"));
    assert!(svg.contains(">Second title line</text>"));
    snapshot("flipped-wrapped-labels", &svg);
}

#[test]
fn categorical_positions_are_shared_across_independent_tables() {
    let svg = render(
        r#"
a = gg.table([("x", Column.Text(["B"]?)), ("y", Column.Num([1.0]?))]?)?
b = gg.table([("x", Column.Text(["A", "B"]?)), ("y", Column.Num([2.0, 3.0]?))]?)?
p = gg.ggplot(a, gg.aes("x", "y"), [gg.geom_point(), gg.geom_point(data=b)]?)?
"#,
    );
    let xs: Vec<_> = svg
        .lines()
        .filter(|line| line.contains("<circle "))
        .map(|line| {
            line.split("cx=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap()
                .parse::<f64>()
                .unwrap()
        })
        .collect();
    assert_eq!(xs[0], xs[2]);
    assert!(xs[1] < xs[0]);
}

#[test]
fn log_histogram_omits_invalid_domain_values_before_binning() {
    let output = support::run_file_stdout(&program(
        r#"
data = gg.table([("x", Column.Num([0.0, 1.0, 10.0, 100.0]?))]?)?
p = gg.ggplot(data, gg.aes("x"), [gg.geom_histogram(bins=2), gg.scale_x_log10()]?)?
mut output = std.io.cursor(std.bytes.empty()?)
print(p.write_svg(&mut output)?)?
print(output.data.to_text()?)?
"#,
    ));
    assert!(output.contains("Removed 1 rows"));
    let geometry = output
        .split("data-geom=\"histogram\"")
        .nth(1)
        .unwrap()
        .split("</g>")
        .next()
        .unwrap();
    let heights: Vec<_> = geometry
        .lines()
        .filter(|line| line.contains("<rect "))
        .map(|line| {
            line.split("height=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap()
                .parse::<f64>()
                .unwrap()
        })
        .collect();
    assert_eq!(heights.len(), 2);
    assert!((heights[1] - 2.0 * heights[0]).abs() < 0.02);
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
match gg.ggplot(huge, gg.aes("x", "y"), [gg.geom_point()]?):
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
fn faceted_interval_allocation_failure_preserves_retry() {
    let mut body = String::from(
        r#"
t = gg.table([("x", Column.Num([1.0, 2.0, 1.0, 2.0]?)), ("low", Column.Num([1.0, 2.0, 3.0, 4.0]?)), ("high", Column.Num([2.0, 3.0, 4.0, 5.0]?)), ("site", Column.Text(["A", "A", "B", "B"]?))]?)?
p = gg.ggplot(t, gg.aes("x", ymin="low", ymax="high"), [gg.geom_ribbon(), gg.geom_errorbar(), gg.facet_wrap("site")]?)?
"#,
    );
    let mut helpers = String::new();
    let budgets: Vec<_> = (0..128)
        .step_by(16)
        .chain([512, 1024, 4096, 16384])
        .collect();
    for budget in &budgets {
        body.push_str(&format!("facet_check{budget}(&p)?\n"));
        helpers.push_str(&format!(
            r#"
def facet_check{budget}(p: &gg.Plot) -> Result[(), PlotError | BytesError | IoError]:
    mut output = std.io.cursor(std.bytes.empty()?)
    print("__test_fail_allocations_after_{budget}__")?
    attempted = p.write_svg(&mut output, width=10.0, height=4.0)
    print("__test_restore_allocations__")?
    match attempted:
        case Ok(_):
            print("rendered")?
        case Err(error):
            print(error)?
    mut retry = std.io.cursor(std.bytes.empty()?)
    drop(p.write_svg(&mut retry, width=10.0, height=4.0)?)
    Ok(())
"#
        ));
    }
    let output = support::run_file_stdout(&format!("{}\n{helpers}", program(&body)));
    assert_eq!(output.lines().count(), budgets.len());
    assert!(
        output.contains("OutOfMemory") && output.contains("rendered"),
        "{output}"
    );
    assert!(
        output
            .lines()
            .all(|line| line == "rendered" || line.contains("OutOfMemory")),
        "{output}"
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
