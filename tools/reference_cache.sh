#!/usr/bin/env bash
# Makes the reference cache the importer's golden tests compare with: the export Source2Viewer 20 made for version 6 of the
# manifest (crates/dbota_import/tests/common), into ~/.cache/dbota-gate/reference-v6, private to the gate (DBOTA_REFERENCE names another place).
#
#   tools/reference_cache.sh
#
# The exporter script was replaced by `dbota import` (commit ad097c5); the golden tests keep comparing with what it exported,
# so this runs the exporter and manifest of the commit before it, from git history, with its checksum-pinned Source2Viewer-CLI
# (kept in ~/.cache/dbota/tools, downloaded once). It needs a Dota 2 install (DBOTA_DOTA_PATH or the usual Steam places) and
# takes a while; nothing of Valve's lands anywhere but the cache. Does nothing when the cache is complete.
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
readonly LAST_EXPORTER_PARENT=ad097c5^
ref=${DBOTA_REFERENCE:-$HOME/.cache/dbota-gate/reference-v6}
done_mark=$ref/.reference-complete
[ -f "$done_mark" ] && { echo "reference cache: $ref is complete"; exit 0; }
# The exporter writes where DBOTA_ASSETS says, its Source2Viewer tools folder too (downloaded once, checksum-pinned).
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
git -C "$here" archive "$LAST_EXPORTER_PARENT" assets-manifest tools | tar -x -C "$tmp"
args=()
[ -z "${DBOTA_DOTA_PATH:-}" ] || args=(--dota-path "$DBOTA_DOTA_PATH")
mkdir -p "$ref"
DBOTA_ASSETS=$ref "$tmp/tools/import_assets.sh" "${args[@]}"
date +%FT%T > "$done_mark"
echo "reference cache: $ref made"
