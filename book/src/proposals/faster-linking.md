# Faster linking

> **Status: Not implemented.** This page records a measured study of what the
> compiler can do to make linking faster, and the levers it recommends.
> [Linking](../design/toolchain/01-building/linking.md) and the
> [compilation cache](../design/toolchain/02-architecture/cache.md) own the
> current contract. Compile and link latency is tracked under
> [B19](../backlog.md); no levers here are scheduled.

## Summary

Linking costs a near-constant 18–19 ms per build, whatever the program's size.
That is about 86% of a warm `--compile` of hello world, 27% of an edit loop on
`examples/semantic_search`, and 7% of the `compile_bench` 1000-unit workload.

Plenty's own object adds only 0–2 ms of that, even at 10,000 sections. The
remaining cost is in how the compiler drives the linker and supplies the
runtime: the `cc` driver chain, writing the runtime archive on every link,
reading that archive, and the linker's own startup. Changing the object's
format does not pay; changing the link procedure does.

## Recommended levers

Measured (M) or estimated (E). Times are hyperfine medians pinned to cores 0–7
on x86-64 Fedora with gcc 16.2, mold 2.40.4, LLD 23.1.1, GNU ld 2.46.1 and wild
0.11.0. Differences under about 1 ms are noise.

| # | Lever | Saving | Cost |
|---|---|---|---|
| 1 | Cache the linked executable next to the cached object. On a whole-program cache hit, run or copy the stored executable. | E: unchanged hello rerun 20.5 → about 2.5 ms (−88%); `semantic_search` 23.5 → about 5 ms (−80%). Only unchanged reruns benefit. | Low. Extends the whole-object cache (issue #317 stage 1). |
| 2 | Run the linker directly instead of through `cc`. Run `cc -###` once during the existing linker probe, store the resolved command, then run `ld.mold` or `ld.lld` with it. | M: −3.5 ms per link. The `cc` → `collect2` → linker chain adds 3–4.5 ms, including about 1.5 ms loading the LTO plugin. Clang as `cc` costs 12 ms more than gcc. | Medium. The replayed command must match the driver's. Fall back to `cc` when parsing fails or `--linker`/`--link-arg` is given. macOS and AArch64 need their own replay. |
| 3 | Extract the 17 MB runtime archive once into the cache directory, keyed by compiler build, instead of writing it to a temporary directory on every link. | M: −2 ms per link; peak memory −16 MB (hello 28 → 11.6 MB). | Low. Keep temporary extraction when the cache is off. |
| 4 | Strip debug sections and embedded LLVM bitcode from the embedded runtime archive at compiler build time. | M: archive 17.4 → 9.9 MB; compiler binary −7.5 MB. No link-time change. | Low. `--no-strip` output loses runtime debug information unless an unstripped archive is kept for it. |
| 5 | Link run-only builds (`plenty FILE`, `plenty test`) against a shared runtime library. Not for `--compile`. | M: mold link 12.5 → 8.5 ms (hello), 14.3 → 9.3 ms (`semantic_search`); each run starts 0.3 ms slower. | Medium to high. A second runtime artifact, and run-only executables link differently from shipped ones. Only after 1–3. |
| 6 | Track wild as a future linker. | M: direct link 5.4 / 7.6 / 6.1 ms against mold's 12.5 / 14.3 / 13.7 (hello / `semantic_search` / bench1000). | Not ready: its `--defsym` cannot pull archive members, and it removes far less dead code (hello 393 KB against 116 KB), failing size budgets. |

Levers 2 and 3 were prototyped together (M):

| Program | Warm `--compile` | Edit loop |
|---|---:|---:|
| hello | 20.5 → 15.2 ms (−26%) | – |
| `actor_echo` | – | 55 → 47.7 ms (−13%) |
| `semantic_search` | 23.5 → 17.8 ms (−24%) | 69 → 62 ms (−10%) |
| bench1000 | 26.5 → 22.2 ms (−16%) | 263 → 257 ms (−2.5%) |

After 2 and 3, mold still takes about 11 ms for hello: about 5 ms is the cost
of linking an empty C program (crt files, libc, libm, libgcc_s), about 4 ms is
process startup and its thread pool, and about 6 ms is the runtime archive
(419 members, 24,632 sections). Only levers 5 and 6 reduce that further.

## Measurements

End to end (M, ms). The edit loop disables the whole-program cache and keeps
the function cache warm.

| Program | Emit object | Warm `--compile` | Edit loop | Cache off | Link share of edit loop |
|---|---:|---:|---:|---:|---:|
| hello | 2.4 | 21.3 | 22.2 | 28.4 | 86% |
| `actor_echo` | 3.6 | 21.9 | 52.6 | 66.7 | 35% |
| `semantic_search` | 4.7 | 23.9 | 70.8 | 91.1 | 27% |
| `http_server_bench` | 4.6 | 22.7 | 82.4 | 118.9 | 22% |
| bench1000 | 7.9 | 27.3 | 261.7 | 345.1 | 7% |

Link only, by linker (M, ms). The first three go through `cc -fuse-ld=X`, as
Plenty does today; the rest run the linker directly.

| Program | bfd (cc) | mold (cc) | lld (cc) | bfd | mold | lld | wild |
|---|---:|---:|---:|---:|---:|---:|---:|
| empty C `main` | 14.0 | 11.8 | – | 18.5 | 9.0 | 8.4 | 4.3 |
| hello | 58.8 | 15.6 | 16.4 | 50.6 | 12.4 | 13.0 | 5.4 |
| `semantic_search` | 113.3 | 17.9 | 20.1 | 110.3 | 15.4 | 17.3 | 7.6 |
| bench1000 | 66.4 | 16.2 | 17.5 | 59.4 | 14.2 | 15.5 | 6.1 |

On one core, for parallel `plenty test` runs (M): mold 16.9, lld 16.3, wild 8.9
and bfd 46.9 ms per hello link. Unpinned, mold uses 58 ms of CPU for 12 ms of
wall time, which supports setting `PLENTY_LINKER_THREADS=1` for parallel runs.

What the linker receives (M):

| | hello.o | semantic_search.o | bench1000.o |
|---|---:|---:|---:|
| Size | 12.7 KB | 898 KB | 3.9 MB |
| Sections | 75 | 2,042 | 10,052 |
| `R_X86_64_PLT32` | 4 | 6,558 | 16,005 |
| `R_X86_64_PC32` | 4 | 3,113 | 9,012 |
| `R_X86_64_64` | 47 | 984 | 26 |
| `R_X86_64_GOTPCREL` | 0 | 12 | 0 |

The objects have no COMDAT groups, mergeable sections, debug information or
`.eh_frame`. The runtime archive is 17.4 MB in 419 members: 5.8 MB of debug
information and its relocations, 1.7 MB of embedded bitcode, 3.4 MB of code,
1.0 MB of read-only data and 0.3 MB of `.eh_frame`. Executables are not
position-independent on this system and carry 139 dynamic relocations.

mold's phases for hello (M, ms): 11 in total; reading inputs 5; symbol work
before copying 5 (merging strings 2–3, dead-code removal 0, relocation scan
0–1); creating the output 1; copying 0.

## Levers that do not pay

| Lever | Result (M unless marked) |
|---|---|
| Linker flags (`--build-id=none`, `-z norelro`, no `--eh-frame-hdr`) | ±0.5 ms |
| Static linking | 10 ms slower: it pulls in static glibc |
| Pre-linking the runtime into one object with `ld -r`, stripped or not | 0.5–1.5 ms slower: section count, not member count, is the cost |
| Removing duplicate `-l` stub libraries | No change |
| Linker thread counts | 4 ≈ 8 ≈ 16 threads for one link; the default is fine |
| PIE instead of non-PIE | 3,000 more relocations, 0.3 ms; default-PIE distributions are no slower |
| Removing unreachable code in the compiler | At most 1 ms of link time. Still worth doing for compile time: most of bench1000's 262 ms edit loop compiles functions nothing calls |
| Several objects for parallel linkers | E: none; mold and lld already parallelize within one object |

## Common object-format suggestions

These ideas help some toolchains; none applies to Plenty's output.

- **Deduplicating types before writing the object.** In C++ the linker merges
  repeated COMDAT groups and debug types from many translation units. Plenty
  compiles one unit and emits each generic instance once, so its objects have no
  COMDAT groups, debug types or duplicate symbols. Identical data sections total
  10 KB in `semantic_search`. String merging time comes from the runtime's
  members. E: under 0.1 ms.
- **Page-aligned sections.** bfd, lld, mold and wild all copy each input section
  into the output and patch it; none maps input sections directly, so input
  alignment has no effect. Larger alignment would only pad the output.
- **Fewer, larger sections.** Prototyped with one text section per object: link
  time changed by ±1 ms and objects were 15–17% smaller, but `--gc-sections`
  could no longer drop dead code, so bench1000's executable grew from 341 KB to
  2.39 MB. Keep one section per function. The runtime's per-operation symbols
  (issue #501) are in the archive and unaffected.
- **Pre-computed relocations.** Runtime calls are already direct (issue #372),
  executables are not position-independent here, and the relocation scan takes
  0–1 ms for 25,000 relocations. Resolving calls at emit time would need one
  section, losing dead-code removal. E: under 1 ms.
- **Dependency hints for parallel linkers.** No ELF linker reads such hints and
  ELF has no convention for them. lld's `--symbol-ordering-file` and mold's
  `--section-order` control output layout, not link speed.

## Reproducibility and portability

B17 requires reproducible output.

- Levers 1 and 2 leave output bytes unchanged if the replayed command is
  identical; an object-identity test should check it. The cache key must include
  the link command and linker identity, which the linker probe already records.
  Replaying `cc -###` captures distribution differences such as multiarch paths
  and default PIE instead of hard-coding them.
- Levers 3 and 4 do not change stripped executables. Lever 4 removes runtime
  debug information from `--no-strip` output.
- Levers 5 and 6 change output bytes. They suit run-only builds, or a separately
  pinned linker input.
- macOS (ld64) and AArch64 need their own versions of lever 2, with `cc` as the
  fallback.
