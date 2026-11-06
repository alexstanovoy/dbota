#!/usr/bin/env bash
# Runs a visual-check client with private settings and replay data.
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
: "${DBOTA_ASSETS:?DBOTA_ASSETS must name a test asset cache}"
[ -f "$DBOTA_ASSETS/index.txt" ] || { echo "test asset cache has no index: $DBOTA_ASSETS" >&2; exit 1; }
mkdir -p "$here/target"
private_home=$(mktemp -d "$here/target/test-client.XXXXXX")
trap 'rm -r -- "$private_home"' EXIT
env -u BOTA_SERVER -u BOTA_SEAT_KEY -u DBOTA_SERVICE -u DBOTA_DRILL -u DBOTA_DRILL_SHOTS \
    HOME="$private_home" XDG_CONFIG_HOME="$private_home/config" XDG_DATA_HOME="$private_home/data" \
    XDG_STATE_HOME="$private_home/state" LOCALAPPDATA="$private_home/data" APPDATA="$private_home/data" \
    XDG_CACHE_HOME="${CARGO_TARGET_DIR:-$here/target}/tmp/gpu-cache" "$here/target/release/dbota" "$@"
