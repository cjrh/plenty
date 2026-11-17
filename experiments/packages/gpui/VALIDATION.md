# Unified executable qualification (2026-11-17)

Fedora 44, Linux GNU x86-64, Rust 1.100.0, Plenty 0.8.32 from this branch,
GPUI **0.2.2**. The original approved desktop smoke used GNOME/Wayland with
a real Xwayland window at scale 1; the fixture removed `WAYLAND_DISPLAY`. Renderer:
AMD Radeon 780M Graphics / RADV PHOENIX / Mesa 26.2.6,
`is_software_emulated: false`. No test renderer or virtual desktop substituted.
Artifacts and detailed logs stay in ignored `target/` and `/tmp/gpui-*`.

## Post-review checked-callback correction (issue #993)

Independent review rejected candidate `672bccdb` because the retained startup
trap table rooted otherwise-unused checked callback/destructor bodies. An unused
callback could therefore require an unavailable native import. The original
constant-returning GC test did not cover this defect; the original green suite
below was not sufficient evidence for that dead-stripping claim.

Explicit executable-export builds now use existing runtime-helper checks
throughout the application, without a startup signal-trap table. Only the
`entry.is_some() && interface.is_none()` path registers that table; ordinary
application, library and fuzz behavior is unchanged. Native references and type
metadata still retain live checked bodies/destructors through normal relocations.
Stack probes stay enabled. Unqualified AArch64 callback builds do not register
below-SP loop-probe PCs for specialized overflow diagnostics; probe protection
remains, but that diagnostic and AArch64 qualification are not promised.

Post-fix evidence:

- The reviewer's exact missing-native-import reproduction now links and exits
  zero in both optimized and fast-compile modes. Both modes failed before this
  fix. Logs: `/tmp/gpui-gc-{old,new}-repro.log`.
- Five focused callback tests passed. New regressions cover a dead checked
  callback plus an unused owned class with a checked destructor, all depending
  on an undefined native import; and live native-referenced callback/destructor
  failure diagnostics with the correct source lines, in both modes. Existing
  ownership, protected adapters, publication and production Cargo-runtime actor
  containment checks passed. `/tmp/gpui-gc-callback-tests2.log`.
- Focused callbacks/exports/trap locations/library metadata/unloading/actors/
  stack overflow: final all-features rerun 139 passed, including object-before-GC
  witnesses and wide relocation inspection. `/tmp/gpui-gc-focused-final.log`.
- Strict root workspace/all-target/all-feature Clippy passed:
  `/tmp/gpui-gc-clippy-final.log`.
- Fresh headless GPUI lifecycle/ABI checks passed in both modes with `DISPLAY`
  and `WAYLAND_DISPLAY` unset: `/tmp/gpui-gc-counter-headless.log`. No additional
  desktop input or hardware smoke was performed; the original smoke below is
  preserved historical evidence, not a post-fix rerun.
- Narrow optimized emitted-code comparison: the positive checked-divide passing
  path remains 27 instructions, with unchanged frame/probe configuration. The
  cold check changes from `ud2` to site `lea`, helper `call`, `ud2` (+12 bytes).
  C's equivalent floor-divide kernel takes 11 passing-path instructions; the
  16-instruction pre-existing gap is four frame instructions, two redundant
  zero-check instructions, eight remainder recomputation/save/restore
  instructions, one sign-normalization instruction and one floor-adjustment
  instruction. No speedup or general optimizer change is claimed. Disassembly,
  C comparison and objects: `/tmp/gpui-gc-emitted/`.
- Full post-fix timed `CARGO_BUILD_JOBS=6 just test`: **passed**, 432.3s
  (47.5s compile, 384.7s run, including lock/cache effects). Compiler 332,
  CLI two, integration 6964/four ignored, benchmark-generator nine, runtime
  339/five ignored; both doc-test targets passed with zero tests. Log:
  `/tmp/gpui-gc-full-suite.log`; report:
  `target/check-timings/suite-20261117T220230-3624279/timings.{md,json}`.
  The report labels HEAD `672bccdb` because this fixing diff was uncommitted
  during the run; both newly added regressions passed in that run. No readiness
  implementation changed and issue #992 did not reproduce in this run.
- Root/native fmt checks and all three post-fix docs CI checks passed: 820
  Markdown files/6213 local targets/no missing links, Python 389 tests/four
  skips, pinned mdBook 0.5.4 complete 3328-page site. Logs:
  `/tmp/gpui-gc-fmt-final.log` and `/tmp/gpui-gc-doc-{links,python,site}.log`.
- Fresh link/symbol evidence in `/tmp/gpui-gc-native-evidence/` again shows
  exactly one main, Plenty main, runtime startup, Rust allocator, panic entry
  and allocation-error handler; one native support archive; no application DSO,
  RPATH or RUNPATH. The current uninstrumented development-Rust/optimized-Plenty
  binary is 33,877,976 bytes; the archive remains 229,327,304 bytes. These are
  warm development artifacts, not release or clean-build measurements.

Initial new fixture assertions incorrectly expected absolute source paths and
absence of undefined names in the static symbol table. They were corrected to
check root-relative diagnostics, actual defined bodies and live relocations;
linking itself still requires no definition of the unavailable native import.
The final focused rerun also checks that every dead body/destructor witness
exists in the application object before native section GC removes it.

## Original infrastructure evidence (`672bccdb`)

- Compiler release build passed. Both optimized and fast-compile application
  objects/headers linked and ran against one Cargo-built support archive.
- Native default Rust suite: six passed. All-target/application-feature suite:
  seven passed, including rejection outside process-main entry. Native fmt and
  Clippy `--all-targets --features application -- -D warnings` passed.
- `tests/test_counter.py`: passed both compilation modes, real C adapters,
  opaque callback addresses, state sequence 0→1→2→0, event/render/layout/null
  configuration/factory failures, reentrant and repeated run rejection,
  exactly-once originating destruction, package checking and worker rejection.
  Log: `/tmp/gpui-counter-tests-final.log`.
- **One user-approved hardware smoke** (`test_counter.py --smoke`) passed:
  rendered Count pixels, XTest Increment/Reset, restored pixels, Alt+F4 close,
  successful exit and exactly one `STATE_DESTROYED` from Plenty's originating
  destructor. Preserved artifacts:
  `/tmp/plenty-gpui-lifecycle-smoke/{before,incremented,reset}.png` and
  `runner.log`; command log `/tmp/gpui-counter-smoke.log`. No further intrusive
  run was made for this milestone. Headless tests are ABI/model tests only.
- Executable callback regression suite: three passed, covering both compile
  modes, Result/ref/scalar adapters, owned handles/factory failures/originating
  destructors, protected visibility, callback addresses, dead adapter removal,
  invalid modes/names/ABI/namespace/publication and an offline Cargo-runtime
  archive without GUI dependencies. Its production actor test checks
  PREPARE/CREATE, routed fallible allocation, contained arithmetic trap and
  DISCARD after normal startup. `/tmp/gpui-callback-tests-verified.log`.
- Root workspace/all-target/all-feature Clippy with `-D warnings` passed
  (`/tmp/gpui-root-clippy-verified.log`); both root and native fmt checks passed.
- All-features runtime unit rerun: 339 passed, five ignored
  (`/tmp/gpui-runtime-tests-final.log`). The executable Cargo feature initially
  enabled production actor dispatch inside the unit harness; three cfg checks
  now retain library dispatch in unit tests. Production dispatch is separately
  covered by an actually linked Cargo executable-feature regression.
- Full docs CI passed with pinned mdBook **0.5.4**: source links checked 820
  Markdown files/6211 targets with no missing links; Python discovery 389 tests,
  four skips; complete published site checked 3328 HTML pages. Logs:
  `/tmp/gpui-doc-{links,python,site}-final.log`.

The original candidate's **full timed `just test` passed**: 332 compiler unit
tests, two CLI unit tests, 6962 integration tests (four ignored), nine benchmark-generator
tests and 339 runtime tests (five ignored); both doc-test targets passed with
zero tests. Total 356.0s (50.0s compile, 306.0s run), including cache/lock effects.
Report: `target/check-timings/suite-20261117T213558-2423550/timings.{md,json}`;
command log `/tmp/gpui-full-suite-acceptance.log`. This includes existing export,
library-unloading, C-host, callback, threading, allocator and tutorial regressions.

Earlier checks were not green: the executable feature exposed unit-harness
actor assumptions; the new deliberate-trap fixture needed correct integer
syntax and normal startup/check-table registration before native callbacks;
cache reporting needed an Arc dereference; native Clippy found three closures.
Those were corrected. A stale suite was explicitly interrupted, not passed.
An independent parallel readiness unit test intermittently missed immediate
pipe HUP; local issue #992 records the failure and passing isolated/full-runtime
reruns. No readiness implementation changed.

## One-runtime and CLI isolation evidence

The captured final link is exactly:

```text
cc application.o libplenty_gpui.a -Wl,--gc-sections <rustc native-static-libs> -o plenty-gpui
```

`libplenty_gpui.a` is the sole Rust archive, built in one Cargo graph containing
runtime + bridge + GPUI. No compiler-bundled runtime, panic redirects, archive
extraction/merging, duplicate-symbol suppression or ignored link errors.
`target/optimized/link-command.txt`, `native-build.log` and
`native-link-args.txt` retain actual commands and reported native requirements.

`readelf`/`nm` evidence in `/tmp/gpui-unified-evidence/` shows exactly one each
of main, plenty_main, plenty_runtime_start, Rust allocator, panic entry and
allocation-error handler. C callback adapters and the originating destructor
are ELF GLOBAL PROTECTED. There is no `libgpui_app.so`, RPATH or RUNPATH.
Dynamic requirements are system freetype, XCB, xkbcommon/xkbcommon-x11,
libgcc_s, libm, libc and the loader, not a shared Plenty application.
This is not a fully static executable or a per-widget stripping guarantee.

An ordinary CLI-only program prints `CLI_ONLY` and links only libgcc_s, libm,
libc and the loader. Its ordinary compiler link is unchanged. The root
workspace's all-features normal dependency graph contains no GPUI, Blade or
xkbcommon. Evidence: `/tmp/gpui-cli-proof/` and
`/tmp/gpui-unified-evidence/cli-workspace-tree.txt`.

## Costs and lock

These original-candidate measurements are mixed/warm development measurements,
**not clean/release benchmarks**.
The unstripped optimized-Plenty/development-Rust counter is 33,878,664 bytes;
its native support archive is 229,327,304 bytes. The mixed native target
folder occupied 2.1 GiB. Warm native support builds reported roughly one
second; Plenty emission and final cc linking are additional work.

The lock contains 695 packages across targets/tools; Linux-filtered metadata
with application feature contains 535. Neither is an executed compilation
count. Adding the runtime updates its pinned libc (0.2.186), memchr, rustls,
siphasher and zeroize requirements and adds its crypto/compression dependencies;
the GPUI family remains 0.2.2. Old xattr remains incompatible with libc
0.2.191. No GPUI + gpui-kit ecosystem upgrade is implied.

The installed xkbcommon-x11 runtime lacks its development linker symlink.
Native commands used the local-only
`/tmp/gpui-native-libs/libxkbcommon-x11.so` symlink to the real installed
`/usr/lib64/libxkbcommon-x11.so.0`, via `LIBRARY_PATH`. No stub, system-package
installation or replacement library was used. Install the documented development
package normally.

## Historical baseline and remaining limits

Local PR #1303 initially qualified a Rust host plus resident shared Plenty app
because independently built Rust staticlibs collided. That historical runner
was 35,443,664 bytes plus a 5,415,024-byte app library and had separate resident
runtimes. Those sizes/costs and that workaround do **not** describe this milestone.

Still unqualified: native Wayland input/rendering, Rust release profile,
ARM/macOS/Windows, production accessibility/keyboard widgets, IME/text editing,
recoverable upstream OOM/panic, GUI actor/arena recovery, automatic package-native
build selection and binary redistribution. The helper stages one application
file explicitly and needs matching compiler/runtime sources and native tools.
Executable callbacks currently require baseline CPU; closures/retained callback
registries and arbitrary thread transfer are not introduced.

Notice generation includes the application Cargo graph and vendored/embedded
runtime credits, but still intentionally fails closed on 31 upstream crates
without license texts (`/tmp/gpui-notices.log`). No binary is shipped or declared
redistributable. GPUI still reports upstream future-incompatibility warnings for
num-bigint-dig 0.8.4 and proc-macro-error2 2.0.1. No dependency-advisory audit is
claimed. GPUI + gpui-kit evaluation, widget catalog and std GUI API remain a
separate follow-up under B38.
