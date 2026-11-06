#!/usr/bin/env bash
# Runs a visual-check client with private settings and replay data.
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
# shellcheck source=tools/client_env.sh
. "$here/tools/client_env.sh"
require_test_asset_cache || exit 1
target=${CARGO_TARGET_DIR:-$here/target}
mkdir -p "$target"
private_home=$(mktemp -d "$target/test-client.XXXXXX")
trap 'rm -r -- "$private_home"' EXIT
binary=$(client_binary)
private_client_environment "$private_home" "$target/tmp/gpu-cache"
"$binary" "$@"
