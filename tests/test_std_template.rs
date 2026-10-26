//! Bounded pure-Plenty templates: grammar, escaping, inheritance and failures.
mod support;

fn program(body: &str) -> String {
    format!(
        "import std.template\nfrom std.template import TemplateError\nfrom std.json import Json, Member\ndef main() -> Result[(), AllocError | IoError | TemplateError]:\n{}    Ok(())\n",
        body.lines()
            .map(|line| format!("    {line}\n"))
            .collect::<String>()
    )
}

#[test]
fn values_filters_escaping_and_plain_text() {
    let source = program(
        r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
templates.add("page", "{{ name }}|{{ name | safe }}|{{ name | escape }}|{{ word | trim | upper }}|{{ word | lower }}|{{ name | length }}|{{ number }}|{{ true }}|{{ null }}|{{ '}}|ok' }}")?
context = Json.Object([Member("name", Json.Text("<a&\"'>")), Member("word", Json.Text(" AbC ")), Member("number", Json.Int(-42))]?)
print(templates.render("page", &context)?)?
templates.add("plain", "{{ name }}")?
print(templates.render("plain", &context, html=False)?)?
templates.add("unicode", "🙂 {{ person.name }} / {{ items.1 }}")?
other = Json.Object([Member("person", Json.Object([Member("name", Json.Text("Zoë"))]?)), Member("items", Json.Array([Json.Text("zero"), Json.Text("一") ]?))]?)
print(templates.render("unicode", &other)?)?
"#,
    );
    assert_eq!(
        support::run_file_stdout(&source),
        "&lt;a&amp;&quot;&#x27;&gt;|<a&\"'>|&lt;a&amp;&quot;&#x27;&gt;|ABC| abc |6|-42|true||}}|ok\n<a&\"'>\n🙂 Zoë / 一"
    );
}

#[test]
fn conditions_nested_loops_and_empty_else() {
    let source = program(
        r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
templates.add("page", "{% if active %}yes{% else %}no{% endif %}/{% if not active %}off{% endif %}/{% for row in rows %}[{{ loop.index }}:{{ loop.first }}:{{ loop.last }}:{{ loop.length }}{% for x in row %} {{ loop.index0 }}={{ x }}{% else %} empty{% endfor %} outer={{ loop.index }}]{% endfor %}")?
context = Json.Object([Member("active", Json.Bool(False)), Member("rows", Json.Array([Json.Array([Json.Int(10), Json.Int(20)]?), Json.Array([]?)]?))]?)
print(templates.render("page", &context)?)?
"#,
    );
    assert_eq!(
        support::run_file_stdout(&source),
        "no/off/[1:true:false:2 0=10 1=20 outer=1][2:false:true:2 empty outer=2]"
    );
}

#[test]
fn inheritance_is_named_and_most_derived_wins() {
    let source = program(
        r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
templates.add("base", "<h1>{% block title %}base{% endblock %}</h1>{% block body %}body{% endblock %}")?
templates.add("middle", "{% extends 'base' %}{% block title %}middle{% endblock %}")?
templates.add("child", "{# comment #}{% extends 'middle' %}{% block title %}{{ name }}{% endblock %}{% block body %}{% if true %}child{% endif %}{% endblock %}")?
context = Json.Object([Member("name", Json.Text("<Ada>"))]?)
print(templates.render("base", &context)?)?
print(templates.render("middle", &context)?)?
print(templates.render("child", &context)?)?
print(templates.render("child", &context)?)?
"#,
    );
    assert_eq!(
        support::run_file_stdout(&source),
        "<h1>base</h1>body\n<h1>middle</h1>body\n<h1>&lt;Ada&gt;</h1>child\n<h1>&lt;Ada&gt;</h1>child"
    );
}

#[test]
fn syntax_is_checked_in_unexecuted_branches_and_registration_is_atomic() {
    let source = program(
        r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
bad = ["{{", "{% if true %}", "{% else %}", "{% if true %}{% endfor %}", "{% for x in items %}{% endif %}", "{% block x %}{% endfor %}", "{% if true %}{% else %}{% else %}{% endif %}", "{% if false %}{{ x | unknown }}{% endif %}", "{{ foo() }}", "{% for loop in items %}{% endfor %}", "{% extends 'base' %}unexpected", "{% block x %}{% block y %}{% endblock %}{% endblock %}", "{% block x %}{% endblock %}{% block x %}{% endblock %}", "{{ 'bad\\q' }}"]?
for text in &bad:
    match templates.add("reusable", text):
        case Err(TemplateError.Syntax(_)):
            print("syntax")?
        case _:
            print("wrong")?
templates.add("reusable", "ok")?
context = Json.Object([]?)
print(templates.render("reusable", &context)?)?
match templates.add("reusable", "replacement"):
    case Err(TemplateError.DuplicateTemplate(_)):
        print("duplicate")?
    case _:
        print("wrong")?
print(templates.render("reusable", &context)?)?
"#,
    );
    assert_eq!(
        support::run_file_stdout(&source),
        format!("{}\nok\nduplicate\nok", vec!["syntax"; 14].join("\n"))
    );
}

#[test]
fn missing_values_types_and_inheritance_fail_explicitly() {
    let source = program(
        r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
templates.add("missing", "{{ unknown }}")?
templates.add("type", "{{ items }}")?
templates.add("cycle-a", "{% extends 'cycle-b' %}")?
templates.add("cycle-b", "{% extends 'cycle-a' %}")?
templates.add("parent", "{% extends 'absent' %}")?
context = Json.Object([Member("items", Json.Array([]?))]?)
for name in ["missing", "type", "cycle-a", "parent", "absent"]?:
    match templates.render(&name, &context):
        case Err(error):
            print(error)?
        case Ok(_):
            print("wrong")?
duplicate = Json.Object([Member("x", Json.Int(1)), Member("x", Json.Int(2))]?)
match templates.render("missing", &duplicate):
    case Err(TemplateError.TypeMismatch):
        print("duplicate context")?
    case _:
        print("wrong")?
"#,
    );
    assert_eq!(
        support::run_file_stdout(&source),
        "std.template.TemplateError.MissingValue(\"unknown\")\nstd.template.TemplateError.TypeMismatch\nstd.template.TemplateError.InheritanceCycle\nstd.template.TemplateError.UnknownTemplate(\"absent\")\nstd.template.TemplateError.UnknownTemplate(\"absent\")\nduplicate context"
    );
}

#[test]
fn output_work_context_and_source_limits() {
    let source = program(
        r#"
mut bounds = std.template.limits()
bounds.max_output = 4
mut templates = std.template.engine(&bounds)?
templates.add("fit", "éé")?
templates.add("large", "ééx")?
templates.add("escaped", "{{ name }}")?
context = Json.Object([Member("name", Json.Text("&"))]?)
print(templates.render("fit", &context)?)?
for name in ["large", "escaped"]?:
    match templates.render(&name, &context):
        case Err(TemplateError.LimitExceeded):
            print("output limit")?
        case _:
            print("wrong")?
bounds.max_steps = 10
mut slow = std.template.engine(&bounds)?
slow.add("loop", "{% for x in xs %}{% for y in xs %}{% endfor %}{% endfor %}")?
many = Json.Object([Member("xs", Json.Array([Json.Int(1), Json.Int(2), Json.Int(3)]?))]?)
match slow.render("loop", &many):
    case Err(TemplateError.LimitExceeded):
        print("work limit")?
    case _:
        print("wrong")?
bounds.max_steps = 1000
bounds.max_context_bytes = 4
mut small = std.template.engine(&bounds)?
small.add("ok", "ok")?
match small.render("ok", &context):
    case Err(TemplateError.LimitExceeded):
        print("context limit")?
    case _:
        print("wrong")?
bounds.max_source = 3
mut short = std.template.engine(&bounds)?
match short.add("ok", "xx"):
    case Err(TemplateError.LimitExceeded):
        print("source limit")?
    case _:
        print("wrong")?
bounds.max_depth = 65
match std.template.engine(&bounds):
    case Err(TemplateError.InvalidLimits):
        print("invalid limit")?
    case _:
        print("wrong")?
"#,
    );
    assert_eq!(
        support::run_file_stdout(&source),
        "éé\noutput limit\noutput limit\nwork limit\ncontext limit\nsource limit\ninvalid limit"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_leaves_registration_and_rendering_reusable() {
    let mut body = String::from(
        r#"
bounds = std.template.limits()
context = Json.Object([Member("name", Json.Text("<a long value needing heap allocation>")), Member("items", Json.Array([Json.Int(1), Json.Int(2)]?))]?)
mut ready = std.template.engine(&bounds)?
ready.add("base", "<p>{% block body %}base{% endblock %}</p>")?
ready.add("child", "{% extends 'base' %}{% block body %}{{ name | upper }}{% for x in items %}{{ x }}{% endfor %}{% endblock %}")?
"#,
    );
    let mut helpers = String::new();
    for budget in 0..100 {
        body.push_str(&format!("check{budget}(&ready, &context, &bounds)?\n"));
        let check = format!(
            r#"
mut registry{budget} = std.template.engine(bounds)?
print("__test_fail_allocations_after_{budget}__")?
registered{budget} = registry{budget}.add("test", "Before {{{{ name | upper }}}}! {{{{ items.0 }}}}")
print("__test_restore_allocations__")?
match registered{budget}:
    case Err(_):
        registry{budget}.add("test", "Before {{{{ name | upper }}}}! {{{{ items.0 }}}}")?
    case Ok(_):
        pass
drop(registry{budget}.render("test", context)?)
print("__test_fail_allocations_after_{budget}__")?
rendered{budget} = ready.render("child", context)
print("__test_restore_allocations__")?
match rendered{budget}:
    case Err(error):
        print(error)?
    case Ok(text):
        print(text)?
drop(ready.render("child", context)?)
Ok(())
"#
        );
        helpers.push_str(&format!(
            "\ndef check{budget}(ready: &std.template.Engine, context: &Json, bounds: &std.template.Limits) -> Result[(), TemplateError | IoError]:\n{}",
            check.lines().map(|line| format!("    {line}\n")).collect::<String>()
        ));
    }
    let output = support::run_file_stdout(&format!("{}\n{helpers}", program(&body)));
    assert_eq!(output.lines().count(), 100);
    assert!(output.contains("OutOfMemory"));
    assert!(output.contains("<p>&lt;A LONG VALUE NEEDING HEAP ALLOCATION&gt;12</p>"));
    for line in output.lines() {
        assert!(
            line.contains("OutOfMemory")
                || line == "<p>&lt;A LONG VALUE NEEDING HEAP ALLOCATION&gt;12</p>",
            "{line}"
        );
    }
}

#[test]
fn unicode_scanning_and_independent_resource_limits() {
    let literal = "é🙂".repeat(10_000);
    let template = format!("é{{{{ '{literal}' | length }}}}");
    let source = program(&format!(
        r#"
mut bounds = std.template.limits()
mut large = std.template.engine(&bounds)?
large.add("unicode", {template:?})?
empty = Json.Object([]?)
print(large.render("unicode", &empty)?)?
bounds.max_tokens = 1
mut tokens = std.template.engine(&bounds)?
match tokens.add("bad", "{{{{ true }}}}{{{{ false }}}}"):
    case Err(TemplateError.LimitExceeded):
        print("tokens")?
    case _:
        print("wrong")?
bounds.max_tokens = 100
bounds.max_depth = 2
mut depth = std.template.engine(&bounds)?
match depth.add("bad", "{{% if true %}}{{% if true %}}{{% if true %}}{{% endif %}}{{% endif %}}{{% endif %}}"):
    case Err(TemplateError.LimitExceeded):
        print("depth")?
    case _:
        print("wrong")?
bounds.max_context_nodes = 1
mut nodes = std.template.engine(&bounds)?
nodes.add("ok", "ok")?
context = Json.Object([Member("one", Json.Int(1))]?)
match nodes.render("ok", &context):
    case Err(TemplateError.LimitExceeded):
        print("nodes")?
    case _:
        print("wrong")?
bounds.max_templates = 0
mut count = std.template.engine(&bounds)?
match count.add("a", ""):
    case Err(TemplateError.LimitExceeded):
        print("templates")?
    case _:
        print("wrong")?
"#
    ));
    assert_eq!(
        support::run_file_stdout(&source),
        "é20000\ntokens\ndepth\nnodes\ntemplates"
    );
}
