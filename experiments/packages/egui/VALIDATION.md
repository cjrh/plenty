# Egui port validation

Reviewed source: `26b744f4` (local PR #1314). Base: `1e608519`
(native executable foundation, local PR #1316). Fedora 44, Linux x86-64,
Rust 1.100.0, Plenty 0.8.32. Built the compiler and the separate support
manifest in this worktree, offline/locked with `CARGO_BUILD_JOBS=3`.
Machine-local logs are in the worktree's `target/egui-checks/`.

- `make fmt`, native `cargo test --offline --locked --features application`
  (5 tests) and `cargo clippy --offline --locked --all-targets --features
  application -- -D warnings` passed. Use `native/Cargo.toml` and point
  `PLENTY_EGUI_APP_DIR` at the generated application directory for both
  Cargo commands. Guards cover the live token, exclusive access, thread,
  entry, text/widget budgets, status tags and unwind cleanup.
- `python3 -B tests/test_counter.py --smoke --plenty ../../../target/debug/plenty`
  exercised **both** Plenty modes. Headless pointer input with requested
  discard and idle frames painted 0 three times, 1 seven times, 2 seven
  times, then 0 five times. Covered overflow without writeback, later error
  after writeback, factory/start/draw failure, reentry, `/dev/full`, path
  dependency use and worker rejection.
- Private Weston/Xwayland, software GL, XTest: the instrumented Plenty UI
  logged exactly `MUTATION 1`, `MUTATION 2`, `MUTATION 0`. The painted value
  changed on each click; reset restored the initial count pixels. All DROP
  lines on native close were exactly `['DROP 0']`; after a draw failure,
  exactly `['DROP 1']`, with exit 1 and `LimitExceeded` returned to main.
  See package `target/{optimized,fast-compile}/{lifecycle-smoke,error-smoke}/`
  for PNGs and process logs. No existing display used.
- Consuming `State` instead of borrowing it and adding an unrelated export
  retained the C draw prototype, but failed final link on the independently
  required guard, in both modes. **No mismatched fixture was executed.**
  Restored the uninstrumented example afterward. The final link uses the
  foundation helper, one Plenty object and one Cargo-selected support archive;
  the runtime belongs to that archive, not a second runtime link input.
- `cargo tree --offline --locked` on the root workspace versus the isolated
  application manifest: no egui/eframe/winit, iced GUI or GPUI in the root
  graph, no iced GUI or GPUI in the egui build. Ordinary unstripped CLI
  controls compiled and ran in both modes without GUI symbols or a callback
  header. `readelf -d` showed only libgcc_s, libm, libc and the ELF loader.

Only the explicit source build and Linux x86-64 X11/Glow path are qualified
here. No hardware GL, other platforms, upstream OOM recovery, distribution
notice bundle, import-driven native builds or std GUI API is claimed.
