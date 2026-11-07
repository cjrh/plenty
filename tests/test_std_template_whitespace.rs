//! Delimiter trimming changes adjacent source literals, never rendered values.
fn check(body: &str) {
    let source = format!(
        "import std.template\nimport std.test\nimport std.bytes\nfrom std.template import TemplateError\nfrom std.json import Json, Member\ndef main() -> Result[(), AllocError | IoError | TemplateError | std.test.TestError | std.bytes.BytesError]:\n{}    print(\"ok\")?\n    Ok(())\n",
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
fn whitespace_delimiters_make_nav_adjacent_without_trimming_values_or_preformatted_text() {
    check(
        r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
context = Json.Object([Member("items", Json.Array([Json.Text("Ada"), Json.Text("Grace")]?)), Member("value", Json.Text(" \t<&>\r\n "))]?)
templates.add("nav", "<nav>\n  {%- for item in items -%}\n  <a>{{ item }}</a>\n  {%- endfor -%}\n</nav>")?
std.test.check(templates.render("nav", &context)? == "<nav><a>Ada</a><a>Grace</a></nav>", "nav adjacency")?
templates.add("default", "<pre>\n  {{ value }}\n</pre>")?
std.test.check(templates.render("default", &context)? == "<pre>\n   \t&lt;&amp;&gt;\r\n \n</pre>", "default and preformatted bytes")?
templates.add("values", "a \t{{- value -}} \r\nb{{ ' -}} ' }}c{{ -42 }}d{{- -42 -}}e")?
std.test.check(templates.render("values", &context)? == "a \t&lt;&amp;&gt;\r\n b -}} c-42d-42e", "runtime values and quoted delimiter text untouched")?
templates.add("numeric", "a  {{-42}}  b/{{-0.5}}/{{-.5}}/x \t{{- 42 -}} \n y/x \t{{- -42 -}} \n y")?
std.test.check(templates.render("numeric", &context)? == "a  -42  b/-0.5/-0.5/x42y/x-42y", "compact numeric signs retain default whitespace and values")?
templates.add("one-side", "a {{- 'L' }} b {{ 'R' -}} c")?
std.test.check(templates.render("one-side", &context)? == "aL b Rc", "each side independent")?
"#,
    );
}

#[test]
fn whitespace_delimiters_are_lexical_across_comments_and_control_flow() {
    check(
        r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
context = Json.Object([]?)
templates.add("comments", "a \r\n{#- ignored -#}\t b {# plain #} \n{{- 'c' -}}\n d")?
std.test.check(templates.render("comments", &context)? == "ab cd", "comments and tag boundaries")?
templates.add("control", "a\n{%- if false -%}\n discarded \n{%- else -%}\n yes \n{%- endif -%}\nb{% for x in empty %} no {% else -%}\n empty \n{%- endfor %}c")?
values = Json.Object([Member("empty", Json.Array([]?))]?)
std.test.check(templates.render("control", &values)? == "ayesbemptyc", "branch and empty loop trimming")?
templates.add("boundaries", "a {{ ' x ' }} \n{%- if false %}gone{% endif %}\n z")?
std.test.check(templates.render("boundaries", &context)? == "a  x \n z", "left trim cannot reach an emitted value")?
templates.add("empty-comments", "a {#-#} b {#- -#} c")?
std.test.check(templates.render("empty-comments", &context)? == "a bc", "empty comments and adjacent markers")?
"#,
    );
}

#[test]
fn whitespace_delimiters_trim_exactly_ascii_whitespace_and_keep_unicode_bytes() {
    check(
        r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
context = Json.Object([]?)
space = std.bytes.from_list(&[32u8, 9u8, 10u8, 11u8, 12u8, 13u8]?)?.to_text()?
source = f"a{space}{{{{- 'X' -}}}}{space}b"?
templates.add("ascii", &source)?
std.test.check(templates.render("ascii", &context)? == "aXb", "all six ASCII whitespace bytes")?
templates.add("unicode", "é🙂  \t{{- 'X' -}}\r\n β")?
std.test.check(templates.render("unicode", &context)? == "é🙂 X β", "NBSP and EM SPACE stop trimming")?
templates.add("line-endings", "a\r\n\r\n {#- both -#} \n\r\t b")?
std.test.check(templates.render("line-endings", &context)? == "ab", "mixed line endings")?
"#,
    );
}

#[test]
fn whitespace_delimiters_preserve_inheritance_source_limits_and_original_byte_diagnostics() {
    check(
        r#"
bounds = std.template.limits()
mut templates = std.template.engine(&bounds)?
context = Json.Object([]?)
templates.add("base", "A \n{% block body -%}\n base \n{%- endblock %}\n Z")?
templates.add("child", " \n{#- comment -#}\n{%- extends 'base' -%}\n{%- block body -%}\n child \n{%- endblock -%}\n")?
std.test.check(templates.render("child", &context)? == "A \nchild\n Z", "trim compiled in defining template, no inherited boundary stripping")?
for source in ["é \r\n{{- unknown | bad -}}", "é \r\n{{- 'unterminated", "é \r\n{% - if true %}"]?:
    result = templates.add("retry", &source)
    std.test.check(result == Result[(), TemplateError].Err(TemplateError.Syntax(5)), "original UTF-8 byte offset")?
std.test.check(templates.add("retry", "{{- -}}") == Result[(), TemplateError].Err(TemplateError.Syntax(0)), "empty trimmed expression rejected")?
templates.add("retry", "ok")?
std.test.check(templates.render("retry", &context)? == "ok", "failed registration remains atomic")?
mut limited = std.template.limits()
limited.max_source = 3
mut small = std.template.engine(&limited)?
std.test.check(small.add("x", " {#- -#} ") == Result[(), TemplateError].Err(TemplateError.LimitExceeded), "removed bytes still count as source")?
small.add("x", "ok")?
limited.max_source = 100
limited.max_tokens = 1
mut tokens = std.template.engine(&limited)?
tokens.add("one", " \n{{- 'X' -}}\r\n ")?
std.test.check(tokens.render("one", &context)? == "X", "trimmed empty literals do not consume tokens")?
std.test.check(tokens.add("two", "{{- 'Y' -}}") == Result[(), TemplateError].Err(TemplateError.LimitExceeded), "retained tokens remain bounded")?
"#,
    );
}
