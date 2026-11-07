# Declared template context shapes

`Engine.validate(name, schema)` checks a named template against a declared
`std.template.Shape`, without sample data. It visits every reachable branch,
loop body and empty arm, including paths that a particular request would not
execute. `Engine.add` still checks syntax alone, and ordinary `render` retains
its dynamic JSON behavior. Validation happens at runtime; registering a runtime
string does not give a Plenty compiler guarantee.

```plenty
import std.template as template
from std.template import Shape, Field, Presence, TemplateError
from std.json import Json, Member

def main() -> Result[(), TemplateError | AllocError | IoError]:
    bounds = template.limits()
    mut pages = template.engine(&bounds)?
    pages.add("greeting", "Hello {{ user.display_name }}!")?
    schema = Shape.Object([
        Field("user", Shape.Object([
            Field("display_name", Shape.Text, Presence.Required),
        ]?), Presence.Required),
    ]?)
    pages.validate("greeting", &schema)?
    values = Json.Object([
        Member("user", Json.Object([
            Member("display_name", Json.Text("Ada & Grace")),
        ]?)),
    ]?)
    print(pages.render_checked("greeting", &values, &schema)?)?
    Ok(())
```
```output
Hello Ada &amp; Grace!
```

A typo such as `user.dispay_name`, even inside `if false`, returns
`SchemaPath(template_name, original_byte_offset, full_dotted_path)`. The offset
points to the opening template tag, counting the original UTF-8 bytes before
[whitespace trimming](template.md#whitespace-control).

## Shape declarations

| Declaration | Accepted JSON values |
| --- | --- |
| `Shape.Null`, `Bool`, `Int`, `Float`, `Text` | The corresponding exact JSON kind; integers do not widen to floats |
| `Shape.Array(Box(element)?, minimum_length)` | Arrays whose every element matches `element`, with at least that many entries |
| `Shape.Object(fields)` | Objects containing every required field; extra keys are allowed |
| `Shape.OneOf(alternatives)` | Values matching at least one alternative; an empty union is invalid |

Each `Field(name, shape, presence)` owns its declaration. `Presence.Required`
requires the key even if its value may be null; `Presence.Optional` permits an
absent key but checks its value when present. Duplicate field names are invalid.
The root must be an object, or a union whose every alternative is an object.
Constructing lists and boxes is explicitly fallible; validation borrows the
schema and never retains or modifies it.

For a field that templates should read, prefer a required nullable value:
`Field("name", Shape.OneOf([Shape.Null, Shape.Text]?), Presence.Required)`.
Null interpolates as empty text. A read of an optional field returns
`SchemaOptional(template_name, offset, path)`, including in conditions, because
a missing field is an error during rendering. Unused optional fields may be
absent. There is currently no flow refinement: a truthiness guard does not make
an optional field present or turn a nullable object into a guaranteed object.

## Paths, types and composition

Every possible union alternative must support each path and operation. Static
validation checks every operand, even an operand that runtime short-circuiting
would skip. It does not infer correlations between variant tags and fields.
A union of objects can expose common required fields; variant-specific reads
need a different declaration/template rather than an assumed discriminant.

Text filters require text in every alternative; `length` accepts text, arrays
and objects. Direct interpolation requires null or another scalar kind.
Conditions accept the ordinary template truthiness rules. Arrays are the only
loop sources, and the body is checked against their declared element shape
even when the minimum length is zero. Dotted index `items.2` requires a declared
minimum length of at least three, ensuring the index exists in every conforming
request. `SchemaType(template_name, offset)` reports an incompatible operation.

Loop variables shadow outer fields; nested loops use the innermost `loop`
metadata. An empty arm sees the restored outer scope. Includes validate their
actual selected object root, with caller loop locals hidden; inherited layouts
use their effective most-derived blocks. Replaced base bodies and unused child
blocks are ignored. Every branch of an effective body is checked, including
unknown includes and composition cycles that a sample render might miss.

## Checking requests and resource limits

`render_checked(name, context, schema, html=True)` repeats preflight, verifies the
actual request against the declaration, then renders with ordinary escaping
and output limits. `ContextShape(path)` identifies a missing or incompatible
request location, beginning with `$`; a failed union reports its union location.
Extra keys are allowed, but the entire JSON tree still obeys ordinary duplicate
key, finite-float, depth, node and byte checks. Allocation and limit failures
propagate even when testing a union alternative. No partial output is returned.

A successful `validate` establishes declared path and operation compatibility,
not that future data conforms, output fits, allocations succeed or limits will
not be reached. Use `render_checked` when receiving dynamic JSON. There is no
validation cache to become stale when a previously unknown dependency is added.

Schema nodes, field-name bytes and depth use `max_context_nodes`,
`max_context_bytes` and `max_depth`, independently from the actual request tree.
Nested unions count as nodes and depth too. Their flattening, candidate scans,
path/type checks, template traversal and actual-data matching charge
`max_steps`. Checked rendering shares that work budget across preflight,
request flattening, matching and output generation. Candidate sets contain
indexed alternatives; validation does not construct products of union values.
The default caps and other errors are in the [template contract](template.md#values-limits-and-errors).
