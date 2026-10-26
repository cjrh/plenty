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

Set limits for your application before constructing the engine: for example,
lower `max_output` for a short notification. Registration and rendering are
fallible, and the example propagates those failures to the caller. Keep the
engine and render it again with different values to reuse its parsed templates.

See the [`std.template` reference](../../design/stdlib/template.md) for the
supported syntax, filters, inheritance rules and precise limits.
