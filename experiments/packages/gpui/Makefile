PLENTY ?= ../../../target/release/plenty
JOBS ?= 6
export CARGO_BUILD_JOBS = $(JOBS)

.PHONY: build run test fmt clippy smoke
build:
	python3 -B build.py --plenty "$(PLENTY)"
run: build
	native/target/debug/plenty-gpui
# This includes real Plenty shared application/runtime linkage, but does not open a window.
test:
	cargo test --locked --manifest-path native/Cargo.toml
	python3 -B tests/test_counter.py --plenty "$(PLENTY)"
fmt:
	cargo fmt --manifest-path native/Cargo.toml -- --check
clippy: build
	PLENTY_GPUI_APP_DIR=$(CURDIR)/target/optimized cargo clippy --locked --manifest-path native/Cargo.toml --all-targets --features application -- -D warnings
# Interactive desktop required; uses XTest on X11, not GPUI's test platform.
smoke: build
	python3 -B tests/smoke_x11.py
