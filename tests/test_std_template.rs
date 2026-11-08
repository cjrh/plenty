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
fn includes_compose_registered_fragments_with_isolated_object_contexts() {
    let source = program(
        r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
templates.add("header", "<h1>{{ title }}</h1>")?
templates.add("form", "<form><button>{{ label }}</button>{% for part in parts %}{{ loop.index }}={{ part }};{% endfor %}</form>")?
templates.add("page", "{% include 'header' %}{% for row in rows %}[{{ loop.index }}:{% include 'form' with row %}:{{ loop.index }}]{% endfor %}:{{ title }}")?
context = Json.Object([Member("title", Json.Text("A & B")), Member("rows", Json.Array([Json.Object([Member("label", Json.Text("<edit>")), Member("parts", Json.Array([Json.Text("one"), Json.Text("two")]?))]?), Json.Object([Member("label", Json.Text("save")), Member("parts", Json.Array([]?))]?)]?))]?)
print(templates.render("page", &context)?)?
print(templates.render("page", &context, html=False)?)?
templates.add("needs_loop", "{{ loop.index }}")?
templates.add("isolated", "{% for row in rows %}{% include 'needs_loop' with row %}{% endfor %}")?
match templates.render("isolated", &context):
    case Err(TemplateError.MissingValue(_)):
        print("no caller loop")?
    case _:
        print("wrong")?
print(templates.render("header", &context)?)?
"#,
    );
    let expected = "<h1>A &amp; B</h1>[1:<form><button>&lt;edit&gt;</button>1=one;2=two;</form>:1][2:<form><button>save</button></form>:2]:A &amp; B\n<h1>A & B</h1>[1:<form><button><edit></button>1=one;2=two;</form>:1][2:<form><button>save</button></form>:2]:A & B\nno caller loop\n<h1>A &amp; B</h1>";
    for optimize in [false, true] {
        assert_eq!(output_with_optimization(&source, optimize), expected);
    }
}

#[test]
fn included_inheritance_has_its_own_overrides_and_detects_real_cycles() {
    let source = program(
        r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
templates.add("base", "<{% block body %}base{% endblock %}>")?
templates.add("card", "{% extends 'base' %}{% block body %}{{ title }}{% endblock %}")?
templates.add("page", "{% extends 'base' %}{% block body %}page:{% include 'card' %}:{% include 'base' %}{% endblock %}")?
context = Json.Object([Member("title", Json.Text("<&>"))]?)
print(templates.render("page", &context)?)?
templates.add("a", "{% include 'b' %}")?
templates.add("b", "{% include 'a' %}")?
templates.add("self", "{% include 'self' %}")?
templates.add("derived", "{% extends 'base' %}{% block body %}{% include 'derived' %}{% endblock %}")?
for name in ["a", "self", "derived"]?:
    match templates.render(&name, &context):
        case Err(TemplateError.InheritanceCycle):
            print("cycle")?
        case _:
            print("wrong")?
print(templates.render("page", &context)?)?
"#,
    );
    assert_eq!(
        support::run_file_stdout(&source),
        "<page:<&lt;&amp;&gt;>:<base>>\ncycle\ncycle\ncycle\n<page:<&lt;&amp;&gt;>:<base>>"
    );
}

#[test]
fn includes_preserve_syntax_and_shared_resource_limits() {
    let source = program(
        r#"
mut bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
for bad in ["{% include name %}", "{% include '' %}", "{% include 'card' extra %}", "{% include 'card' with %}", "{% if false %}{% include 'card' with foo() %}{% endif %}"]?:
    match templates.add("retry", &bad):
        case Err(TemplateError.Syntax(_)):
            print("syntax")?
        case _:
            print("wrong")?
templates.add("retry", "ok")?
templates.add("missing", "before{% include 'absent' %}")?
templates.add("wrong", "{% include 'retry' with 4 %}")?
context = Json.Object([]?)
match templates.render("missing", &context):
    case Err(TemplateError.UnknownTemplate(_)):
        print("missing")?
    case _:
        print("wrong")?
match templates.render("wrong", &context):
    case Err(TemplateError.TypeMismatch):
        print("object required")?
    case _:
        print("wrong")?
bounds.max_output = 3
mut short = std.template.engine(&bounds)?
short.add("x", "xx")?
short.add("large", "{% include 'x' %}{% include 'x' %}")?
match short.render("large", &context):
    case Err(TemplateError.LimitExceeded):
        print("output limit")?
    case _:
        print("wrong")?
print(short.render("x", &context)?)?
bounds.max_output = 100
bounds.max_depth = 1
mut shallow = std.template.engine(&bounds)?
shallow.add("x", "x")?
shallow.add("outer", "{% include 'x' %}")?
match shallow.render("outer", &context):
    case Err(TemplateError.LimitExceeded):
        print("depth limit")?
    case _:
        print("wrong")?
print(shallow.render("x", &context)?)?
bounds.max_depth = 64
bounds.max_steps = 8
mut slow = std.template.engine(&bounds)?
slow.add("x", "x")?
slow.add("outer", "{% include 'x' %}{% include 'x' %}")?
match slow.render("outer", &context):
    case Err(TemplateError.LimitExceeded):
        print("step limit")?
    case _:
        print("wrong")?
print(slow.render("x", &context)?)?
"#,
    );
    assert_eq!(
        support::run_file_stdout(&source),
        "syntax\nsyntax\nsyntax\nsyntax\nsyntax\nmissing\nobject required\noutput limit\nxx\ndepth limit\nx\nstep limit\nx"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn include_allocation_failures_leave_the_engine_and_context_reusable() {
    let source = format!(
        "from std.bytes import BytesError\n{}",
        program(
            r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
templates.add("part", "<p>{{ title }}</p>")?
templates.add("page", "{% include 'part' with row %}")?
context = Json.Object([Member("row", Json.Object([Member("title", Json.Text("A & B long enough to require storage"))]?))]?)
for budget in [0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 85, 90, 95, 128, 256]?:
    marker = f"__test_fail_allocations_after_{budget}__"?
    print(marker)?
    result = templates.render("page", &context)
    print("__test_restore_allocations__")?
    match result:
        case Err(TemplateError.Allocation(_)):
            print("allocation")?
        case Err(TemplateError.Bytes(BytesError.Allocation(_))):
            print("allocation")?
        case Ok(value):
            print(value == "<p>A &amp; B long enough to require storage</p>")?
        case _:
            print("wrong")?
    print(templates.render("page", &context)? == "<p>A &amp; B long enough to require storage</p>")?
"#
        )
    );
    let output = support::run_file_stdout(&source);
    let lines: Vec<_> = output.lines().collect();
    assert_eq!(lines.len(), 44, "{output}");
    assert!(lines.contains(&"allocation"), "{output}");
    assert!(
        lines
            .iter()
            .all(|line| ["allocation", "True"].contains(line)),
        "{output}"
    );
    assert!(
        lines.iter().skip(1).step_by(2).all(|line| *line == "True"),
        "{output}"
    );
    assert_eq!(&lines[42..], &["True", "True"], "{output}");
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
"#,
    );
    assert_eq!(
        support::run_file_stdout(&source),
        "éé\noutput limit\noutput limit\nwork limit\ncontext limit\nsource limit"
    );
}

#[test]
fn a_depth_limit_above_the_ceiling_traps() {
    support::assert_file_traps(
        &program(
            "mut bounds = std.template.limits()\nbounds.max_depth = Positive.try_from(65).unwrap()\ndrop(std.template.engine(&bounds))",
        ),
        "std.template.engine: max_depth is above 64",
    );
}

#[test]
fn out_of_domain_limits_are_compile_errors() {
    // `max_depth` and `max_context_nodes` are `Positive[i64]`; the other
    // fields are `NonNegative[i64]`, replacing the old `InvalidLimits` cases.
    for (assignment, message) in [
        ("bounds.max_depth = 0", "must be greater than zero"),
        ("bounds.max_context_nodes = 0", "must be greater than zero"),
        ("bounds.max_steps = -1", "cannot be negative"),
    ] {
        let source = program(&format!("mut bounds = std.template.limits()\n{assignment}"));
        let error = support::check_file_error(&source);
        assert!(error.contains(message), "{error}");
    }
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

fn output_with_optimization(source: &str, optimize: bool) -> String {
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
fn exact_budgets_and_delimiter_endpoints_at_i64_limits() {
    let library = include_str!("../stdlib/std/template.plenty");
    let helpers = ["has_pair", "bounded_total"]
        .map(|name| {
            library[library.find(&format!("def {name}(")).unwrap()..]
                .split("\n\n")
                .next()
                .unwrap()
        })
        .join("\n\n");
    let source = format!(
        r#"from std.template import TemplateError
{helpers}

def main() -> Result[(), IoError]:
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    last = bounded_total(9223372036854775806, 1, i64.MAX).unwrap()
    full = bounded_total(i64.MAX, 0, i64.MAX).unwrap()
    overflow = bounded_total(i64.MAX, 1, i64.MAX)
    limit = bounded_total(8, 3, 10)
    pair = has_pair(9223372036854775805, i64.MAX)
    single = has_pair(9223372036854775806, i64.MAX)
    end = has_pair(i64.MAX, i64.MAX)
    empty = has_pair(0, 0)
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    expected: Result[i64, TemplateError] = Err(TemplateError.LimitExceeded)
    print(last == i64.MAX, full == i64.MAX, overflow == expected, limit == expected)?
    print(pair, single, end, empty)?
    Ok(())
"#
    );
    for optimize in [false, true] {
        assert_eq!(
            output_with_optimization(&source, optimize),
            "True True True True\nTrue False False False"
        );
    }
}

#[test]
fn maximum_budgets_keep_registration_and_rendering_exact() {
    let source = program(
        r#"
mut bounds = std.template.limits()
bounds.max_source = NonNegative.try_from(i64.MAX).unwrap()
bounds.max_tokens = NonNegative.try_from(i64.MAX).unwrap()
bounds.max_context_bytes = NonNegative.try_from(i64.MAX).unwrap()
bounds.max_steps = NonNegative.try_from(i64.MAX).unwrap()
bounds.max_output = NonNegative.try_from(i64.MAX).unwrap()
mut templates = std.template.engine(&bounds)?
templates.add("page", "{% for x in xs %}{{ loop.index }}={{ x }}{% else %}empty{% endfor %}!")?
context = Json.Object([Member("xs", Json.Array([Json.Text("<&"), Json.Text("é")]?))]?)
print(templates.render("page", &context)?)?
match templates.add("bad", "{{"):
    case Err(TemplateError.Syntax(at)):
        print("syntax", at)?
    case _:
        print("wrong")?
templates.add("bad", "ok")?
print(templates.render("bad", &context)?)?
"#,
    );
    for optimize in [false, true] {
        assert_eq!(
            output_with_optimization(&source, optimize),
            "1=&lt;&amp;2=é!\nsyntax 0\nok"
        );
    }
}

#[test]
fn explicit_html_contexts_preserve_exact_text_and_quote_delimiters() {
    let source = program(
        r#"
text = "&<>\"'é🙂"
print(std.template.escape(&text, std.template.HtmlContext.Text)?)?
print(std.template.escape(&text, std.template.HtmlContext.DoubleQuotedAttribute)?)?
print(std.template.escape(&text, std.template.HtmlContext.SingleQuotedAttribute)?)?
print(std.template.escape(&text, std.template.HtmlContext.TextAndAttributes)?)?
# Existing character references are input text, not trusted markup.
print(std.template.escape("&amp;", std.template.HtmlContext.Text)?)?
attr = std.template.escape("\" autofocus=\"yes", std.template.HtmlContext.DoubleQuotedAttribute)?
child = std.template.escape("<b>\"hello\"</b>", std.template.HtmlContext.Text)?
print(f"<span title=\"{attr}\">{child}</span>"?)?
"#,
    );
    let expected = "&amp;&lt;&gt;\"'é🙂\n&amp;<>&quot;'é🙂\n&amp;<>\"&#x27;é🙂\n&amp;&lt;&gt;&quot;&#x27;é🙂\n&amp;amp;\n<span title=\"&quot; autofocus=&quot;yes\">&lt;b&gt;\"hello\"&lt;/b&gt;</span>";
    for optimize in [false, true] {
        assert_eq!(output_with_optimization(&source, optimize), expected);
    }
}

#[test]
fn explicit_escape_caps_count_utf8_bytes_and_entity_expansion() {
    let source = program(
        r#"
for mode, exact in [(std.template.HtmlContext.Text, 21), (std.template.HtmlContext.DoubleQuotedAttribute, 20), (std.template.HtmlContext.SingleQuotedAttribute, 20), (std.template.HtmlContext.TextAndAttributes, 31)]?:
    text = "&<>\"'é🙂"
    print(std.template.escape(&text, mode, NonNegative.try_from(exact).unwrap())?.byte_len())?
    below = unchecked:
        exact - 1
    match std.template.escape(&text, mode, NonNegative.try_from(below).unwrap()):
        case Err(TemplateError.LimitExceeded):
            print("expansion limit")?
        case _:
            print("wrong")?
    print(std.template.escape("", mode, 0)? == "")?
    print(std.template.escape("é🙂", mode, 6)?)?
    match std.template.escape("é🙂", mode, 5):
        case Err(TemplateError.LimitExceeded):
            print("input limit")?
        case _:
            print("wrong")?
print(std.template.escape("&", std.template.HtmlContext.Text, NonNegative.try_from(i64.MAX).unwrap())?)?
"#,
    );
    let expected = [21, 20, 20, 31]
        .map(|size| format!("{size}\nexpansion limit\nTrue\né🙂\ninput limit"))
        .join("\n")
        + "\n&amp;";
    for optimize in [false, true] {
        assert_eq!(output_with_optimization(&source, optimize), expected);
    }
    let error = support::check_file_error(&program(
        "drop(std.template.escape(\"x\", std.template.HtmlContext.Text, -1))",
    ));
    assert!(error.contains("cannot be negative"), "{error}");
}

#[test]
fn escaped_strings_carry_no_template_trust_and_existing_filters_keep_their_policy() {
    let source = program(
        r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
templates.add("page", "<p>{{ value }}</p><i title=\"{{ value }}\"></i>")?
templates.add("filters", "{{ value | escape }}|{{ value | escape | trim }}|{{ value | safe | upper }}|{{ value | safe | lower }}")?
raw = Json.Object([Member("value", Json.Text("<B&\"'>"))]?)
print(templates.render("page", &raw)?)?
print(templates.render("filters", &raw)?)?
# Reusing text-escaped data in an attribute still applies normal escaping,
# including the quotes that the Text policy intentionally left literal.
escaped = std.template.escape("\" onclick=\"bad&", std.template.HtmlContext.Text)?
values = Json.Object([Member("value", Json.Text(escaped))]?)
print(templates.render("page", &values)?)?
print(templates.render("page", &values, html=False)?)?
"#,
    );
    assert_eq!(
        support::run_file_stdout(&source),
        "<p>&lt;B&amp;&quot;&#x27;&gt;</p><i title=\"&lt;B&amp;&quot;&#x27;&gt;\"></i>\n&lt;B&amp;&quot;&#x27;&gt;|&lt;B&amp;&quot;&#x27;&gt;|&lt;B&amp;&quot;&#x27;&gt;|&lt;b&amp;&quot;&#x27;&gt;\n<p>&quot; onclick=&quot;bad&amp;amp;</p><i title=\"&quot; onclick=&quot;bad&amp;amp;\"></i>\n<p>\" onclick=\"bad&amp;</p><i title=\"\" onclick=\"bad&amp;\"></i>"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn escaping_reports_allocation_failures_and_rejects_oversize_before_allocating() {
    let mut body = String::from(
        r#"
text = "&<>\"'é🙂 a long value requiring heap storage &<>\"'é🙂"
# An output cap smaller than the source is rejected without allocating.
print("__test_fail_allocations_after_0__")?
rejected = std.template.escape(&text, std.template.HtmlContext.Text, 1)
print("__test_restore_allocations__")?
expected: Result[str, TemplateError] = Err(TemplateError.LimitExceeded)
print(rejected == expected)?
"#,
    );
    for budget in 0..24 {
        body.push_str(&format!(
            r#"
print("__test_fail_allocations_after_{budget}__")?
result{budget} = std.template.escape(&text, std.template.HtmlContext.DoubleQuotedAttribute)
print("__test_restore_allocations__")?
match result{budget}:
    case Err(TemplateError.Allocation(AllocError.OutOfMemory)):
        print("allocation")?
    case Err(TemplateError.Bytes(BytesError.Allocation(AllocError.OutOfMemory))):
        print("allocation")?
    case Ok(value):
        print(value == "&amp;<>&quot;'é🙂 a long value requiring heap storage &amp;<>&quot;'é🙂")?
    case _:
        print("wrong")?
"#
        ));
    }
    let source = format!("from std.bytes import BytesError\n{}", program(&body));
    let output = support::run_file_stdout(&source);
    let lines: Vec<_> = output.lines().collect();
    assert_eq!(lines.len(), 25);
    assert_eq!(lines[0], "True");
    let failures = lines[1..]
        .iter()
        .take_while(|line| **line == "allocation")
        .count();
    assert!(failures > 0 && failures < 24, "{output}");
    assert!(lines[1 + failures..].iter().all(|line| *line == "True"));
}

#[test]
fn interpolation_contexts_encode_original_values_once_in_both_render_modes() {
    let source = program(
        r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
templates.add("contexts", "<p>{{ value | escape_text }}</p><i title=\"{{ value | escape_double_quoted }}\" data-label='{{ value | escape_single_quoted }}'></i>|{{ value }}")?
context = Json.Object([Member("value", Json.Text("&<>\"'é🙂"))]?)
print(templates.render("contexts", &context)?)?
print(templates.render("contexts", &context, html=False)?)?
"#,
    );
    let contexts = "<p>&amp;&lt;&gt;\"'é🙂</p><i title=\"&amp;<>&quot;'é🙂\" data-label='&amp;<>\"&#x27;é🙂'></i>";
    let expected = format!("{contexts}|&amp;&lt;&gt;&quot;&#x27;é🙂\n{contexts}|&<>\"'é🙂");
    for optimize in [false, true] {
        assert_eq!(output_with_optimization(&source, optimize), expected);
    }
}

#[test]
fn context_filter_chains_select_emission_without_laundering_text_trust() {
    let source = program(
        r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
templates.add("chains", "{{ value | trim | escape_text }}|{{ value | escape_text | escape_double_quoted }}|{{ value | escape_double_quoted | escape_text }}|{{ value | safe | escape_single_quoted }}|{{ value | escape_text | safe }}|{{ value | escape_text | upper }}|{{ value | escape_double_quoted | lower }}|{{ value | escape_single_quoted | trim }}|{{ value | escape_text | length }}|{{ value | escape_text | escape }}|{{ value | escape | escape_text }}")?
context = Json.Object([Member("value", Json.Text(" <B&\"'> "))]?)
print(templates.render("chains", &context)?)?
# Each lookup starts from the original raw value, without another expression's policy.
templates.add("reuse", "{{ value | escape_text }}|{{ value }}|{{ value | escape_double_quoted }}")?
print(templates.render("reuse", &context)?)?
# A standalone escaper returns ordinary text, never a transferable context marker.
escaped = std.template.escape("\" onclick=\"bad&", std.template.HtmlContext.Text)?
other = Json.Object([Member("value", Json.Text(escaped))]?)
print(templates.render("reuse", &other)?)?
"#,
    );
    assert_eq!(
        support::run_file_stdout(&source),
        "&lt;B&amp;\"'&gt;| <B&amp;&quot;'> | &lt;B&amp;\"'&gt; | <B&amp;\"&#x27;> | <B&\"'> | &lt;B&amp;&quot;&#x27;&gt; | &lt;b&amp;&quot;&#x27;&gt; |<B&amp;\"&#x27;>|8| &lt;B&amp;&quot;&#x27;&gt; | &amp;lt;B&amp;amp;&amp;quot;&amp;#x27;&amp;gt; \n &lt;B&amp;\"'&gt; | &lt;B&amp;&quot;&#x27;&gt; | <B&amp;&quot;'> \n\" onclick=\"bad&amp;amp;|&quot; onclick=&quot;bad&amp;amp;|&quot; onclick=&quot;bad&amp;amp;"
    );
}

#[test]
fn context_filters_respect_remaining_output_work_and_registration_recovery() {
    let source = program(
        r#"
context = Json.Object([Member("value", Json.Text("&<>\"'é🙂"))]?)
for filter, size in [("escape_text", 21), ("escape_double_quoted", 20), ("escape_single_quoted", 20)]?:
    for extra in [0, 1]?:
        mut bounds = std.template.limits()
        bounds.max_output = NonNegative.try_from(size + extra).unwrap()
        mut templates = std.template.engine(&bounds)?
        templates.add("value", f"x{{{{ value | {filter} }}}}"?)?
        templates.add("ok", "ok")?
        match templates.render("value", &context):
            case Ok(output):
                print(output.byte_len() == size + 1)?
            case Err(TemplateError.LimitExceeded):
                print(extra == 0)?
            case _:
                print(False)?
        print(templates.render("ok", &context)? == "ok")?
    mut bounds = std.template.limits()
    bounds.max_steps = 8
    mut templates = std.template.engine(&bounds)?
    templates.add("value", f"{{{{ value | {filter} | {filter} }}}}"?)?
    templates.add("once", f"{{{{ value | {filter} }}}}"?)?
    match templates.render("value", &context):
        case Err(TemplateError.LimitExceeded):
            print(True)?
        case _:
            print(False)?
    print(templates.render("once", &context)?.byte_len() == size)?
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
for source in ["{% if false %}{{ value | escape_attribute }}{% endif %}", "{{ value | escape_text() }}"]?:
    match templates.add("recover", source):
        case Err(TemplateError.Syntax(_)):
            print(True)?
        case _:
            print(False)?
templates.add("recover", "{{ 42 | escape_text }}")?
match templates.render("recover", &context):
    case Err(TemplateError.TypeMismatch):
        print(True)?
    case _:
        print(False)?
"#,
    );
    let source = source.replace("TemplateError]:", "TemplateError | OverflowError[i64]]:");
    assert_eq!(
        support::run_file_stdout(&source),
        std::iter::repeat_n("True", 21)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn context_filter_render_allocation_failures_leave_engine_reusable() {
    let source = format!(
        "from std.bytes import BytesError\n{}",
        program(
            r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
templates.add("value", "{{ value | escape_double_quoted }}")?
context = Json.Object([Member("value", Json.Text("a long &<>\"'é🙂 value requiring heap storage"))]?)
expected = "a long &amp;<>&quot;'é🙂 value requiring heap storage"
mut failures = 0
mut successes = 0
mut correct = True
for budget in range(80):
    marker = f"__test_fail_allocations_after_{budget}__"?
    print(marker)?
    result = templates.render("value", &context)
    print("__test_restore_allocations__")?
    match result:
        case Err(TemplateError.Allocation(AllocError.OutOfMemory)):
            failures += 1
        case Err(TemplateError.Bytes(BytesError.Allocation(AllocError.OutOfMemory))):
            failures += 1
        case Ok(output):
            if output != expected:
                correct = False
            successes += 1
        case _:
            correct = False
    if templates.render("value", &context)? != expected:
        correct = False
print(failures > 0, successes > 0, correct)?
"#,
        )
        .replace("TemplateError]:", "TemplateError | OverflowError[i64]]:")
    );
    assert_eq!(output_with_optimization(&source, false), "True True True");
}

#[test]
fn template_registration_and_rendering_accept_retained_and_read_only_ranges() {
    let source = program(
        r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
mut name = "!page!"
mut text = "!<p>café 🙂</p>!"
owned_name = name[1:-1]?
templates.add(&owned_name, &text[1:-1])?
name = "changed"
text = "changed"
lookup = "!page!"
context = Json.Object([]?)
schema = std.template.Shape.Object([]?)
templates.validate(&lookup[1:-1], &schema)?
templates.validate_html(&lookup[1:-1])?
print(templates.render(&lookup[1:-1], &context)? == "<p>café 🙂</p>")?
print(templates.render_checked(&lookup[1:-1], &context, &schema)? == "<p>café 🙂</p>")?
print(templates.render_html(&lookup[1:-1], &context)? == "<p>café 🙂</p>")?
escaped = "!<&\0🙂!"
print(std.template.escape(&escaped[1:-1], std.template.HtmlContext.Text)? == "&lt;&amp;\0🙂")?
"#,
    );
    assert_eq!(support::run_file_stdout(&source), "True\nTrue\nTrue\nTrue");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn registration_shares_the_retained_template_name_handle() {
    let source = r#"
import std.template
from std.json import Json

def needed(name: &str) -> Result[i64, AllocError | IoError | std.template.TemplateError]:
    bounds = std.template.limits()
    for budget in range(200):
        mut templates = std.template.engine(&bounds)?
        marker = "__test_fail_allocations_after_".concat(str.from(budget)?)?.concat("__")?
        print(marker)?
        result = templates.add(name, "ok")
        print("__test_restore_allocations__")?
        if result.is_ok():
            return Ok(budget)
    Ok(-1)

def registered() -> Result[std.template.Engine, AllocError | std.template.TemplateError]:
    name = "a template name ".concat("beyond inline storage")?
    mut templates = std.template.engine(&std.template.limits())?
    templates.add(&name, "ok")?
    Ok(templates)

def main() -> Result[(), AllocError | IoError | std.template.TemplateError]:
    short = needed("a")?
    long = needed("a template name beyond inline storage")?
    print(short >= 0, short == long)?
    templates = registered()?
    print(templates.render("a template name beyond inline storage", &Json.Object([]?))? == "ok")?
    Ok(())
"#;
    assert_eq!(output_with_optimization(source, false), "True True\nTrue");
}

#[test]
fn long_quoted_literals_and_include_names_preserve_unicode_and_escapes() {
    let source = program(
        r#"
name = ("a".repeat(8192)? + "é🙂'tail")?
content = ("a".repeat(8192)? + "é🙂\n\t'\\\"")?
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
templates.add(name, "included")?
quoted_name = ("a".repeat(8192)? + "é🙂\\'tail")?
include = (("{% include '" + quoted_name)? + "' %}")?
templates.add("include", include)?
context = Json.Object([]?)
print(templates.render("include", &context, html=False)? == "included")?
quoted_value = ("a".repeat(8192)? + "é🙂\\n\\t\\'\\\\\"")?
literal = (("{{ '" + quoted_value)? + "' }}")?
templates.add("literal", literal)?
print(templates.render("literal", &context, html=False)? == content)?
templates.add("double", "{{ \"é🙂\\\"'\" }}")?
print(templates.render("double", &context, html=False)? == "é🙂\"'")?
for invalid in ["{{ 'unterminated }}", "{% include 'unterminated %}", "{% include x %}"]?:
    print(templates.add("invalid", invalid).is_err())?
"#,
    );
    for optimize in [false, true] {
        assert_eq!(
            output_with_optimization(&source, optimize),
            "True\nTrue\nTrue\nTrue\nTrue\nTrue"
        );
    }
}

#[test]
fn template_quote_loops_do_not_rescan_scalar_indices() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("main.plenty");
    std::fs::write(
        &path,
        program("bounds = std.template.limits()\nmut templates = std.template.engine(&bounds)?\ntemplates.add(\"page\", \"{{ 'quoted' }}{% include 'fragment' %}\")?"),
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"))
        .env("PLENTY_CACHE", "off")
        .args([
            "--release",
            "--dump",
            "ops",
            "--dump-std",
            "--dump-filter",
            "std.template.",
        ])
        .arg(&path)
        .args(["-o", "-"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let dump = String::from_utf8(output.stdout).unwrap();
    for name in ["quoted", "include_expression"] {
        let header = format!("std.template.{name} ");
        let function = dump
            .split("\n==== ")
            .find(|function| function.starts_with(&header))
            .unwrap_or_else(|| panic!("{header} missing from dump"));
        let (_, loop_and_tail) = function.split_once("\nLoop\n").expect("scalar loop");
        // Indexing a text view counts the entire input before locating the
        // scalar. Repeating even index zero in these loops makes ASCII inputs
        // quadratic; sequential scalar iteration must remain the only scan.
        assert!(
            !loop_and_tail.contains("Collection(TextView(Index))"),
            "{function}"
        );
        assert!(
            loop_and_tail.contains("Collection(TextView(Scalar))"),
            "{function}"
        );
    }
}
