PLENTY ?= ../../../target/release/plenty
JOBS ?= 3
export CARGO_BUILD_JOBS = $(JOBS)

.PHONY: build run test fmt clippy smoke
build:
	python3 -B build.py --plenty "$(PLENTY)"
run: build
	target/optimized/plenty-egui
# This includes real Plenty executable/native support linkage, but does not open a window.
test: build
	PLENTY_EGUI_APP_DIR=$(CURDIR)/target/optimized cargo test --offline --locked --manifest-path native/Cargo.toml --features legacy-application
	python3 -B tests/test_counter.py --plenty "$(PLENTY)"
fmt:
	cargo fmt --manifest-path native/Cargo.toml -- --check
clippy: build
	PLENTY_EGUI_APP_DIR=$(CURDIR)/target/optimized cargo clippy --offline --locked --manifest-path native/Cargo.toml --all-targets --features legacy-application -- -D warnings
# Creates its own headless Weston/Xwayland, never uses the desktop display.
smoke: build
	python3 -B tests/test_counter.py --plenty "$(PLENTY)" --smoke
