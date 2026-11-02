# Recipes run from the checkout root, even when invoked in a subdirectory.
set positional-arguments

# Complete local release installation; the default recipe.
build: compiler (engine "dataframe") (engine "linalg")

# Release compiler only; optional engines are not Cargo dependencies.
compiler:
    cargo build --locked --release --bins --target-dir target

# Build one engine plus the notices/metadata needed by --dist.
engine name:
    python3 -B scripts/build_native_engine.py "$1" --out target/release
    python3 -B scripts/release/package-engine.py "$1" --engine "target/release/libplenty_$1_v1.so" --out-dir target/release --local-install

# Full workspace suite, including allocation-failure tests.
test:
    cargo test --locked --workspace --all-features

# All published-site checks (requires the mdBook version in book/releases.json).
docs:
    python3 -B scripts/check_links.py book/src
    python3 -B -m unittest discover -s scripts -p 'test_*.py'
    python3 -B scripts/build_docs_site.py

# Steps and logs: see scripts/local_ci.sh --help. `just ci test clippy` runs two.
# Every CI check this Linux host can run, continuing past failures.
ci *steps:
    scripts/local_ci.sh "$@"

# macOS and Windows release jobs still run only in CI.
# Before tagging: `just ci` plus the Docker release build, tests, and smoke tests.
release-check:
    scripts/local_ci.sh --release

# Spends API credit; not run in CI. Arguments pass through, e.g.
# `just skill-trial wordfreq --model sonnet`.
# Trial the agent skill with fresh agents; report in target/skill-trial/.
skill-trial *args: compiler
    python3 -B scripts/skill_trial.py "$@"

# Assemble the plentylang/skills repository from .agents/skills/plenty.
skill-sync out:
    python3 -B scripts/sync_agent_skill.py "$1"
