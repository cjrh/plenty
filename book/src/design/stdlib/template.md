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

## Declared context validation

Use `validate(name, schema)` to check every reachable field path and operation
against a declared `Shape`, including branches that a sample render skips.
`render_checked(name, context, schema, html=True)` additionally verifies the
actual JSON request before rendering. See [declared context shapes](template-schema.md)
for nullable/optional fields, unions, arrays, composition, errors and bounds.
Ordinary `add` and `render` keep their existing dynamic contracts.

## Language

The syntax is a small Jinja-style subset, using [Tera](https://keats.github.io/tera/)
as a syntax reference, not a Tera/Jinja compatibility implementation.

| Form | Meaning |
| --- | --- |
| `{{ person.name }}` | Look up an object field; `items.0` indexes an array |
| `{{ 'hello' }}`, `{{ 42 }}`, `{{ true }}` | Text, numeric, boolean and `null` literals |
| `{{ name \| upper }}` | Apply a filter; filters can be chained |
| `{# comment #}` | Ignore a comment |
| `{% if active %}yes{% elif pending %}wait{% else %}no{% endif %}` | Conditional, with optional `elif` branches and `else` |
| `{% if current == item.id and not hidden %}...{% endif %}` | Compare scalars and compose conditions |
| `{% for item in items %}{{ item.name }}{% else %}empty{% endfor %}` | Array loop; optional `else` renders only for an empty array |
| `{% block title %}Title{% endblock %}` | Define a top-level named block |
| `{% extends 'base' %}` | Inherit from a registered template |
| `{% include 'form' with row %}` | Render a registered fragment using an object context |

Names use ASCII letters, digits and underscores, starting with a letter or
underscore. The operator words `not`, `and` and `or` are reserved as complete
expression names; fields such as `record.and` remain valid. A path's later
segments may be nonnegative array indices. Text
literals support `\\`, `\'`, `\"`, `\n`, `\r` and `\t`. Conditions treat
null, false, zero and empty text/arrays/objects as false. Missing fields are
errors when evaluated, including in conditions. Arrays and objects cannot be interpolated
directly; loop over an array or select a scalar field.

The innermost loop provides `loop.index` (one-based), `loop.index0`, `loop.first`,
`loop.last` and `loop.length`. A loop variable shadows an outer value with the
same name and is removed when that loop ends. The name `loop` is reserved for
loop metadata. Loops accept a single variable and an array path, without spaces
inside that path. Object iteration is not supported.

Filters are `upper`, `lower`, `trim`, `length`, `escape`, `safe`,
`escape_text`, `escape_double_quoted` and `escape_single_quoted`. Text filters
require text. `length` counts Unicode scalars for text and entries for arrays
or objects. Filters do not accept arguments. Arithmetic, ordering comparisons,
calls, bracket indexing, assignments, macros and raw blocks are not part
of this version. Unsupported syntax is rejected when registering a template,
including inside branches that would never execute.

### Expressions and conditional branches

Expressions support paths, literals, `==`, `!=`, `not`, `and`, `or` and
parentheses. Precedence, from strongest to weakest, is:

| Form | Meaning |
| --- | --- |
| `(expression)` | Group explicitly |
| `value \| filter` | Filter a value or a parenthesized expression |
| `left == right`, `left != right` | Compare two scalars; chaining is rejected |
| `not expression` | Negate truthiness |
| `left and right` | Both operands are truthy |
| `left or right` | At least one operand is truthy |

For example, `not name | trim == 'Ada'` means `not ((name | trim) == 'Ada')`.
A filter after parentheses applies to the complete result, as in
`(' Ada ' | trim) | upper`. Filters retain their ordinary type and escaping
rules at each node. Boolean results do not accept text or `length` filters.
Comparisons and boolean operators produce boolean
values with the default escaping policy; they do not forward an operand's
`safe` assertion or selected output context.

Equality compares scalar values of the same JSON kind, without conversions.
Booleans, integers and floats are distinct: `true == 1` and `1 == 1.0` are
false. Text compares exactly, `null == null` is true, and finite floats use
numeric equality. Comparing different scalar kinds is false (`!=` is true).
An array or object on either side returns `TypeMismatch`, including when the
other side has a different kind. Both comparison operands are evaluated.

`and` and `or` evaluate left to right, short-circuit, and return a boolean,
rather than returning one of their operands. `false and missing` and
`true or missing` therefore succeed without looking up `missing`. Evaluated
missing fields and invalid filter input still return errors. Every operand's
syntax and filter name is checked during registration, including skipped ones.

An `if` may have any number of `elif` branches followed by at most one `else`.
Conditions are tested in source order until one is truthy; only its body runs.
A flat chain adds no recursive nesting depth. `elif` is invalid outside an
`if`, after its `else`, or as a loop branch. Loop `else` retains its empty-array
meaning. Expressions are also accepted in interpolations and explicit include
contexts; an include still requires an object result. Loops retain their
single-path syntax.

### Whitespace control

Whitespace is preserved by default, including indentation and line endings.
Add a hyphen immediately after an opening delimiter to remove the preceding
literal whitespace, or immediately before a closing delimiter to remove the
following literal whitespace. Each side is independent:

| Form | Effect |
| --- | --- |
| `{{- value }}`, `{%- if active %}`, `{#- comment #}` | Trim before the tag |
| `{{ value -}}`, `{% endif -%}`, `{# comment -#}` | Trim after the tag |
| `{{- value -}}`, `{%- endif -%}`, `{#- comment -#}` | Trim both sides |

Only ASCII space, tab, LF, vertical tab, form feed and CR (bytes `20`, `09`–`0d`
in hexadecimal) are removed. A run can span multiple lines or mixed CR/LF line
endings. Other bytes stop it: non-breaking space and other Unicode whitespace
remain unchanged. A hyphen separated from the delimiter by whitespace is not a
trim marker. For compatibility, an expression-opening hyphen immediately
followed by an ASCII digit or `.` remains a numeric sign: `{{-42}}`,
`{{-0.5}}` and `{{-.5}}` preserve their negative values and surrounding
whitespace. To trim before a numeric expression, separate the marker from its
value: `{{- 42 }}` or `{{- -42 }}`. A closing marker can still be used alone,
as in `{{-42 -}}`. Hyphens and delimiter-like text inside quoted expressions
remain literal.

Trimming happens when registering the source. It changes only the adjacent
literal segment, stopping at a non-whitespace byte or another tag; it never
reaches through comments or control tags, trims an interpolated value, or
modifies the caller's context. It applies equally inside branches and loops,
including branches that do not execute. It does not parse HTML: whitespace in
`pre`, scripts or other meaningful content is preserved unless the author
explicitly marks that boundary for trimming.

Inheritance uses the trimmed literals compiled in each defining template.
A child's trim marker cannot remove text or spacing from the base layout around
a replaced block. Comments and whitespace remain permitted before `extends`;
trim markers do not relax the inheritance or syntax rules.

Source-byte limits and syntax locations count the original UTF-8 source,
including removed whitespace and marker bytes. Fully removed literal segments
produce no tokens; remaining tokens, syntax depth, render steps and output
still use the ordinary limits. Registration failures remain atomic. See
[the tutorial example](../../tutorial/07-modules/26-standard-template.md#controlling-literal-spacing)
for readable source that renders adjacent inline elements.

## Escaping and inheritance

For supported HTML templates, prefer [`validate_html` and
`render_html`](template-html.md): mandatory structural preflight selects text
and quoted-attribute escaping from the literal position. They reject unsupported
positions and unsafe trust filters. The existing `render` behavior below remains
available for trusted source outside that bounded subset.

`render(name, context, html=True)` escapes `&`, `<`, `>`, `"` and `'` in
interpolated values. Literal template text is trusted. Keep original raw text
in `Json.Text`. Select a narrower escaping context per placeholder when exact
text or attribute bytes matter:

| Filter | Escapes | Required position |
| --- | --- | --- |
| `escape_text` | `&`, `<`, `>` | Ordinary HTML text children |
| `escape_double_quoted` | `&`, `"` | Inside a double-quoted ordinary attribute value |
| `escape_single_quoted` | `&`, `'` | Inside a single-quoted ordinary attribute value |

For example, `<p>{{ name | escape_text }}</p>` preserves quotes in text, while
`<i title="{{ name | escape_double_quoted }}"></i>` escapes its double quotes.
Keep the matching delimiters around the complete attribute value. These
filters select the context explicitly; they do not inspect surrounding markup.
Selecting the wrong context can leave a quote delimiter unescaped.

The context filters retain raw text until final interpolation, where the
remaining output-byte budget includes entity expansion. The last context
filter wins: `value | escape_text | escape_double_quoted` escapes the original
value for a double-quoted attribute once. The selection belongs to that
expression, never to the stored JSON value or another placeholder.

`safe` is a separate explicit trust assertion that bypasses escaping. A context
filter after `safe` restores escaping; `safe` after a context filter deliberately
bypasses it. `upper` and `lower` revoke both trust and a selected context, so put
the desired context after those transforms. `trim` preserves the selection or
trust. `length` reads the current raw text's Unicode scalar count (or collection
size) and produces a number with the default policy. Conditions also inspect the
raw value; a context filter only changes final output.

The existing `escape` filter immediately encodes all five characters and marks
that encoded result trusted. Its behavior is unchanged: a later context filter
escapes that already encoded text again, including its ampersands. Prefer one
context filter after ordinary transforms. Do not pre-escape template data or
add `safe` merely to avoid double escaping.

Use `html=False` for plain-text emails or generated text. It disables default
escaping; explicit `escape` and context filters still request escaping. A later
`upper` or `lower` revokes that explicit selection, returning to the render's
default policy.

This escaping is for HTML text and quoted ordinary attributes. It does not
validate URLs or make values safe inside JavaScript, CSS, tag names, unquoted
attributes or raw HTML. Template authors control `safe` and literal markup;
do not treat untrusted templates as a markup sanitizer.

### Escaping individual values

Use `escape(text: &str, context: HtmlContext, max_bytes: NonNegative[i64] = 16777216)`
when assembling a fragment directly or matching another renderer's exact output.
It returns `Result[str, TemplateError]`. Choose the context from the position
where the value will appear; the escaper does not parse surrounding HTML.

| `HtmlContext` | Escapes | Intended position |
| --- | --- | --- |
| `Text` | `&`, `<`, `>` | Ordinary HTML text children |
| `DoubleQuotedAttribute` | `&`, `"` | Inside a double-quoted ordinary attribute value |
| `SingleQuotedAttribute` | `&`, `'` | Inside a single-quoted ordinary attribute value |
| `TextAndAttributes` | `&`, `<`, `>`, `"`, `'` | The default `render` policy and the `escape` filter |

Entities are `&amp;`, `&lt;`, `&gt;`, `&quot;` and `&#x27;`, respectively.
Other UTF-8 bytes are preserved. The attribute modes do not add delimiters;
keep the matching quotes around the complete attribute value. A text-escaped
value can still contain quotes and is unsuitable for an attribute without
escaping its original input for that attribute's context.

```plenty
from std.template import escape, HtmlContext, TemplateError

def main() -> Result[(), TemplateError | AllocError | IoError]:
    name = "Ada & \"friends\""
    title = escape(&name, HtmlContext.DoubleQuotedAttribute, max_bytes=128)?
    child = escape(&name, HtmlContext.Text, max_bytes=128)?
    print(f"<span title=\"{title}\">{child}</span>"?)?
    Ok(())
```
```output
<span title="Ada &amp; &quot;friends&quot;">Ada &amp; "friends"</span>
```

The input is borrowed and never modified; success returns ordinary owned text,
with no trust marker. Passing that text through `Json.Text` into `render`
applies normal escaping again. Prefer raw input for template values and these
helpers for directly assembled fragments. `safe` remains an explicit trust
assertion by the template author; it does not check an escaping context.

Escaping treats the input as text, including existing entities: `&amp;` becomes
`&amp;amp;`. Call it once on the original value for the intended context;
repeated calls are not idempotent. For registered templates, choose the
[per-placeholder context filters](#escaping-and-inheritance) on raw values
instead of passing this helper's encoded output back through the engine.

`max_bytes` caps UTF-8 output, including entity expansion. Oversized input is
rejected before allocation because escaping cannot shrink it. Each input byte
is scanned once; replacement entities are appended directly and are not
revisited. The output cap therefore also bounds source bytes scanned and
copied. The function may allocate source/output buffers and returned text;
allocation failures are `TemplateError.Allocation` or an allocation cause
inside `TemplateError.Bytes`. Exceeding the cap returns `LimitExceeded`, with
no partial output. A zero cap accepts only empty text; a dynamic cap uses
`NonNegative.try_from(value)?`.

These contexts do not support HTML comments, tag/attribute names, unquoted
attributes, scripts, styles or URL validation. Choosing an enum value is
explicit caller responsibility, not proof that its surrounding HTML is valid.

### Inheritance

`extends` must precede every non-whitespace, non-comment construct. An extending
template contains only whitespace, comments and top-level blocks. Blocks cannot
be nested or repeated within a template. The most-derived definition replaces
its ancestor's entire block; no `super()` call is provided. Blocks absent from
the base's layout are not rendered. Parent names are literal registry names,
not filesystem paths. Missing parents and inheritance cycles are render errors.

### Reusable fragments

`{% include 'header' %}` renders another registered template with the current
object context. `{% include 'form' with row %}` instead uses the object selected
by `row`; the expression is evaluated in the caller before entering the fragment.
The context expression must produce an object. The name is a quoted literal,
not a filename or a computed path, and no filesystem access occurs.

An include has its own loop scope and inheritance overrides. Pass an object
explicitly to use a caller's loop variable; the fragment does not inherit the
caller's loop locals or `loop` metadata. Its own loops work normally, and the
caller regains its original root and loop metadata afterward. A fragment may
inherit from another registered template. Inheritance and inclusion cycles
return `InheritanceCycle`; missing names return `UnknownTemplate`.

Fragments render directly into the same output with normal escaping. Keep raw
values in their contexts: composition requires neither pre-escaping nor `safe`.
Nested calls share the original context index and all render budgets; each
invocation also charges inheritance resolution and cycle checks to `max_steps`.
Include and inheritance nesting contribute to the same `max_depth` bound.
Errors return no partial output and leave the engine and input context reusable.
Parameterized macros and arbitrary function calls remain unsupported.

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

| Field | Type | Default | Bounds |
| --- | --- | ---: | --- |
| `max_source` | `NonNegative[i64]` | 1,048,576 | Total UTF-8 bytes of registered names and sources |
| `max_tokens` | `NonNegative[i64]` | 16,384 | Total compiled tokens across templates |
| `max_expression_nodes` | `NonNegative[i64]` | 32,768 | Total expression AST nodes across registered templates |
| `max_templates` | `NonNegative[i64]` | 128 | Registered template count |
| `max_context_nodes` | `Positive[i64]` | 65,536 | Values in a render context, including its root |
| `max_context_bytes` | `NonNegative[i64]` | 16,777,216 | UTF-8 bytes of context keys and text values |
| `max_depth` | `Positive[i64]` | 64 | Syntax/context/render nesting, expression tree/grouping depth and inheritance chain; at most 64 |
| `max_steps` | `NonNegative[i64]` | 1,000,000 | Render operations, including context construction, visited expression nodes/filters, lookup and loop visits |
| `max_output` | `NonNegative[i64]` | 16,777,216 | UTF-8 output bytes and transformed text values |

A literal converts where a field is expected; a computed value needs
`Positive.try_from(value)?` or `NonNegative.try_from(value)?`.
`engine(&bounds)` traps when `max_depth` is above 64 and copies the settings. `add` compiles and validates before
registering; a failure leaves the engine unchanged. Names cannot be empty or
registered twice. A render failure returns no partial output and leaves the
engine reusable. Limits bound sizes and work counts, not an exact heap-byte
quota or wall-clock deadline. Budget totals remain exact: an unrepresentable
source, token, expression-node, context-byte or work total returns `TemplateError.LimitExceeded`,
without wrapping or saturating. Registration commits its totals only after the
compiled template is appended successfully. Expression nodes have their own aggregate
cap; adding an operator does not change the meaning of `max_tokens`. Its default
is twice the default token cap to preserve the full legacy capacity of 16,384 adjacent
negated placeholders: each now needs a value node and a `not` node. Custom token
and expression-node limits remain independent; raising one does not raise the
other. Parser
recursion and expression-tree depth are bounded by `max_depth`, while render
steps charge only evaluated nodes and filters. Registration failures, including
allocation failures, consume none of the engine's node budget. Text filters may
allocate intermediate text.

`TemplateError` distinguishes exceeded limits, syntax with a
zero-based source location, missing/duplicate template names, missing values,
type mismatches, cycles, allocation failures and byte-buffer errors. Syntax
locations are UTF-8 byte offsets at the containing template construct, rather
than at an individual expression character. Allocation errors remain explicit, including those
wrapped by `BytesError`. Declared-shape preflight additionally reports
`InvalidShape`, `SchemaPath`, `SchemaOptional`, `SchemaType` and `ContextShape`;
see [their guarantees and locations](template-schema.md).
Checked HTML additionally reports
`InvalidHtml(name, offset)` with the defining registered source and an original
UTF-8 byte position; see [its contract](template-html.md#composition-and-failure).

## Acknowledgements

- Syntax is a subset of [Jinja](https://jinja.palletsprojects.com/), with [Tera](https://keats.github.io/tera/) as the syntax and behaviour reference.

- [WHATWG HTML syntax](https://html.spec.whatwg.org/multipage/syntax.html) supplies
  the checked HTML grammar vocabulary; the restricted recognizer is independently
  implemented and bundles no specification code or data.
