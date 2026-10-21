# Inspecting generated code

`plenty --dump STAGES FILE -o DIR` writes what the compiler produced for each
function as plain text, for reading, `grep`, and `diff`. It compiles the
program as `--emit-object` would, with no object written and nothing linked.

```text
plenty --dump all app.plenty -o dump/
plenty --release --dump clif-opt,asm --dump-filter parse app.plenty -o dump/
plenty --dump asm --dump-std app.plenty -o - | less
```

| Option | Effect |
|---|---|
| `STAGES` | `all`, or a comma list of `ops`, `clif`, `clif-opt`, `asm` |
| `-o DIR` | Write `index.txt` and one `STAGE.txt` per stage into `DIR`, creating it; stage files from an earlier dump that this one does not write are removed |
| `-o -` | Print the files to standard output, each after a `#### NAME` line |
| `--release` | Dump the code a release build produces |
| `--dump-std` | Include standard-library functions; by default only the program's own modules are dumped |
| `--dump-filter TEXT` | Dump only functions whose name or symbol contains `TEXT` |
| `--module-root DIR` | Resolve imports as for the other file commands |

## Stages

| Stage | Contents |
|---|---|
| `ops` | The typed operation IR the [frontend](frontend.md) hands to code generation: each local slot's type, then one operation per line, with nested bodies indented |
| `clif` | Cranelift IR as lowered from the operations, before Cranelift's passes |
| `clif-opt` | Cranelift IR after its passes, as compiled to machine code |
| `asm` | x86-64 machine code in AT&T syntax, with byte offsets |

Without `--release`, Cranelift runs at `opt_level=none`, so `clif-opt` shows
legalized rather than optimized IR; the [native emission](emission.md) page
describes what optimization does. Operations are printed with the compiler's
own debug form and change with it; they are for comparison, not parsing.

## Files

Every stage file holds one section per function, in the order the compiler
places functions in the object, which does not depend on the worker count:

```text
==== twice  symbol=__plenty_fn_twice  kind=function  source=app.plenty:1
     0:  push    %rbp
     1:  mov     %rsp,%rbp
     4:  mov     %rdi,%rax
    # line 2
     7:  imulq   0x30
     e:  seto    %sil
    12:  test    %sil,%sil
    15:  jne     0x0000000000000020
    1b:  mov     %rbp,%rsp
    1e:  pop     %rbp
    1f:  ret
    20:  lea     0x27,%rdi  # .Ldata1 (trap sites: 1)
    27:  call    0x000000000000002C  # plenty_trap_overflow
    2c:  ud2
    2e:  .byte   0x00,0x00,0x02,0x00,0x00,0x00,0x00,0x00,0x00,0x00
```

The header names the function as source shows it (`Point.length`,
`first[i64]`, `main.<closure@26:11>` for a closure at line 26, column 11 of
`main`), its object symbol, its kind, and where it is defined. Kinds are
`function`, `drop` and `resume` (a class's destructor and a generator's
resume function, keyed `NAME@drop` and `NAME@resume`), `entry`
(`plenty_main`), and `adapter` (thread, executor and sort trampolines). Keys are unique; a repeated name gets
a `#N` suffix.

- `clif` and `clif-opt` mark each change of source line with `; line N`, and
  name each external function and data symbol on its declaration line.
- `asm` marks source lines with `# line N`, names the symbol each relocated
  operand refers to, and shows the constants Cranelift places after the code
  as `.byte` rows. RIP-relative addresses are offsets within the function.
- Data symbols carry what they hold: a string literal's text, a type's
  metadata, or the function's trap-site table.

`index.txt` starts with the compiler version, target, optimization level,
stages, scope, and filter, then lists one row per dumped function, largest
machine code first:

```text
#  bytes    ops   clif    opt kind     lib  source                           symbol key
      56      6      7      7 function user app.plenty:1                     __plenty_fn_twice twice
```

`ops` counts operations, including nested ones, and `clif` and `opt` count
Cranelift instructions before and after its passes; adapters have no
operations and show `-`.
`lib` is `user` or `std`.

## Guarantees and limits

- The output is deterministic: the same compiler and source produce the same
  text for any worker count and cache state. Source files appear relative to
  the module root, so the checkout's location does not matter.
- `--dump` bypasses the [compile cache](cache.md) and the program cache: every
  function is compiled and nothing is stored. Source line markers are part of
  the function's cache key, so ordinary builds never carry them; they do not
  change the machine code.
- Line markers carry a line, not a column. Code Cranelift moves or merges
  keeps the line of the operation it came from.
- Offsets and data symbol numbers shift when unrelated code changes, which adds
  noise to a `diff` of `asm` between revisions; diff `clif-opt` or compare the
  index first.
- Only x86-64 Linux is supported, as for compilation generally.

The [compiler phase profiler](../../../development/profiling.md) measures
where compile time goes; `--dump` shows what that time produced.
