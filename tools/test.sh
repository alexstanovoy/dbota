#!/usr/bin/env bash
# Build the selected release test binaries once, then overlap menu-core tests with the rest.
# Cargo flags and libtest arguments are accepted as by cargo test. Filtered/ignored runs stay serial.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
exec python3 tools/test_runner.py "$@"
