#!/usr/bin/env bash
# The merge gate of this repository: every check a change must pass, in order, stopping at the first failure.
#
#   tools/gate.sh [REPLAY.brp]
#
# Steps: fmt, clippy, web-webgpu and web-webgl2 (the browser clients compile), assets (the asset cache of this build, imported
# once per stamp from a Dota 2 install), test, build (release), deny, then, with the cache: install-tests (the ignored importer
# tests) and visual (tools/visual_check.sh on a replay), and, when asked, fps.
# The replay for `visual` is the argument, or GATE_REPLAY, or one the bota checkout beside this one records
# (../bota, the example local_match), so the gate needs nothing but this checkout and bota.
# Environment:
#   DBOTA_ASSETS=DIR         the asset cache the tests and the visual check use (default: <DBOTA_GATE_ASSETS, else
#                            ~/.cache/dbota-gate>/<stamp of this build's manifest, ids and importer>, imported once and
#                            reused; never the shared ~/.cache/dbota, whose stamp another build may have changed)
#   GATE_REQUIRE_INSTALL=1   without a Dota 2 install the install steps fail instead of being skipped
#   GATE_REQUIRE_REFERENCE=1 the importer's golden tests also need the reference cache (DBOTA_REFERENCE); without it they
#                            are not run, and this makes that a failure
#   GATE_FPS_CMD / GATE_FPS_BASE   the FPS gate: runs `$GATE_FPS_CMD $GATE_FPS_BASE HEAD` (a command taking a base and a
#                            candidate git revision of this repository and exiting non-zero on a regression)
#   GATE_LEAN=1              only what a change of the crates under ../bota can break: fmt, clippy, web-webgpu, assets and test
#                            (plus the asset cache the tests need; no webgl2, release build, deny, install-tests or visual check); prints GATE-OK (lean)
#   GATE_SKIP="deny web-webgl2"   steps left out (to iterate on one; the gate only counts as passed with none skipped)
#   GATE_LOGS=DIR            where each step's output goes (default: temp/gate under the repository)
# Prints GATE-OK when every step ran and passed, GATE-FAIL <step> (with the tail of its output) otherwise.
set -uo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$here" || exit 2
logs=${GATE_LOGS:-$here/temp/gate}
mkdir -p "$logs"
skip=" ${GATE_SKIP:-} "
skipped=0
# The tests that start the client (tests/e2e) and the visual check need an imported asset cache of this build. It is made once
# per asset stamp (a hash of the manifest, the ids and the importer's revision) in a directory outside the repository, reused by
# every run and every checkout with that stamp, and only read by the tests.
key=$(
    cat assets-manifest/manifest.txt assets-manifest/ids.txt assets-manifest/version
    grep -h 'pub const IMPORT_REV' crates/dbota_import/src/import.rs
)
key=$(printf '%s' "$key" | sha256sum | cut -c1-16)
export DBOTA_ASSETS=${DBOTA_ASSETS:-${DBOTA_GATE_ASSETS:-${XDG_CACHE_HOME:-$HOME/.cache}/dbota-gate}/$key}

step() { # name command...
    local name=$1
    shift
    case "$skip" in *" $name "*) echo "== $name: skipped"; skipped=1; return 0 ;; esac
    echo "== $name"
    if ! "$@" > "$logs/$name.log" 2>&1; then
        tail -n 40 "$logs/$name.log"
        echo "GATE-FAIL $name (full output: $logs/$name.log)"
        exit 1
    fi
    grep -E '^test result' "$logs/$name.log" | awk '{p += $4; f += $6; i += $8} END {if (NR) print "   " p " passed, " f " failed, " i " ignored"}'
}

# The browser clients, one per graphics API; the build embeds a replay, any file does for a check.
web() { DBOTA_WEB_REPLAY="$here/Cargo.toml" cargo check --target wasm32-unknown-unknown --profile web -p dbota --features "web-replay,$1"; }

step fmt cargo fmt --check
step clippy cargo clippy --all-targets -- -D warnings
step web-webgpu web webgpu
[ "${GATE_LEAN:-0}" = 1 ] || step web-webgl2 web webgl2

# The asset cache: `dbota import` finds the Dota 2 install and does nothing when the cache already has this stamp.
echo "== assets"
have_install=0
mkdir -p "$(dirname -- "$DBOTA_ASSETS")"
if (flock 9 && cargo run --release -q -- import) 9> "$DBOTA_ASSETS.lock" > "$logs/import.log" 2>&1; then
    have_install=1
else
    tail -n 5 "$logs/import.log"
    if [ "${GATE_REQUIRE_INSTALL:-0}" = 1 ]; then
        echo "GATE-FAIL assets (no usable Dota 2 install; full output: $logs/import.log)"
        exit 1
    fi
    echo "   no usable Dota 2 install: the tests that need the cache fail, install-tests and visual are skipped"
    skipped=1
fi
step test cargo test
if [ "${GATE_LEAN:-0}" = 1 ]; then
    [ "$skipped" = 1 ] && echo "GATE-OK (lean, with skipped steps: not a pass)" || echo "GATE-OK (lean)"
    exit 0
fi
step build cargo build --release
step deny cargo deny check

# With an install (the asset cache above), the steps that draw from it.
if [ "$have_install" = 1 ]; then
    # The importer's golden tests also compare with a reference cache (see crates/dbota_import/tests/common).
    if [ -d "${DBOTA_REFERENCE:-$HOME/.cache/dbota/v6}" ]; then
        step install-tests cargo test --release -p dbota_import -- --ignored
    elif [ "${GATE_REQUIRE_REFERENCE:-0}" = 1 ]; then
        echo "GATE-FAIL install-tests (no reference cache; see crates/dbota_import/tests/common)"
        exit 1
    else
        echo "== install-tests: not run (no reference cache; GATE_REQUIRE_REFERENCE=1 makes that a failure)"
    fi
    replay=${1:-${GATE_REPLAY:-}}
    if [ -z "$replay" ]; then
        echo "== replay (recorded by ../bota)"
        replay=$logs/replay.brp
        if ! cargo run --release -q --manifest-path "$here/../bota/Cargo.toml" -p bota_server --example local_match \
            -- --replay "$replay" > "$logs/replay.log" 2>&1; then
            tail -n 40 "$logs/replay.log"
            echo "GATE-FAIL replay (full output: $logs/replay.log)"
            exit 1
        fi
    fi
    step visual tools/visual_check.sh "$replay"
fi

if [ -n "${GATE_FPS_CMD:-}" ]; then
    # shellcheck disable=SC2086
    step fps $GATE_FPS_CMD "${GATE_FPS_BASE:?GATE_FPS_BASE: the git revision to compare with}" HEAD
fi

if [ "$skipped" = 1 ]; then
    echo "GATE-OK (with skipped steps: not a pass)"
else
    echo "GATE-OK"
fi
