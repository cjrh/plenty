# Unified executable qualification (2026-11-17)

Fedora 44, Linux GNU x86-64, Rust 1.100.0, Plenty 0.8.32 from this branch,
GPUI **0.2.2**. The current desktop is GNOME/Wayland with a real Xwayland
window at scale 1; the fixture removes `WAYLAND_DISPLAY`. Renderer:
AMD Radeon 780M Graphics / RADV PHOENIX / Mesa 26.2.6,
`is_software_emulated: false`. No test renderer or virtual desktop substituted.
Artifacts and detailed logs stay in ignored `target/` and `/tmp/gpui-*`.

## Current results

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
  destructor. Current artifacts:
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

The final **full timed `just test` passed**: 332 compiler unit tests, two CLI
unit tests, 6962 integration tests (four ignored), nine benchmark-generator
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

These are mixed/warm development measurements, **not clean/release benchmarks**.
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
