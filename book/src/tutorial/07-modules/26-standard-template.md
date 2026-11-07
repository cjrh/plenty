# Rendering text templates

Use `std.template` when the layout of a page or message is clearer as text than
as a sequence of string concatenations. Register a layout once and pass values
for each render. Templates cannot call your program's functions or read files.

```plenty
import std.template
from std.template import TemplateError
from std.json import Json, Member

def main() -> Result[(), TemplateError | AllocError | IoError]:
    bounds = std.template.limits()
    mut pages = std.template.engine(&bounds)?
    pages.add("layout", "<h1>{% block title %}Report{% endblock %}</h1>{% block body %}{% endblock %}")?
    pages.add("report", "{% extends 'layout' %}{% block title %}{{ title }}{% endblock %}{% block body %}<ul>{% for name in names %}<li>{{ name }}</li>{% else %}<li>No entries</li>{% endfor %}</ul>{% endblock %}")?
    values = Json.Object([
        Member("title", Json.Text("Team & friends")),
        Member("names", Json.Array([Json.Text("Ada"), Json.Text("Grace")]?)),
    ]?)
    print(pages.render("report", &values)?)?
    Ok(())
```
```output
<h1>Team &amp; friends</h1><ul><li>Ada</li><li>Grace</li></ul>
```

The context uses the [JSON value model](10-standard-json.md) explicitly; no
class-to-template conversion happens automatically. Fields such as
`{{ person.name }}` look up object members. A missing field returns an error
rather than silently producing an empty string. An empty array takes the loop's
`else` branch, which is useful for empty reports.

Interpolated text is HTML-escaped by default. Keep user text in HTML text or
quoted ordinary attributes. HTML escaping does not validate links or JavaScript;
do not place user values in scripts, styles or tag names. For a plain-text
message, call `pages.render("report", &values, html=False)` with a plain-text
template. The `safe` filter deliberately disables escaping for trusted markup.

When quotes should stay literal in text, choose `escape_text` on that
placeholder. For an ordinary attribute, use the filter matching its quote
delimiter. Keep the original value in `Json.Text`; these selections escape once
at final output and need no `safe` assertion:

```plenty
import std.template
from std.template import TemplateError
from std.json import Json, Member

def main() -> Result[(), TemplateError | AllocError | IoError]:
    bounds = std.template.limits()
    mut pages = std.template.engine(&bounds)?
    pages.add("button", "<button title=\"{{ label | escape_double_quoted }}\">{{ label | escape_text }}</button>")?
    values = Json.Object([Member("label", Json.Text("Say \"hello\" & smile"))]?)
    print(pages.render("button", &values)?)?
    Ok(())
```
```output
<button title="Say &quot;hello&quot; &amp; smile">Say "hello" &amp; smile</button>
```

Use `escape_single_quoted` for an attribute surrounded by single quotes. Put
the selected context after `upper` or `lower`, which reset escaping to the
render's default; `trim` preserves it. Multiple context filters select the last
one over the raw value. A context filter requests escaping even with
`html=False`. These filters cannot validate URLs or protect script/style content.

Set limits for your application before constructing the engine: for example,
lower `max_output` for a short notification. Registration and rendering are
fallible, and the example propagates those failures to the caller. Keep the
engine and render it again with different values to reuse its parsed templates.

## Checking a declared context

For reusable layouts, declare a `Shape` and call `validate` after registration
to catch misspelled fields in every reachable branch and loop body, even with
no sample values. Use `render_checked` to verify each dynamic JSON request too.
Required nullable fields can render null as empty text; optional absent fields
cannot be read safely by a template. The [declared-shape example](../../design/stdlib/template-schema.md)
shows the fallible list/box construction, exact guarantees and limits. This is
runtime preflight for declared data, not compile-time checking of template strings.

## Controlling literal spacing

For adjacent inline navigation links, trim the source indentation around the
loop while keeping its layout readable:

```plenty
import std.template
from std.template import TemplateError
from std.json import Json, Member

def main() -> Result[(), TemplateError | AllocError | IoError]:
    bounds = std.template.limits()
    mut pages = std.template.engine(&bounds)?
    pages.add("nav", """<nav>
        {%- for name in names -%}
            <a>{{ name }}</a>
        {%- endfor -%}
    </nav>""")?
    values = Json.Object([Member("names", Json.Array([
        Json.Text("Ada"), Json.Text("Grace"),
    ]?))]?)
    print(pages.render("nav", &values)?)?
    Ok(())
```
```output
<nav><a>Ada</a><a>Grace</a></nav>
```

The hyphen after `{%` removes the preceding literal whitespace; the one before
`%}` removes the following literal whitespace. Expressions and comments support
the same markers (`{{- value -}}` and `{#- comment -#}`). They trim ASCII
space, tab and line/control whitespace only, leaving Unicode whitespace and
rendered values unchanged. Use ordinary delimiters to preserve spacing, such
as the newlines inside a `pre` element. A marker in an overriding block cannot
trim the surrounding base layout. See [whitespace control](../../design/stdlib/template.md#whitespace-control)
for exact boundaries, negative-literal spelling and resource limits.

## Escaping individual fragments

For a small fragment assembled in your program, use `std.template.escape`
rather than a chain of string replacements. Choose `HtmlContext.Text` for text
children or the quote-specific attribute mode for each attribute value. For
example, quotes should stay literal in text but must be escaped inside a
double-quoted attribute:

```plenty
from std.template import escape, HtmlContext, TemplateError

def main() -> Result[(), TemplateError | AllocError | IoError]:
    label = "Say \"hello\" & smile"
    attribute = escape(&label, HtmlContext.DoubleQuotedAttribute, max_bytes=256)?
    child = escape(&label, HtmlContext.Text, max_bytes=256)?
    print(f"<button title=\"{attribute}\">{child}</button>"?)?
    Ok(())
```
```output
<button title="Say &quot;hello&quot; &amp; smile">Say "hello" &amp; smile</button>
```

`max_bytes` includes the expanded entities. The helper returns ordinary text:
passing its output into a template would escape it again. Keep raw values in
`Json.Text` when using `Engine.render`, and use these helpers once per original
value when assembling HTML directly. The context is your choice; the library
does not detect HTML positions or validate URLs, scripts, styles or names.

## Reusable fragments

Use registered fragments for repeated markup. An include with an explicit object
context works inside a loop without rendering strings in your application:

```plenty
import std.template
from std.template import TemplateError
from std.json import Json, Member

def main() -> Result[(), TemplateError | AllocError | IoError]:
    bounds = std.template.limits()
    mut pages = std.template.engine(&bounds)?
    pages.add("action", "<form><button>{{ label }}</button></form>")?
    pages.add("table", "<table>{% for action in actions %}<tr><td>{% include 'action' with action %}</td></tr>{% endfor %}</table>")?
    values = Json.Object([
        Member("actions", Json.Array([
            Json.Object([Member("label", Json.Text("Save & continue"))]?),
            Json.Object([Member("label", Json.Text("Cancel"))]?),
        ]?)),
    ]?)
    print(pages.render("table", &values)?)?
    Ok(())
```
```output
<table><tr><td><form><button>Save &amp; continue</button></form></td></tr><tr><td><form><button>Cancel</button></form></td></tr></table>
```

The fragment receives raw values and escapes them normally. It has its own loop
scope; `loop` metadata belongs to its own loops. Omit `with action` when a fragment
should receive the current root object instead. Includes share the caller's
render limits and cannot load files or call your program's functions.

See the [`std.template` reference](../../design/stdlib/template.md) for the
supported syntax, filters, inheritance rules and precise limits.
