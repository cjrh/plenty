# `std.template`

Bounded text templates, implemented in Plenty. Register named sources once with
`Engine.add`, then render them repeatedly with explicit `std.json.Json` objects.
No files are opened and no Plenty code, shell commands or methods are evaluated.

```plenty
import std.template
from std.template import TemplateError
from std.json import Json, Member

def main() -> Result[(), TemplateError | AllocError | IoError]:
    bounds = std.template.limits()
    mut templates = std.template.engine(&bounds)?
    templates.add("greeting", "<p>Hello {{ name }}!</p>")?
    values = Json.Object([Member("name", Json.Text("Ada & Grace"))]?)
    print(templates.render("greeting", &values)?)?
    Ok(())
```
```output
<p>Hello Ada &amp; Grace!</p>
```

## Language

The syntax is a small Jinja-style subset, using [Tera](https://keats.github.io/tera/)
as a syntax reference, not a Tera/Jinja compatibility implementation.

| Form | Meaning |
| --- | --- |
| `{{ person.name }}` | Look up an object field; `items.0` indexes an array |
| `{{ 'hello' }}`, `{{ 42 }}`, `{{ true }}` | Text, numeric, boolean and `null` literals |
| `{{ name \| upper }}` | Apply a filter; filters can be chained |
| `{# comment #}` | Ignore a comment |
| `{% if active %}yes{% else %}no{% endif %}` | Conditional, with optional `else` |
| `{% if not active %}...{% endif %}` | Negate a condition |
| `{% for item in items %}{{ item.name }}{% else %}empty{% endfor %}` | Array loop; optional `else` renders only for an empty array |
| `{% block title %}Title{% endblock %}` | Define a top-level named block |
| `{% extends 'base' %}` | Inherit from a registered template |

Names use ASCII letters, digits and underscores, starting with a letter or
underscore. A path's later segments may be nonnegative array indices. Text
literals support `\\`, `\'`, `\"`, `\n`, `\r` and `\t`. Conditions treat
null, false, zero and empty text/arrays/objects as false. Missing fields are
errors, including in conditions. Arrays and objects cannot be interpolated
directly; loop over an array or select a scalar field.

The innermost loop provides `loop.index` (one-based), `loop.index0`, `loop.first`,
`loop.last` and `loop.length`. A loop variable shadows an outer value with the
same name and is removed when that loop ends. The name `loop` is reserved for
loop metadata. Loops accept a single variable and an array path, without spaces
inside that path. Object iteration is not supported.

Filters are `upper`, `lower`, `trim`, `length`, `escape` and `safe`. Text filters
require text. `length` counts Unicode scalars for text and entries for arrays
or objects. Filters do not accept arguments. Arithmetic, comparisons, boolean
operators other than `not`, calls, bracket indexing, `elif`, assignments,
includes, macros, raw blocks and whitespace-trimming delimiters are not part
of this version. Unsupported syntax is rejected when registering a template,
including inside branches that would never execute.

## Escaping and inheritance

`render(name, context, html=True)` escapes `&`, `<`, `>`, `"` and `'` in
interpolated values. Literal template text is trusted. `escape` marks its
escaped result safe, avoiding double escaping; `safe` explicitly trusts the
value. `upper` and `lower` remove that trust. Use `html=False` for plain-text
emails or generated text.

This escaping is for HTML text and quoted ordinary attributes. It does not
validate URLs or make values safe inside JavaScript, CSS, tag names, unquoted
attributes or raw HTML. Template authors control `safe` and literal markup;
do not treat untrusted templates as a markup sanitizer.

`extends` must precede every non-whitespace, non-comment construct. An extending
template contains only whitespace, comments and top-level blocks. Blocks cannot
be nested or repeated within a template. The most-derived definition replaces
its ancestor's entire block; no `super()` call is provided. Blocks absent from
the base's layout are not rendered. Parent names are literal registry names,
not filesystem paths. Missing parents and inheritance cycles are render errors.

## Values, limits and errors

The root context must be `Json.Object`. Build values explicitly with
`Json` and `Member`, or supply a value from the [JSON decoder](json.md).
Object keys must be unique and float values finite throughout the context.
Values are read-only; rendering builds an indexed view and does not modify or
deep-copy the caller's tree. This API needs no serialization derivation. A
future derived serializer can provide the same JSON data model without changing
template syntax; [derived serialization](../../proposals/derived-serialization.md)
is currently a proposal, not an implemented conversion.

`limits()` returns:

| Field | Default | Bounds |
| --- | ---: | --- |
| `max_source` | 1,048,576 | Total UTF-8 bytes of registered names and sources |
| `max_tokens` | 16,384 | Total compiled tokens across templates |
| `max_templates` | 128 | Registered template count |
| `max_context_nodes` | 65,536 | Values in a render context, including its root |
| `max_context_bytes` | 16,777,216 | UTF-8 bytes of context keys and text values |
| `max_depth` | 64 | Syntax/context/render nesting and inheritance chain; allowed 1–64 |
| `max_steps` | 1,000,000 | Render operations, including context construction, lookup and loop visits |
| `max_output` | 16,777,216 | UTF-8 output bytes and transformed text values |

Other limits must be nonnegative; the context-node limit must be positive.
`engine(&bounds)` copies the settings. `add` compiles and validates before
registering; a failure leaves the engine unchanged. Names cannot be empty or
registered twice. A render failure returns no partial output and leaves the
engine reusable. Limits bound sizes and work counts, not an exact heap-byte
quota or wall-clock deadline. Text filters may allocate intermediate text.

`TemplateError` distinguishes invalid limits, exceeded limits, syntax with a
zero-based source location, missing/duplicate template names, missing values,
type mismatches, cycles, allocation failures and byte-buffer errors. Syntax
locations are UTF-8 byte offsets at the containing template construct, rather
than at an individual expression character. Allocation errors remain explicit, including those
wrapped by `BytesError`.
