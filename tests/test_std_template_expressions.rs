//! Bounded expression syntax, short circuiting and conditional branch chains.
mod support;

fn program(body: &str) -> String {
    format!(
        "import std.template\nfrom std.template import TemplateError\nfrom std.json import Json, Member\nfrom std.bytes import BytesError\ndef main() -> Result[(), AllocError | IoError | TemplateError | OverflowError[i64]]:\n{}    Ok(())\n",
        body.lines()
            .map(|line| format!("    {line}\n"))
            .collect::<String>()
    )
}

fn output(source: &str, optimize: bool) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    let options = plenty::CompileOptions {
        optimize,
        test_hooks: true,
        ..Default::default()
    };
    plenty::compile_file_to_executable_with_options(&path, &executable, None, &options)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn nav_and_selected_options_use_raw_scalar_fields() {
    let source = program(
        r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
templates.add("nav", "{% for item in items %}<a class=\"{% if current == item.id %}active{% elif signed_in and item.id == 'profile' %}member{% else %}other{% endif %}\">{{ item.label | escape_text }}</a>{% endfor %}")?
templates.add("options", "{% for item in items %}<option{% if selected == item.id %} selected{% endif %}>{{ item.label | escape_text }}</option>{% endfor %}")?
values = Json.Object([Member("current", Json.Text("home")), Member("selected", Json.Text("profile")), Member("signed_in", Json.Bool(True)), Member("items", Json.Array([Json.Object([Member("id", Json.Text("home")), Member("label", Json.Text("Home & news"))]?), Json.Object([Member("id", Json.Text("profile")), Member("label", Json.Text("Profile"))]?)]?))]?)
print(templates.render("nav", &values)?)?
print(templates.render("options", &values)?)?
"#,
    );
    assert_eq!(
        support::run_file_stdout(&source),
        "<a class=\"active\">Home &amp; news</a><a class=\"member\">Profile</a>\n<option>Home &amp; news</option><option selected>Profile</option>"
    );
}

#[test]
fn precedence_scalar_kinds_and_parenthesized_filters_are_explicit() {
    let source = program(
        r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
templates.add("page", "{{ true or false and false }}|{{ (true or false) and false }}|{{ not ' x ' | trim == 'x' }}|{{ (not false) == true }}|{{ true == 1 }}|{{ 1 == 1.0 }}|{{ null == null }}|{{ 'é' == 'é' }}|{{ 1 != true }}|{{ -0.0 == 0.0 }}|{{ ((' <a&> ' | trim)) | escape_text }}|{{ 'a==b|and(or)' == 'a==b|and(or)' }}|{{ not not true }}|{{ ('<a\"&>') | escape_text | escape_double_quoted }}")?
context = Json.Object([]?)
print(templates.render("page", &context)?)?
"#,
    );
    for optimize in [false, true] {
        assert_eq!(
            output(&source, optimize),
            "true|false|false|true|false|false|true|true|true|true|&lt;a&amp;&gt;|true|true|<a&quot;&amp;>"
        );
    }
}

#[test]
fn short_circuit_skips_dynamic_errors_but_equality_evaluates_both_operands() {
    let source = program(
        r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
templates.add("skip", "{{ false and absent }}|{{ true or absent }}|{{ false and (items == items) }}|{{ true or (42 | upper) }}|{{ 'text' and 2 }}|{{ items or false }}")?
templates.add("missing", "{{ false or absent }}")?
templates.add("eager", "{{ false == absent }}")?
templates.add("collections", "{{ items != null }}")?
templates.add("filtered", "{{ (false or true) | trim }}")?
context = Json.Object([Member("items", Json.Array([Json.Int(1)]?))]?)
print(templates.render("skip", &context)?)?
for name in ["missing", "eager", "collections", "filtered"]?:
    match templates.render(&name, &context):
        case Err(TemplateError.MissingValue(_)):
            print("missing")?
        case Err(TemplateError.TypeMismatch):
            print("type")?
        case _:
            print("wrong")?
print(templates.render("skip", &context)?)?
"#,
    );
    for optimize in [false, true] {
        assert_eq!(
            output(&source, optimize),
            "false|true|false|true|true|true\nmissing\nmissing\ntype\ntype\nfalse|true|false|true|true|true"
        );
    }
}

#[test]
fn nested_and_long_flat_elif_chains_preserve_scope_and_depth() {
    let chain = format!(
        "{{% if false %}}bad{}{{% elif true %}}last{{% else %}}bad{{% endif %}}",
        "{% elif false %}bad".repeat(100)
    );
    let source = program(&format!(
        r#"
mut bounds = std.template.limits()
bounds.max_depth = 3
mut templates = std.template.engine(&bounds)?
templates.add("chain", {chain:?})?
templates.add("nested", "{{% for x in xs %}}{{% if x == 0 %}}zero{{% elif x == 1 %}}{{% if true %}}one{{% elif absent %}}bad{{% endif %}}{{% elif x == 2 %}}two{{% else %}}other{{% endif %}};{{% else %}}empty{{% endfor %}}")?
context = Json.Object([Member("xs", Json.Array([Json.Int(0), Json.Int(1), Json.Int(2), Json.Int(3)]?))]?)
print(templates.render("chain", &context)?)?
print(templates.render("nested", &context)?)?
"#
    ));
    assert_eq!(
        support::run_file_stdout(&source),
        "last\nzero;one;two;other;"
    );
}

#[test]
fn malformed_expressions_and_elif_are_rejected_before_registration() {
    let source = program(
        r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
for text in ["{% if false %}{{ true or }}{% endif %}", "{{ 1 == 1 == 1 }}", "{{ true and }}", "{{ () }}", "{{ (true }}", "{{ true) }}", "{{ not }}", "{{ 1 = 1 }}", "{{ !true }}", "{{ true or (absent | unknown) }}", "{{ upper(name) }}", "{% elif true %}bad{% endif %}", "{% if true %}x{% else %}y{% elif true %}z{% endif %}", "{% for x in items %}{% elif true %}bad{% endfor %}", "{% if false %}x{% elif true %}y{% endfor %}", "{% for x in items %}{% endif %}"]?:
    match templates.add("recover", text):
        case Err(TemplateError.Syntax(_)):
            print(True)?
        case _:
            print(False)?
templates.add("recover", "{{ true }}")?
context = Json.Object([]?)
print(templates.render("recover", &context)?)?
"#,
    );
    let expected = format!(
        "{}\ntrue",
        std::iter::repeat_n("True", 16)
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert_eq!(support::run_file_stdout(&source), expected);
}

#[test]
fn ast_caps_are_aggregate_transactional_and_separate_from_token_and_work_limits() {
    let source = program(
        r#"
mut bounds = std.template.limits()
bounds.max_tokens = 4
bounds.max_expression_nodes = 3
mut templates = std.template.engine(&bounds)?
match templates.add("recover", "{{ true or }}"):
    case Err(TemplateError.Syntax(_)):
        print(True)?
    case _:
        print(False)?
templates.add("fit", "{{ true or false }}")?
match templates.add("recover", "{{ true }}"):
    case Err(TemplateError.LimitExceeded):
        print(True)?
    case _:
        print(False)?
templates.add("recover", "ok")?
context = Json.Object([]?)
print(templates.render("fit", &context)?, templates.render("recover", &context)?)?
bounds.max_expression_nodes = 0
mut empty = std.template.engine(&bounds)?
empty.add("empty", "")?
match empty.add("value", "{{ true }}"):
    case Err(TemplateError.LimitExceeded):
        print(True)?
    case _:
        print(False)?
bounds.max_expression_nodes = 100
bounds.max_depth = 3
mut deep = std.template.engine(&bounds)?
for text in ["{{ not not not true }}", "{{ (((true))) }}", "{{ true and true and true and true }}"]?:
    match deep.add("bad", text):
        case Err(TemplateError.LimitExceeded):
            print(True)?
        case _:
            print(False)?
deep.add("fit", "{{ not not true }}")?
print(deep.render("fit", &context)?)?
bounds.max_steps = 5
bounds.max_tokens = 10
mut work = std.template.engine(&bounds)?
work.add("skip", "{{ true or absent }}")?
work.add("visit", "{{ false or true }}")?
print(work.render("skip", &context)?)?
match work.render("visit", &context):
    case Err(TemplateError.LimitExceeded):
        print(True)?
    case _:
        print(False)?
print(work.render("skip", &context)?)?
"#,
    );
    assert_eq!(
        support::run_file_stdout(&source),
        "True\nTrue\ntrue ok\nTrue\nTrue\nTrue\nTrue\ntrue\ntrue\nTrue\ntrue"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn ast_registration_allocation_failures_preserve_node_totals_and_registry() {
    let source = program(
        r#"
bounds = std.template.limits()
context = Json.Object([]?)
mut failures = 0
mut successes = 0
mut correct = True
for budget in range(160):
    mut templates = std.template.engine(&bounds)?
    marker = f"__test_fail_allocations_after_{budget}__"?
    print(marker)?
    result = templates.add("page", "{{ ('a long string needing heap storage' == 'a long string needing heap storage') or false }}")
    print("__test_restore_allocations__")?
    match result:
        case Err(TemplateError.Allocation(AllocError.OutOfMemory)):
            failures += 1
            templates.add("page", "{{ true }}")?
        case Err(TemplateError.Bytes(BytesError.Allocation(AllocError.OutOfMemory))):
            failures += 1
            templates.add("page", "{{ true }}")?
        case Ok(_):
            successes += 1
        case _:
            correct = False
    if templates.render("page", &context)? != "true":
        correct = False
print(failures > 0, successes > 0, correct)?
"#,
    );
    assert_eq!(output(&source, false), "True True True");
}
