//! Declared shapes check every reachable template path without sample data.
fn check(body: &str) {
    let source = format!(
        "import std.template as t\nimport std.test\nimport std.bytes\nfrom std.template import TemplateError, Shape, Field, Presence\nfrom std.json import Json, Member\ndef main() -> Result[(), AllocError | IoError | TemplateError | std.test.TestError]:\n{}    print(\"ok\")?\n    Ok(())\n",
        body.lines()
            .map(|line| format!("    {line}\n"))
            .collect::<String>()
    );
    for optimize in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.plenty");
        let executable = dir.path().join("program");
        std::fs::write(&path, &source).unwrap();
        plenty::compile_file_to_executable_with_options(
            &path,
            &executable,
            None,
            &plenty::CompileOptions {
                optimize,
                test_hooks: true,
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
        let output = std::process::Command::new(executable).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"ok\n");
    }
}

#[test]
fn declared_shapes_catch_hidden_paths_and_filter_types_with_source_locations() {
    check(
        r#"
bounds = t.limits()
mut e = t.engine(&bounds)?
shape = Shape.Object([Field("user", Shape.Object([Field("display_name", Shape.Text, Presence.Required)]?), Presence.Required)]?)
e.add("bad", "é{% if false %}{{ user.dispay_name }}{% endif %}")?
match e.validate("bad", &shape):
    case Err(TemplateError.SchemaPath(name, at, path)):
        std.test.check(name == "bad" and at == 16 and path == "user.dispay_name", "full path and original byte offset")?
    case _:
        std.test.check(False, "hidden typo accepted")?
e.add("filter", "{% if false %}{{ user | upper }}{% endif %}")?
match e.validate("filter", &shape):
    case Err(TemplateError.SchemaType(_, _)):
        pass
    case _:
        std.test.check(False, "object filter accepted")?
e.add("plain", "{{ user.display_name | upper | length }}")?
e.validate("plain", &shape)?
values = Json.Object([Member("user", Json.Object([Member("display_name", Json.Text("Ada"))]?))]?)
std.test.check(e.render_checked("plain", &values, &shape)? == "3", "checked scalar filters")?
std.test.check(e.render("bad", &values)? == "é", "dynamic rendering unchanged")?
"#,
    );
}

#[test]
fn declared_shapes_distinguish_nullable_required_fields_from_optional_fields() {
    check(
        r#"
bounds = t.limits()
mut e = t.engine(&bounds)?
shape = Shape.Object([Field("name", Shape.OneOf([Shape.Null, Shape.Text]?), Presence.Required), Field("note", Shape.Text, Presence.Optional)]?)
e.add("good", "{{ name }}")?
e.validate("good", &shape)?
std.test.check(e.render_checked("good", &Json.Object([Member("name", Json.Null)]?), &shape)? == "", "required null supported")?
std.test.check(e.render_checked("good", &Json.Object([Member("name", Json.Text("<&")), Member("extra", Json.Bool(True))]?), &shape)? == "&lt;&amp;", "extra fields allowed")?
e.add("optional", "{% if note %}{{ note }}{% endif %}")?
match e.validate("optional", &shape):
    case Err(TemplateError.SchemaOptional(_, _, path)):
        std.test.check(path == "note", "optional read is explicit")?
    case _:
        std.test.check(False, "optional missing read accepted")?
for values in [Json.Object([]?), Json.Object([Member("name", Json.Int(1))]?), Json.Object([Member("name", Json.Null), Member("note", Json.Int(1))]?)]?:
    match e.render_checked("good", &values, &shape):
        case Err(TemplateError.ContextShape(_)):
            pass
        case _:
            std.test.check(False, "bad request shape accepted")?
"#,
    );
}

#[test]
fn declared_shapes_check_loop_elements_empty_arms_union_alternatives_and_indices() {
    check(
        r#"
bounds = t.limits()
mut e = t.engine(&bounds)?
shape = Shape.Object([Field("rows", Shape.Array(Box(Shape.OneOf([Shape.Object([Field("name", Shape.Text, Presence.Required)]?), Shape.Object([Field("name", Shape.Text, Presence.Required), Field("other", Shape.Int, Presence.Optional)]?)]?))?, 0), Presence.Required), Field("item", Shape.Text, Presence.Required)]?)
e.add("loop", "{% for item in rows %}{{ loop.index }}:{{ item.name }}{% else %}{{ item }}{% endfor %}")?
e.validate("loop", &shape)?
empty = Json.Object([Member("rows", Json.Array([]?)), Member("item", Json.Text("empty"))]?)
std.test.check(e.render_checked("loop", &empty, &shape)? == "empty", "empty arm restores root binding")?
e.add("bad-loop", "{% for item in rows %}{{ item.typo }}{% endfor %}")?
std.test.check(e.validate("bad-loop", &shape).is_err(), "empty arrays do not hide element typos")?
e.add("bad-index", "{{ rows.0.name }}")?
std.test.check(e.validate("bad-index", &shape).is_err(), "unguaranteed index rejected")?
fixed = Shape.Object([Field("rows", Shape.Array(Box(Shape.Object([Field("name", Shape.Text, Presence.Required)]?))?, 1), Presence.Required)]?)
e.validate("bad-index", &fixed)?
match e.render_checked("bad-index", &Json.Object([Member("rows", Json.Array([]?))]?), &fixed):
    case Err(TemplateError.ContextShape(path)):
        std.test.check(path == "$.rows", "minimum mismatch path")?
    case _:
        std.test.check(False, "minimum length ignored")?
variants = Shape.OneOf([Shape.Object([Field("a", Shape.Text, Presence.Required)]?), Shape.Object([Field("b", Shape.Text, Presence.Required)]?)]?)
e.add("variant", "{{ a }}")?
std.test.check(e.validate("variant", &variants).is_err(), "all union alternatives must support the path")?
"#,
    );
}

#[test]
fn declared_shapes_follow_effective_inheritance_and_isolate_include_bindings() {
    check(
        r#"
bounds = t.limits()
mut e = t.engine(&bounds)?
shape = Shape.Object([Field("name", Shape.Text, Presence.Required), Field("card", Shape.Object([Field("name", Shape.Text, Presence.Required)]?), Presence.Required), Field("rows", Shape.Array(Box(Shape.Text)?, 0), Presence.Required)]?)
e.add("base", "{% block body %}{{ obsolete }}{% endblock %}")?
e.add("child", "{% extends 'base' %}{% block body %}{% for name in rows %}{% include 'card' with card %}{% else %}{% include 'card' %}{% endfor %}{% endblock %}{% block unused %}{{ ignored }}{% endblock %}")?
e.add("card", "{{ name }}")?
e.validate("child", &shape)?
values = Json.Object([Member("name", Json.Text("root")), Member("card", Json.Object([Member("name", Json.Text("card"))]?)), Member("rows", Json.Array([Json.Text("local")]?))]?)
std.test.check(e.render_checked("child", &values, &shape)? == "card", "include explicit object root")?
e.add("locals", "{{ loop.index }}")?
e.add("hidden", "{% for x in rows %}{% include 'locals' %}{% endfor %}")?
std.test.check(e.validate("hidden", &shape).is_err(), "include cannot see loop metadata")?
e.add("cycle", "{% if false %}{% include 'cycle' %}{% endif %}")?
match e.validate("cycle", &shape):
    case Err(TemplateError.InheritanceCycle):
        pass
    case _:
        std.test.check(False, "hidden include cycle accepted")?
e.add("unknown", "{% if false %}{% include 'missing' %}{% endif %}")?
match e.validate("unknown", &shape):
    case Err(TemplateError.UnknownTemplate(_)):
        pass
    case _:
        std.test.check(False, "missing hidden include accepted")?
"#,
    );
}

#[test]
fn declared_shapes_bound_schema_work_and_preserve_context_validation_failures() {
    check(
        r#"
mut bounds = t.limits()
mut e = t.engine(&bounds)?
e.add("p", "ok")?
for bad in [Shape.Text, Shape.OneOf([]?), Shape.Object([Field("x", Shape.Int, Presence.Required), Field("x", Shape.Text, Presence.Optional)]?)]?:
    match e.validate("p", &bad):
        case Err(TemplateError.InvalidShape(_)):
            pass
        case _:
            std.test.check(False, "invalid declaration accepted")?
shape = Shape.Object([]?)
for bad in [Json.Object([Member("x", Json.Int(1)), Member("x", Json.Int(2))]?), Json.Object([Member("x", Json.Float(f64.INF))]?)]?:
    match e.render_checked("p", &bad, &shape):
        case Err(TemplateError.TypeMismatch):
            pass
        case _:
            std.test.check(False, "invalid JSON accepted via extra fields")?
bounds.max_steps = 0
mut zero = t.engine(&bounds)?
zero.add("p", "ok")?
match zero.validate("p", &shape):
    case Err(TemplateError.LimitExceeded):
        pass
    case _:
        std.test.check(False, "zero work budget ignored")?
bounds.max_steps = 1000000
bounds.max_context_nodes = 1
mut one = t.engine(&bounds)?
one.add("p", "ok")?
one.validate("p", &shape)?
large = Shape.Object([Field("x", Shape.Int, Presence.Required)]?)
std.test.check(one.validate("p", &large).is_err(), "schema node budget")?
bounds.max_context_nodes = 65536
bounds.max_context_bytes = 1
mut bytes = t.engine(&bounds)?
bytes.add("p", "ok")?
std.test.check(bytes.validate("p", &Shape.Object([Field("xx", Shape.Int, Presence.Required)]?)).is_err(), "schema name byte budget")?
bounds.max_context_bytes = 16777216
bounds.max_depth = 1
mut shallow = t.engine(&bounds)?
shallow.add("p", "ok")?
std.test.check(shallow.validate("p", &Shape.Object([Field("x", Shape.Array(Box(Shape.Int)?, 0), Presence.Required)]?)).is_err(), "schema depth budget")?
"#,
    );
}

#[test]
fn checked_render_shares_preflight_budget_and_accepts_common_variant_fields() {
    check(
        r#"
shape = Shape.OneOf([Shape.Object([Field("value", Shape.Int, Presence.Required)]?), Shape.Object([Field("value", Shape.Text, Presence.Required)]?)]?)
mut minimum = 0
for budget in range(256):
    mut bounds = t.limits()
    bounds.max_steps = NonNegative.try_from(budget).unwrap()
    mut candidate = t.engine(&bounds)?
    candidate.add("p", "{{ value }}")?
    if candidate.validate("p", &shape).is_ok():
        minimum = budget
        break
std.test.check(minimum > 0, "bounded preflight eventually succeeds")?
mut bounds = t.limits()
bounds.max_steps = NonNegative.try_from(minimum).unwrap()
mut exact = t.engine(&bounds)?
exact.add("p", "{{ value }}")?
exact.validate("p", &shape)?
match exact.render_checked("p", &Json.Object([Member("value", Json.Int(1))]?), &shape):
    case Err(TemplateError.LimitExceeded):
        pass
    case _:
        std.test.check(False, "checked rendering reset the preflight budget")?
bounds.max_steps = 1000000
mut e = t.engine(&bounds)?
e.add("p", "{{ value }}")?
std.test.check(e.render_checked("p", &Json.Object([Member("value", Json.Int(42))]?), &shape)? == "42", "first object variant")?
std.test.check(e.render_checked("p", &Json.Object([Member("value", Json.Text("<&"))]?), &shape, html=False)? == "<&", "second object variant and text mode")?
nullable_object = Shape.Object([Field("user", Shape.OneOf([Shape.Null, Shape.Object([Field("name", Shape.Text, Presence.Required)]?)]?), Presence.Required)]?)
e.add("guarded", "{% if user %}{{ user.name }}{% endif %}")?
std.test.check(e.validate("guarded", &nullable_object).is_err(), "nullable object guard has no flow refinement")?
"#,
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn schema_and_checked_render_allocation_failures_propagate_and_allow_reuse() {
    check(
        r#"
bounds = t.limits()
mut e = t.engine(&bounds)?
e.add("base", "before{% block body %}old{% endblock %}after")?
e.add("child", "{% extends 'base' %}{% block body %}{% include 'part' %}{% endblock %}")?
e.add("part", "{{ value }}{{ (value == value) and true }}")?
shape = Shape.OneOf([Shape.Object([Field("value", Shape.Int, Presence.Required)]?), Shape.Object([Field("value", Shape.Text, Presence.Required)]?)]?)
context = Json.Object([Member("value", Json.Text("<A long & meaningful value requiring heap storage>"))]?)
expected = "before&lt;A long &amp; meaningful value requiring heap storage&gt;trueafter"
mut validation_failed = False
mut validation_succeeded = False
for budget in range(1024):
    std.test.fail_allocations_after(u32(budget))?
    result = e.validate("child", &shape)
    std.test.restore_allocations()?
    match result:
        case Err(TemplateError.Allocation(AllocError.OutOfMemory)):
            validation_failed = True
        case Ok(_):
            validation_succeeded = True
        case _:
            std.test.check(False, "validation swallowed or changed allocation failure")?
    e.validate("child", &shape)?
    if validation_succeeded:
        break
std.test.check(validation_failed and validation_succeeded, "validation failure sweep reaches success")?
mut render_failed = False
mut render_succeeded = False
for budget in range(1024):
    std.test.fail_allocations_after(u32(budget))?
    result = e.render_checked("child", &context, &shape)
    std.test.restore_allocations()?
    match result:
        case Err(TemplateError.Allocation(AllocError.OutOfMemory)):
            render_failed = True
        case Err(TemplateError.Bytes(std.bytes.BytesError.Allocation(AllocError.OutOfMemory))):
            render_failed = True
        case Ok(output):
            std.test.check(output == expected, "successful injected render output")?
            render_succeeded = True
        case _:
            std.test.check(False, "union matching swallowed or changed allocation failure")?
    std.test.check(e.render_checked("child", &context, &shape)? == expected, "same engine and context recover")?
    if render_succeeded:
        break
std.test.check(render_failed and render_succeeded, "checked-render failure sweep reaches success")?
"#,
    );
}

#[test]
fn declared_shapes_visit_elif_and_short_circuited_operands_and_infer_operator_filters() {
    check(
        r#"
bounds = t.limits()
mut e = t.engine(&bounds)?
shape = Shape.Object([Field("name", Shape.Text, Presence.Required), Field("items", Shape.Array(Box(Shape.Int)?, 0), Presence.Required)]?)
for source in ["{% if true %}ok{% elif typo %}bad{% endif %}", "{{ true or typo }}", "{{ false and typo }}", "{% if true %}ok{% elif false %}{{ typo }}{% else %}{{ name }}{% endif %}"]?:
    mut item = t.engine(&bounds)?
    item.add("bad", source)?
    match item.validate("bad", &shape):
        case Err(TemplateError.SchemaPath(_, _, path)):
            std.test.check(path == "typo", "all operand and branch paths checked")?
        case _:
            std.test.check(False, "hidden expression typo accepted")?
for source in ["{{ true or (42 | upper) }}", "{{ false and (items == null) }}", "{{ (name == 'Ada') | upper }}", "{% if true %}ok{% elif items != items %}bad{% endif %}"]?:
    mut item = t.engine(&bounds)?
    item.add("bad", source)?
    match item.validate("bad", &shape):
        case Err(TemplateError.SchemaType(_, _)):
            pass
        case _:
            std.test.check(False, "hidden expression type error accepted")?
e.add("good", "{{ ((name | upper) == 'ADA' and not false) or (42 != 42.0) }}|{{ (name) | length }}|{% if false %}x{% elif name == 'Ada' %}chosen{% else %}y{% endif %}")?
e.validate("good", &shape)?
values = Json.Object([Member("name", Json.Text("Ada")), Member("items", Json.Array([]?))]?)
std.test.check(e.render_checked("good", &values, &shape)? == "true|3|chosen", "postorder operators and parenthesized filters")?
"#,
    );
}

#[test]
fn flat_elif_preflight_uses_one_depth_frame() {
    let mut template = String::from("{% if false %}a");
    for _ in 0..100 {
        template.push_str("{% elif false %}b");
    }
    template.push_str("{% else %}ok{% endif %}");
    check(&format!(
        r#"
mut bounds = t.limits()
bounds.max_depth = 1
mut e = t.engine(&bounds)?
e.add("flat", "{template}")?
shape = Shape.Object([]?)
e.validate("flat", &shape)?
std.test.check(e.render_checked("flat", &Json.Object([]?), &shape)? == "ok", "flat elif chain depth")?
"#
    ));
}
