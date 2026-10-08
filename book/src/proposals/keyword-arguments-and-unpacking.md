# Keyword arguments, defaults, and static unpacking

Python's keyword arguments, defaults, and `*`/`**` unpacking make calls and
literals easier to read and write. Plenty adopts them only where the compiler
can resolve them completely. This is not a compatibility goal: a Python form
that needs runtime arity checks, runtime key lookup, dynamic typing, or hidden
allocation is rejected, not approximated.

In this role `*` and `**` are unpacking syntax, not operators. Binary `a * b`
and `a ** b` keep their arithmetic meaning.

## Rules every form must satisfy

- **Static resolution.** Every call is rewritten during checking into one
  positional call to a known declaration. Codegen and the runtime never see
  names, defaults, or unpacking at call sites.
- **No runtime call failure.** Arity, names, and types are checked at compile
  time. No form can fail at run time because a sequence has the wrong length or
  a mapping lacks a key.
- **No hidden allocation.** A form that allocates already has a `Result` type
  at that site (a collection or tuple display). A call never allocates to pass
  its arguments.
- **One rule, no exceptions.** Each form has one meaning, and the conditions for
  accepting it can be stated in a sentence.

## Keyword arguments

Any parameter of a named function, method, generic function, or class
constructor can be passed by name unless it is positional-only:

```text
def scale(value: i64, factor: i64, offset: i64) -> i64:
    value * factor + offset

scale(3, offset=1, factor=2)
```

- Positional arguments come before keyword arguments.
- Each parameter is bound exactly once. Unknown names and duplicates are errors.
- Arguments are evaluated and moved left to right **in source order**, as in
  Python, not in parameter order. A call's effects follow the text.
- Expected-type inference works as it does for positional arguments:
  `factor=2` against `factor: u8` produces a `u8`.

Parameter names become part of a declaration's public interface: renaming a
`pub` function's parameter can break callers. Positional-only parameters (below)
let an author opt out.

**Function values stay positional.** `Callable[[A], B]` has no parameter names,
and this proposal does not add them. A call through a function value accepts
positional arguments only. A keyword call needs a statically known declaration.

**Generated constructors** take field names as keywords:
`Point(x=3, y=4)?`. This needs no new mechanism, since the generated
constructor's parameters are the fields.

**Builtins** follow the same rules. Existing special cases, such as
`splitlines(keepends=...)`, become ordinary signatures with a keyword and a
default.

**Protocol requirements.** The reference says implementation parameter names
may differ from the protocol's. Keep that. A generic body checks keyword calls
against the protocol requirement's names and markers, then reaches the
implementation positionally. Implementation names never matter through a bound.

## Positional-only and keyword-only markers

Python's `/` and bare `*` markers are adopted unchanged:

```text
def clamp(value: i64, /, low: i64, high: i64, *, wrap: bool) -> i64:
    ...
```

Parameters before `/` are positional-only. Parameters after `*` are
keyword-only. Keyword-only parameters suit Boolean flags and options, where a
bare positional `True` hides intent.

## Default values

```text
def split(text: &str, separator: &str = ",", limit: i64 = -1) -> Result[list[str], AllocError]:
```

A default is an **allocation-free constant expression**: a numeric, `bool`, or
`str` literal, unit, `Nothing`, or a nullary enum variant that needs no
allocation. A default cannot reference other parameters, globals, or calls.
Inside these limits, the moment of evaluation (definition time or call time) has
no visible effect, so Python's shared mutable default problem cannot occur.

- A positional parameter with a default cannot be followed by a positional
  parameter without one. Keyword-only parameters can appear in any order.
- A parameter whose type mentions a type parameter cannot have a default. A
  default must check against a concrete type, never per specialization.
- The compiler inserts omitted defaults during the positional rewrite.

Class field defaults use the same constant rule and become defaults of the
generated constructor. They are currently deferred in the class reference and
can land with this work or later.

## Unpacking at call sites

**`*tuple`** expands a tuple value into positional arguments:

```text
point = (3, 4)?
distance(*point)
```

The operand's static type must be a tuple, so its arity and component types are
known. The tuple is evaluated once at its source position and consumed like a
whole-tuple unpacking. Several starred tuples may appear in one call. No storage
is allocated.

**`**record`** expands a class value into keyword arguments named by its fields:

```text
def draw(x: i64, y: i64, *, color: Color) -> ():

draw(**origin, color=Color.Red)
```

- The operand's static type must be a class. Its visible fields must each match
  a parameter name. An extra field is an error, not an ignored value.
- The record is consumed and its fields move into the call. Moving fields out
  of a class needs a whole-value destructuring rule that the language does not
  have yet. As a hard rule, `**` rejects classes with a custom `__del__`, since
  their fields cannot be separated from the destructor.
- Remaining parameters come from other arguments or defaults.

**Rejected:** `*list`, `*generator`, or any operand whose length is not in its
type, used for fixed parameters. `**dict` in any position. Both need runtime
checks that could fail during the call.

## Variadic parameters: `*args: T`

```text
def total(*values: i64) -> i64:
    mut sum = 0
    for value in values:
        sum = sum + value
    sum

total(1, 2, 3)
```

The extra arguments must all have one element type `T`. At each call site the
count is known, so the caller places the evaluated arguments in a stack buffer
in its own frame and passes a pointer and length. Inside the body, `values` is a
read-only borrowed view of that buffer. The caller drops the elements after the
call returns. No heap allocation is involved.

- Generic `*values: T` infers `T` from all extra arguments. Under the existing
  inference rule, all evidence must agree.
- A list can supply the whole variadic group, `total(*numbers)`, by passing a
  borrowed view of its storage. Mixing it with other extra arguments, as in
  `total(1, *numbers)`, is rejected because it would need a buffer of unknown
  size.
- Heterogeneous variadics (`*args` of mixed types) need variadic generics and
  are out of scope.
- Taking a variadic function as a `Callable` value is deferred until the view
  type has a stable spelling for callable signatures.
- `*args` has no connection to C varargs, which stay excluded from
  `extern def`.

**Dependency:** a borrowed sequence view type. Plenty has no slice or span type
yet, and a view should be designed for its own sake (string, list, and buffer
views) before `*args` relies on it. A consuming variant, where the callee owns
the elements, can follow once owned views exist.

## No `**kwargs` parameters

Python's `**kwargs` is a `dict[str, Any]`. It needs dynamic typing and allocates
a dictionary on each call. Plenty does not add it.

Its main uses already have static alternatives:

- **Option bags:** use keyword-only parameters with defaults, or take an
  explicit options class with field defaults: `render(page, opts=Options(dense=True)?)`.
  The allocation stays visible.
- **Forwarding:** a wrapper takes `opts: Options` and passes `**opts` to the
  inner call.

A future "keyword group" parameter that exposes a class's fields as keyword-only
parameters without constructing the class is possible. Defer it until a concrete
forwarding need shows that the alternatives above are insufficient.

## Unpacking in displays

Collection and tuple displays already return `Result`, so spreading into them
adds no new failure path:

```text
both = [*first, *second]?
members = {*left, *right}?
merged = {**defaults, **overrides}?
extended = (*pair, 3)?
```

- In a list or set display, `*source` accepts the owned sources that
  `list[T].from` accepts: a collection, range, or generator. The source is
  consumed. Use `copy(source)?` to keep the input, as `update` already requires.
- In a dict display, `**source` accepts only an owned `dict[K, V]` of the same
  type. Later keys replace earlier ones, matching `dict.update`.
- In a tuple display, `*source` accepts only a tuple, so the result arity is
  static.
- Construction stays left to right. The first failure releases the initialized
  prefix and drops evaluated sources not yet transferred; later entries are not
  evaluated. Implementations may sum
  known source lengths to reserve once.
- `return *head, last` and `yield *head, last` are tuple displays and follow the
  tuple rule.

## Starred targets in bindings

`first, *_ = triple` ignores the remaining tuple components and drops their
owners. It is static and allocation-free, so it is accepted.

`first, *rest = triple` with a named `rest` is **rejected for now**. Tuples
currently use heap records, so `rest` would be a new allocated tuple inside a
binding statement that has no place for `?`. If tuples later get an inline
representation, this becomes allocation-free and can be reconsidered.

`head, *tail = items` on a list is rejected permanently as a binding: the list's
length is not in its type. The checked form is a sequence pattern, where a length
mismatch is an explicit branch, not a hidden failure:

```text
match items:
    case [head, *tail]:
        ...
    case []:
        ...
```

`tail` would be a borrowed view. This depends on the same view type as `*args`
and on sequence patterns, which `match` does not support yet. Mapping patterns
with `**rest` are rejected.

## Rejected Python forms

| Form | Reason |
| --- | --- |
| `**kwargs` parameter | Dynamic typing and a per-call dictionary allocation |
| `f(**mapping)` with a dict | Keys are checked only at run time |
| `f(*items)` into fixed parameters, for an operand whose length is not in its type | Arity is checked only at run time |
| `f(1, *items)` into `*args` | Needs a buffer of unknown size |
| Mixed-type `*args` | Needs variadic generics |
| Defaults that are arbitrary expressions | Hidden allocation and effects; Python's evaluation-time trap |
| `head, *tail = items` on a list | Runtime length check in a statement |
| `case {**rest}` mapping patterns | Runtime key sets |
| Keyword calls through `Callable` values | Callable types carry no parameter names |

## Suggested stages

1. Keyword arguments, `/` and `*` markers, constant defaults, keyword
   constructors, and builtin special cases moved onto ordinary signatures. This
   stage is frontend-only.
2. `*tuple` and `**record` at call sites, and `*_` in tuple bindings.
3. Spreads in list, set, dict, and tuple displays.
4. A borrowed sequence view type, then `*args: T`.
5. Sequence patterns with `*rest`, after views and sequence `match`.

Each stage updates the function reference, the tutorial lessons that teach
calls, and its backlog entry.

## Open questions

- The spelling of the view type that `*args` and `*rest` bind.
- Whether `**record` should accept classes with `__del__` once a whole-value
  destructuring rule exists.
- Whether protocol requirements may declare defaults, or only names and markers.
- Whether class field defaults ship in stage 1 or wait for a separate change.
