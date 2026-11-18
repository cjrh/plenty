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
repeated builds and build-only rejection of consuming callbacks without
replacing the last good binary. A rejected application argument consumes
entry without creating an owner.

`python3 -B examples/gui/check_build_races.py --plenty target/debug/plenty`
uses ordinary package builds with marker Cargo/linker stand-ins (no native
GUI dependencies or display). It pauses a driver before its archive read,
observes contention for that backend and that workspace output, lets an
independent build finish, then verifies the bytes, arguments and freshness of
the competing outputs. Add `--runtime-checks` for a compiler built with that
feature; the fixture checks that both GUI package links explicitly reject it
before Cargo runs or an executable is published.
