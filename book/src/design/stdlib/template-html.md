# Checked HTML templates

`Engine.validate_html(name)` checks a registered template's effective layout and
fragments without a JSON context. `Engine.render_html(name, context)` performs
that same mandatory preflight, then renders with escaping inferred from the
literal HTML position. Use this path for HTML templates that fit its deliberately
small syntax. Keep raw values in `Json.Text`.

The existing [`render` contract](template.md) is unchanged. Its explicit context
filters remain useful for trusted templates outside this subset, with position
selection left to the caller. Checked rendering is neither an HTML sanitizer nor
a full HTML5 parser: source templates and literal attribute values remain trusted.

## Supported structure

Names are static ASCII and case-insensitive. Element names are ASCII letters
followed by letters/digits; attribute names additionally permit `-` and `_`.
Unknown/custom elements are rejected. The supported elements are:

```text
a abbr address article aside audio b bdi bdo blockquote body br button caption
cite code col colgroup data datalist dd del details dfn dialog div dl dt em
fieldset figcaption figure footer form h1 h2 h3 h4 h5 h6 head header hgroup hr
html i img input ins kbd label legend li link main map mark menu meta meter nav
ol optgroup option output p picture pre progress q rp rt ruby s samp search
section select slot small source span strong sub summary sup table tbody td
tfoot th thead time title tr track u ul var video wbr
```

Every non-void element needs a matching explicit end tag. The written stack must
match exactly; optional end tags, self-closing non-void elements and mismatched
nesting are rejected. The supported void elements are `br`, `col`, `hr`, `img`,
`input`, `link`, `meta`, `source`, `track` and `wbr`; a trailing `/` is optional.
This checks the written structure, not HTML5 tree repair, element content models,
browser DOM layout or browser-specific behavior.

A static `<!DOCTYPE html>` is allowed once, before non-whitespace output and
before interpolation or a control/include/block boundary. Its letters are
case-insensitive and ASCII whitespace may separate its words. Legacy doctypes,
HTML comments, declarations, foreign content, `template`, scripts, styles,
`iframe`, `object`, `embed`, `noscript` and raw-text elements are unsupported.
Template comments (`{# ... #}`) still disappear during template registration.

`title` is the only supported RCDATA element. Its interpolations use text
escaping. A literal `<` must begin its matching closing tag; control-flow,
include and block boundaries inside `title` are rejected. `textarea` is excluded.

## Attributes and escaping

Attribute values must be quoted; an attribute without `=` is an empty literal
value. Duplicate case-insensitive names are rejected. Supported ordinary
attributes are:

```text
id class title role lang dir name value type placeholder alt for tabindex width
height disabled checked selected multiple required readonly hidden open method
enctype autocomplete min max step rows cols colspan rowspan scope datetime rel target media
```

`data-` and `aria-` names with a nonempty suffix are also supported. In these
attributes, a default-policy interpolation automatically escapes `&` and the
matching quote delimiter. In ordinary text and `title`, it escapes `&`, `<` and
`>`. Quotes remain literal in text. Literal source entities such as `&quot;` do
not terminate a quoted value; the recognizer follows source quote bytes.

`href`, `src`, `action`, `formaction`, `poster`, `cite` and `srcset` may contain
trusted **literal source only**. Interpolation there is rejected, including
interpolation with an explicit escaping filter. Literal URLs are not validated:
the caller chooses their schemes and destinations. Event attributes, `style`,
`srcdoc` and unlisted attributes are rejected entirely. No JavaScript, CSS,
URL or arbitrary attribute interpretation is inferred.

An explicit `escape_text`, `escape_double_quoted` or `escape_single_quoted`
selection must match the inferred position. Filter chains retain their ordinary
semantics: the last selector wins, `upper`/`lower`/`length` reset the selection,
and `trim` preserves it. Checked templates reject `safe` and the immediate
legacy `escape` filter syntactically, even if another filter would later reset
that trust. Every expression arm is inspected, including operands skipped by
boolean short-circuiting. Operators produce default-policy boolean values; a
selector on an operand does not select the enclosing result. Data should stay raw and be encoded once at final emission.

## Composition and failure

Every `if`/`elif` chain, `for`, include and effective inheritance block must begin and finish
in ordinary text with the **same exact element stack**. Each branch, loop body
and loop `else` is checked independently, including branches that never execute.
A balanced fragment can render inside an already open element. A fragment cannot
open an element for its caller to close, or continue an attribute begun outside
it. These restrictions make composition independent of runtime values.

Preflight follows the effective inherited layout with isolated override sets and
resolves literal includes, retaining their ordinary context/loop isolation.
Unused registered templates and overridden block bodies are not rendered or
checked. Cycles return `InheritanceCycle`; missing names return `UnknownTemplate`.
The same rules apply with [source whitespace control](template.md#whitespace-control).

Invalid or unsupported structure/policy returns `InvalidHtml(name, offset)`.
`name` identifies the defining registered source; `offset` is an original UTF-8
byte position, retaining removed whitespace and trim-marker bytes. Rendering
returns no partial text. Validation/rendering borrow the engine and context;
allocation, budget, value or preflight failures leave them reusable.

All existing limits still apply. HTML nesting uses `max_depth`; names and static
doctype declarations are capped at 64 ASCII bytes, and each start tag at 128
attributes. Preflight traversals,
filter inspection and literal source bytes consume `max_steps`. `render_html`
shares that spent work with context indexing, execution and a second literal
scan for emission positions. `max_output` includes final entity expansion.
Oversized names/attribute counts, depth or work return `LimitExceeded`.
Allocations propagate through the existing `Allocation`/`Bytes` errors.

## Acknowledgements

- [WHATWG HTML syntax](https://html.spec.whatwg.org/multipage/syntax.html) supplies
  the grammar vocabulary and quote/void-element/RCDATA distinctions. This is an
  independent restricted recognizer; no specification code or data is bundled.
