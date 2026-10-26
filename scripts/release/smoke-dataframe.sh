#!/usr/bin/env bash
# Compatibility entry point for the dataframe release smoke test.
set -euo pipefail
exec bash "$(dirname "${BASH_SOURCE[0]}")/smoke-engine.sh" "$1" "$2" dataframe
