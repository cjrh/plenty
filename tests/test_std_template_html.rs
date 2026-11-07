//! Strict contextual rendering preflights complete template composition.
fn check(body: &str) {
    let source = format!(
        "import std.template\nimport std.test\nfrom std.template import TemplateError\nfrom std.json import Json, Member\ndef main() -> Result[(), AllocError | IoError | TemplateError | std.test.TestError]:\n{}    print(\"ok\")?\n    Ok(())\n",
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
fn strict_html_infers_text_and_attribute_contexts_and_preserves_legacy_render() {
    check(
        r#"
bounds = std.template.limits()
mut pages = std.template.engine(&bounds)?
values = Json.Object([Member("value", Json.Text("<&\"' >"))]?)
pages.add("page", "<!DoCtYpE html><HTML><head><title>{{ value }}</title></head><body><a href='/trusted?x=1&amp;y=2' title=\"{{ value }}\" data-label='{{ value }}'>{{ value }}</a><br /></body></HTML>")?
pages.validate_html("page")?
std.test.check(pages.render_html("page", &values)? == "<!DoCtYpE html><HTML><head><title>&lt;&amp;\"' &gt;</title></head><body><a href='/trusted?x=1&amp;y=2' title=\"<&amp;&quot;' >\" data-label='<&amp;\"&#x27; >'>&lt;&amp;\"' &gt;</a><br /></body></HTML>", "inferred emission")?
pages.add("selectors", "<p>{{ value | escape_single_quoted | escape_text }}</p><i title=\"{{ value | escape_text | upper }}\"></i>")?
pages.validate_html("selectors")?
std.test.check(pages.render_html("selectors", &values)? == "<p>&lt;&amp;\"' &gt;</p><i title=\"<&amp;&quot;' >\"></i>", "selector override and transform revoke")?
pages.add("legacy", "{{ value | safe }}|{{ value }}")?
std.test.check(pages.render("legacy", &values)? == "<&\"' >|&lt;&amp;&quot;&#x27; &gt;", "legacy trust unchanged")?
pages.add("link", "<link href='/trusted.css' rel='{{ relation }}' media='screen'><a href='/trusted' target='{{ target }}'>go</a>")?
links = Json.Object([Member("relation", Json.Text("stylesheet")), Member("target", Json.Text("_blank"))]?)
std.test.check(pages.render_html("link", &links)? == "<link href='/trusted.css' rel='stylesheet' media='screen'><a href='/trusted' target='_blank'>go</a>", "ordinary link metadata")?
pages.add("entities", "<p title=\"a &quot; {{ value }}\">{{ value }}</p>")?
pages.validate_html("entities")?
std.test.check(pages.render_html("entities", &values)?.startswith("<p title=\"a &quot; <&amp;&quot;' >\">"), "entities do not close source quotes")?
"#,
    );
}

#[test]
fn strict_html_rejects_unsafe_positions_and_malformed_structure_before_values() {
    check(
        r#"
bounds = std.template.limits()
mut pages = std.template.engine(&bounds)?
values = Json.Object([]?)
for source in ["<{{ missing }}>", "<p {{ missing }}='x'></p>", "<p title={{ missing }}></p>", "<p title='{{ missing | escape_double_quoted }}'></p>", "<a href='{{ missing }}'>x</a>", "<img srcset='{{ missing }}'>", "<p onclick='x'></p>", "<p style='x'></p>", "<p srcdoc='x'></p>", "<script>{{ missing }}</script>", "<style>x</style>", "<svg></svg>", "<math></math>", "<iframe></iframe>", "<object></object>", "<embed>", "<noscript></noscript>", "<plaintext>", "<xmp></xmp>", "<template></template>", "<!-- comment -->", "<textarea>x</textarea>", "<title><b></title>", "<title>{{ missing | safe }}</title>", "<div><span></div></span>", "<p title='x' TITLE='y'></p>", "<p title='x'class='y'></p>", "<div/>", "x<!doctype html>", "<!doctype html><!doctype html>", "<div>", "<div"]?:
    mut candidate = std.template.engine(&bounds)?
    candidate.add("bad", &source)?
    match candidate.render_html("bad", &values):
        case Err(TemplateError.InvalidHtml(name, _)):
            std.test.check(name == "bad", "defining template")?
        case _:
            std.test.check(False, &source)?
pages.add("trusted-url", "<form action='/save'><button formaction='/cancel'>go</button><img src='/asset'></form>")?
pages.validate_html("trusted-url")?
"#,
    );
}

#[test]
fn strict_html_preflight_checks_unreached_branches_and_exact_scope_boundaries() {
    check(
        r#"
bounds = std.template.limits()
for source in ["{% if false %}<p title='{{ missing | escape_double_quoted }}'></p>{% endif %}", "{% if false %}{{ missing | safe | upper }}{% endif %}", "{% if false %}{{ missing | escape | length }}{% endif %}", "<p title='{% if false %}x{% endif %}'></p>", "<title>{% if false %}x{% endif %}</title>", "{% if false %}<b>{% else %}<i>{% endif %}x</b>", "<div>{% if false %}</div><span>{% endif %}</div>", "{% for row in rows %}<b>{% endfor %}</b>"]?:
    mut pages = std.template.engine(&bounds)?
    pages.add("bad", &source)?
    std.test.check(pages.validate_html("bad").is_err(), &source)?
mut pages = std.template.engine(&bounds)?
pages.add("good", "<div>{% if active %}<b>yes</b>{% else %}<i>no</i>{% endif %}<ul>{% for row in rows %}<li>{{ row }}</li>{% else %}<li>empty</li>{% endfor %}</ul></div>")?
values = Json.Object([Member("active", Json.Bool(False)), Member("rows", Json.Array([]?))]?)
std.test.check(pages.render_html("good", &values)? == "<div><i>no</i><ul><li>empty</li></ul></div>", "neutral control scopes")?
"#,
    );
}

#[test]
fn strict_html_composes_trimmed_includes_and_isolated_inheritance() {
    check(
        r#"
bounds = std.template.limits()
mut pages = std.template.engine(&bounds)?
pages.add("layout", "<!doctype html><html><head><title>{{ title }}</title></head><body>{% block body %}<p>base</p>{% endblock %}</body></html>")?
pages.add("form", "<form action='/save'><button title=\"{{ label | escape_double_quoted }}\">{{ label | escape_text }}</button></form>")?
pages.add("page", "{% extends 'layout' %}{% block body %}\n {%- for action in actions -%}\n {% include 'form' with action %}\n {%- endfor -%}\n{% endblock %}")?
values = Json.Object([Member("title", Json.Text("<&>")), Member("actions", Json.Array([Json.Object([Member("label", Json.Text("Say \"yes\" & go"))]?)]?))]?)
std.test.check(pages.render_html("page", &values)? == "<!doctype html><html><head><title>&lt;&amp;&gt;</title></head><body><form action='/save'><button title=\"Say &quot;yes&quot; &amp; go\">Say \"yes\" &amp; go</button></form></body></html>", "composed strict output")?
pages.add("ancestor", "<div>{% block body %}<span>base</span>{% endblock %}</div>")?
pages.add("derived", "{% extends 'ancestor' %}{% block body %}{% include 'ancestor' %}{% endblock %}")?
std.test.check(pages.render_html("derived", &values)? == "<div><div><span>base</span></div></div>", "ancestor inclusion is not a cycle")?
pages.add("open", "<b>")?
pages.add("split", "{% include 'open' %}x</b>")?
std.test.check(pages.validate_html("split").is_err(), "fragment must preserve stack")?
pages.add("cycle", "{% if false %}{% include 'cycle' %}{% endif %}")?
std.test.check(pages.validate_html("cycle").is_err(), "static cycle even in dead branch")?
pages.add("missing", "{% if false %}{% include 'absent' %}{% endif %}")?
std.test.check(pages.validate_html("missing").is_err(), "all static includes resolve")?
std.test.check(pages.render_html("form", &Json.Object([Member("label", Json.Text("retry"))]?))? == "<form action='/save'><button title=\"retry\">retry</button></form>", "engine reusable")?
"#,
    );
}

#[test]
fn strict_html_reports_original_utf8_offsets_and_defining_template() {
    check(
        r#"
bounds = std.template.limits()
mut pages = std.template.engine(&bounds)?
pages.add("bad", "é🙂 \n{{- bad | escape_double_quoted }}")?
match pages.validate_html("bad"):
    case Err(TemplateError.InvalidHtml(name, offset)):
        std.test.check(name == "bad" and offset == 8, "original byte offset before trim")?
    case _:
        std.test.check(False, "expected source location")?
pages.add("base", "é🙂 <div>{% block body %}base{% endblock %}</div>")?
pages.add("child", "{% extends 'base' %}{% block body %}<{{ name }}>{% endblock %}")?
match pages.validate_html("child"):
    case Err(TemplateError.InvalidHtml(name, offset)):
        std.test.check(name == "child" and offset == 37, "override defining source offset")?
    case _:
        std.test.check(False, "expected defining source")?
"#,
    );
}

#[test]
fn strict_html_shares_work_depth_and_output_limits_and_recovers() {
    check(
        r#"
mut bounds = std.template.limits()
bounds.max_output = 3
mut pages = std.template.engine(&bounds)?
pages.add("large", "{{ value }}")?
values = Json.Object([Member("value", Json.Text("<&"))]?)
match pages.render_html("large", &values):
    case Err(TemplateError.LimitExceeded):
        pass
    case _:
        std.test.check(False, "entity expansion output limit")?
std.test.check(pages.render_html("large", &Json.Object([Member("value", Json.Text("ok"))]?))? == "ok", "retry after output failure")?
bounds.max_output = 100
bounds.max_steps = 10
mut short = std.template.engine(&bounds)?
short.add("x", "<p>x</p>")?
short.validate_html("x")?
match short.render_html("x", &Json.Object([]?)):
    case Err(TemplateError.LimitExceeded):
        pass
    case _:
        std.test.check(False, "preflight and rendering share work cap")?
bounds.max_steps = 1000
bounds.max_depth = 1
mut shallow = std.template.engine(&bounds)?
shallow.add("bad", "<div><p>x</p></div>")?
std.test.check(shallow.validate_html("bad").is_err(), "HTML stack depth")?
shallow.add("good", "<p>x</p>")?
std.test.check(shallow.render_html("good", &Json.Object([]?))? == "<p>x</p>", "depth failure recovery")?
bounds = std.template.limits()
mut capped = std.template.engine(&bounds)?
mut name = "data-"
for _ in range(59):
    name = f"{name}x"?
within = f"<p {name}='ok'></p>"?
capped.add("name64", &within)?
capped.validate_html("name64")?
over = f"<p {name}x='bad'></p>"?
capped.add("name65", &over)?
match capped.validate_html("name65"):
    case Err(TemplateError.LimitExceeded):
        pass
    case _:
        std.test.check(False, "name cap checked before copying")?
mut attributes = "<p"
for number in range(128):
    attributes = f"{attributes} data-x{number}=''"?
attributes128 = f"{attributes}></p>"?
capped.add("attrs128", &attributes128)?
capped.validate_html("attrs128")?
attributes129 = f"{attributes} data-over=''></p>"?
capped.add("attrs129", &attributes129)?
match capped.validate_html("attrs129"):
    case Err(TemplateError.LimitExceeded):
        pass
    case _:
        std.test.check(False, "attribute count cap")?
capped.validate_html("name64")?
"#,
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn strict_html_allocation_failures_preserve_engine_and_context() {
    let source = r#"
import std.template
from std.template import TemplateError
from std.json import Json, Member
from std.bytes import BytesError
def main() -> Result[(), AllocError | IoError | TemplateError]:
    bounds = std.template.limits()
    mut pages = std.template.engine(&bounds)?
    pages.add("part", "<button title=\"{{ value }}\">{{ value }}</button>")?
    pages.add("page", "<div>{% include 'part' %}</div>")?
    values = Json.Object([Member("value", Json.Text("<&\" sufficiently long to allocate"))]?)
    for budget in [0, 3, 6, 9, 12, 18, 24, 32, 48, 64, 96, 128, 192, 256, 384, 768]?:
        marker = f"__test_fail_allocations_after_{budget}__"?
        print(marker)?
        result = pages.render_html("page", &values)
        print("__test_restore_allocations__")?
        match result:
            case Err(TemplateError.Allocation(_)):
                print("allocation")?
            case Err(TemplateError.Bytes(BytesError.Allocation(_))):
                print("allocation")?
            case Ok(value):
                print(value == "<div><button title=\"<&amp;&quot; sufficiently long to allocate\">&lt;&amp;\" sufficiently long to allocate</button></div>")?
            case _:
                print("wrong")?
        pages.validate_html("page")?
        print(pages.render_html("page", &values)? == "<div><button title=\"<&amp;&quot; sufficiently long to allocate\">&lt;&amp;\" sufficiently long to allocate</button></div>")?
    Ok(())
"#;
    let output = support::run_file_stdout(source);
    let lines: Vec<_> = output.lines().collect();
    assert_eq!(lines.len(), 32, "{output}");
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
    assert_eq!(&lines[30..], &["True", "True"], "{output}");
}

#[cfg(feature = "runtime-checks")]
mod support;
