# Source-built GUI packages

Start with [setup and build commands](../../book/src/design/stdlib/gui-build.md)
and the [counter walkthrough](../../book/src/tutorial/special-purpose/gui-counters.md).
`iced_counter/` and `egui_counter/` are independent ordinary Plenty packages.

Opt-in headless package validation, from the repository root:

```sh
CARGO_BUILD_JOBS=3 python3 -B examples/gui/check_packages.py --plenty target/debug/plenty
```

Uses temporary consumer packages and the checked-out compiler; both profiles
exercise redraws, checked failure with a full stderr, owner destruction,
repeated builds and build-only rejection of consuming callbacks.
