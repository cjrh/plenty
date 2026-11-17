# Initial qualification (2026-11-17)

Host: Fedora 44, Linux x86-64, Rust 1.100.0, Plenty 0.8.32 from this branch,
GPUI 0.2.2, AMD Radeon 780M (RADV PHOENIX), Mesa 26.2.6,
Cinnamon/X11/Xwayland desktop at scale 1. The runner reports
`is_software_emulated: false` from its actual GPUI window.
No compiler/runtime source changes. All artifacts and detailed logs stayed
outside tracked source (`target/` and `/tmp/gpui-*`).

## Results

- `cargo build --locked --release --bin plenty`: passed (3m06s, compiler only).
- `cargo test --locked --manifest-path experiments/packages/gpui/native/Cargo.toml`:
  six deterministic boundary tests passed; no GPUI test platform substituted.
- `python3 -B experiments/packages/gpui/tests/test_counter.py`: passed, including
  real resident shared-library/opaque-command-pointer ABI, counter sequence
  0→1→2→0, both Plenty compilation modes, callback/null configuration/factory
  failures and exactly-once originating Plenty destructor. Package consumer
  checking and uncertified worker rejection passed.
- `python3 -B experiments/packages/gpui/tests/smoke_x11.py`: passed after fixing
  the native close's inline-quit reborrow. A real rendered window received
  XTest clicks, changed Count pixels, restored them with Reset and exited zero
  on Alt+F4. Screenshots: `/tmp/plenty-gpui-smoke/{before,incremented,reset}.png`.
  This assumes an idle scale-1 desktop and known counter layout; earlier
  calibration attempts missed buttons or lost first-click focus and failed,
  rather than falsely passing. The fixture now permits at most three clicks
  for desktop click-to-focus policies; changed/restored pixels remain required.
- `make fmt clippy`: passed, including `--all-targets --features application`
  Clippy with `-D warnings`; the native all-target/feature test run also passed.
- `cargo test --locked --test integration test_c_host::`: 2 passed.
- `cargo test --locked --test integration test_library_unloading::`: 3 passed.
- `cargo test --locked --test integration test_host_callbacks::`: 5 passed.
- `cargo test --locked --test integration test_tutorial::summary_lists_every_book_page`:
  passed. The new book page contains shell commands, not an untested Plenty fence.
- `tests/test_counter.py --smoke`: additionally passed a real rendered/close
  lifecycle with the originating Plenty destructor instrumented: exactly one
  `STATE_DESTROYED` in `/tmp/plenty-gpui-lifecycle-smoke/runner.log`.
- Full docs source links, Python suite and published-site build with mdBook
  0.5.4 passed (819 Markdown files/6183 local links, Python 389 tests with 4
  existing skips, complete site 3324 HTML pages). No full compiler suite or
  native Rust release-profile build is claimed.

The machine had the xkbcommon-x11 runtime but no development linker symlink,
and no passwordless sudo. Native link/test/lint commands used a **local-only**
`/tmp/gpui-native-libs/libxkbcommon-x11.so` symlink to the installed
`/usr/lib64/libxkbcommon-x11.so.0`, with `LIBRARY_PATH=/tmp/gpui-native-libs`.
This supplies the real system library; no stub or replacement implementation.
Install the documented development package on a normal build machine.

## Practical costs

These are development measurements, **not** pristine clean-build benchmarks.
Root Cargo configuration optimizes third-party dependencies; developer caches
were mixed/warm. With six jobs, building the compatible locked native library
reported 2m59s wall / 936.56s user / 57.49s system and 1,929,124 KiB maximum
child RSS (`/usr/bin/time -v`; not aggregate process memory), compiling 438
package units. Initial incompatible newest-resolution attempt separately cost
2m13s before failing in xattr. The already-built host subsequently linked in
about 1–3 seconds. Current mixed native target directory is roughly 2.9 GiB.

The independent lockfile has 681 total package entries (all targets/tools).
Linux-filtered Cargo metadata has 521 packages; the notice generator reaches
447 normal non-proc-macro registry dependency crates. These are different
counts, neither a claim of 681 compiled production dependencies nor a minimal
graph: GPUI still pulls image, networking and other support outside the exposed
UI slice. Unstripped development runner: 35,443,664 bytes; optimized Plenty
counter shared library: 5,415,024 bytes. These are separate resident runtimes,
not a self-contained single binary or a measured release size.

## Deliberately unqualified

- Native Wayland input/rendering, Rust release profile, ARM/macOS/Windows,
  production accessibility, keyboard widget behavior, IME/text editing,
  recoverable upstream OOM/panic and actor/arena recovery.
- Binary redistribution notices: `scripts/release/third_party_notices.py
  --repo . --component gpui --out /tmp/gpui-notices.txt` intentionally fails
  closed on 31 upstream crates without license files. This records a real
  limitation, not completed release packaging. No binary is shipped.
- GPUI 0.2.2 reports upstream Rust future-incompatibility warnings for
  `num-bigint-dig 0.8.4` and `proc-macro-error2 2.0.1`; this task does not
  update its dependency family or claim a dependency-advisory audit.
