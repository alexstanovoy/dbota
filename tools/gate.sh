#!/usr/bin/env bash
# The merge gate of this repository: every check a change must pass, in order, stopping at the first failure.
#
#   tools/gate.sh [REPLAY.brp]
#
# Steps: ignored-audit (every #[ignore]d test has a step that runs it), fuzz-check (the fuzz targets compile), clippy, web-webgpu and web-webgl2 (the browser clients compile), assets (the asset cache of this build, imported
# once per stamp from a Dota 2 install), test, build (release), deny, then, with the cache: reference (the importer's golden
# reference cache, exported once from the install by tools/reference_cache.sh), install-tests (the ignored importer tests),
# visual (tools/visual_check.sh on a replay) and browser (tools/browser_check.sh: the pages built, every check of
# web/test/smoke.py run), and, when asked, fps, then fmt (last: it changes no code).
# A step that does not run fails the gate (GATE-FAIL): nothing is skipped quietly. The one exception is GATE_LEAN=1, which
# says what it leaves out and prints GATE-OK (lean: not a pass).
# The replay for `visual` is the argument, or GATE_REPLAY, or one the bota checkout beside this one records
# (../bota, the example local_match), so the gate needs nothing but this checkout and bota.
# Environment:
#   GATE_STEP_LOG=FILE       append the seconds each step took to FILE (tab separated)
#   GATE_PRECHECKED=1        start although tools/precheck.sh has not passed this tree
#   GATE_AGAIN=1             run although this tree already passed
#   DBOTA_ASSETS=DIR         the asset cache the tests and the visual check use (default: <DBOTA_GATE_ASSETS, else
#                            ~/.cache/dbota-gate>/<stamp of this build's manifest, ids and importer>, imported once and
#                            reused; never the shared ~/.cache/dbota, whose stamp another build may have changed)
#   DBOTA_REFERENCE=DIR      the importer's golden reference cache (default ~/.cache/dbota-gate/reference-v6, made by tools/reference_cache.sh; never the shared ~/.cache/dbota)
#   DBOTA_GATE_BOTA, DBOTA_GATE_RADOVID   the bota and radovid checkouts the browser step builds servers from (default ../bota, ../radovid)
#   GATE_FPS_CMD / GATE_FPS_BASE   the FPS gate: runs `$GATE_FPS_CMD $GATE_FPS_BASE HEAD` (a command taking a base and a
#                            candidate git revision of this repository and exiting non-zero on a regression)
#   GATE_LEAN=1              only what a change of the crates under ../bota can break: fmt, clippy, web-webgpu, assets and test
#                            (plus the asset cache the tests need; no webgl2, release build, deny, reference, install-tests, visual or
#                            browser check); prints GATE-OK (lean: not a pass), which the consumer gate does not count as a full pass
#   GATE_SKIP="deny web-webgl2"   steps left out (to iterate on one; the gate fails with them left out)
#   GATE_LOGS=DIR            where each step's output goes (default: temp/gate under the repository)
# Prints GATE-OK when every step ran and passed, GATE-FAIL <step> (with the tail of its output) otherwise.
set -uo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$here" || exit 2
logs=${GATE_LOGS:-$here/temp/gate}
mkdir -p "$logs"
# shellcheck source=tools/stamp.sh
. "$here/tools/stamp.sh"
# The gate confirms; the precheck finds. It starts only on a tree tools/precheck.sh has passed, and not again on a tree that
# already passed (formatting is left out of both). GATE_PRECHECKED=1 and GATE_AGAIN=1 skip the two checks, for a head that is
# gated on behalf of something else (a merge, a consumer check) and was checked elsewhere.
mode=gate
[ "${GATE_LEAN:-0}" = 1 ] && mode=gate-lean
tree=$(tree_key)
if [ "${GATE_PRECHECKED:-0}" != 1 ] && [ -z "$(stamp_has "$tree" precheck)" ]; then
    echo "gate: no passing precheck for this tree. Run tools/precheck.sh first (it checks what the change touches, in minutes; this gate is the confirmation, not the place to find errors)." >&2
    exit 3
fi
if [ "${GATE_AGAIN:-0}" != 1 ] && [ -n "$(stamp_has "$tree" "$mode")" ]; then
    echo "gate: this tree already passed the gate ($(stamp_has "$tree" "$mode")). GATE_AGAIN=1 runs it again." >&2
    exit 3
fi
if only_docs_changed "${GATE_BASE:-main}"; then
    echo "GATE-OK (documents only: nothing to confirm)"
    stamp_put "$tree" "$mode"
    exit 0
fi
skip=" ${GATE_SKIP:-} "
skipped=0
# The tests that start the client (tests/e2e) and the visual check need an imported asset cache of this build. It is made once
# per asset stamp (a hash of the manifest, the ids and the importer's revision) in a directory outside the repository, reused by
# every run and every checkout with that stamp, and only read by the tests.
key=$(
    cat assets-manifest/manifest.txt assets-manifest/ids.txt
    grep -h 'pub const IMPORT_REV' crates/dbota_import/src/import.rs
    grep -h 'pub const OUTPUT_REV' "$here/../heatel/src/lib.rs" 2>/dev/null || true
)
key=$(printf '%s' "$key" | sha256sum | cut -c1-16)
export DBOTA_REFERENCE=${DBOTA_REFERENCE:-${XDG_CACHE_HOME:-$HOME/.cache}/dbota-gate/reference-v6}
export DBOTA_ASSETS=${DBOTA_ASSETS:-${DBOTA_GATE_ASSETS:-${XDG_CACHE_HOME:-$HOME/.cache}/dbota-gate}/$key}

step() { # name command...
    local name=$1
    shift
    case "$skip" in *" $name "*) echo "== $name: skipped"; skipped=1; return 0 ;; esac
    echo "== $name"
    local began=$SECONDS
    "$@" > "$logs/$name.log" 2>&1
    local rc=$?
    # GATE_STEP_LOG=FILE: one line per step (time, repository directory, step, seconds, exit code), to see where a gate spends its minutes
    [ -z "${GATE_STEP_LOG:-}" ] || printf '%s\t%s\t%s\t%s\t%s\n' "$(date +%FT%T)" "$(basename -- "$here")" "$name" "$((SECONDS - began))" "$rc" >> "$GATE_STEP_LOG"
    if [ $rc -ne 0 ]; then
        tail -n 40 "$logs/$name.log"
        echo "GATE-FAIL $name (full output: $logs/$name.log)"
        exit 1
    fi
    grep -E '^test result' "$logs/$name.log" | awk '{p += $4; f += $6; i += $8} END {if (NR) print "   " p " passed, " f " failed, " i " ignored"}'
}

# The browser clients, one per graphics API; the build embeds a replay, any file does for a check.
web() { DBOTA_WEB_REPLAY="$here/Cargo.toml" cargo check --target wasm32-unknown-unknown --profile web -p dbota --features "web-replay,$1"; }

step ignored-audit tools/ignored_audit.sh
step fuzz-check fuzz/fuzz.sh check
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
    if [ "${GATE_LEAN:-0}" != 1 ]; then
        echo "GATE-FAIL assets (no usable Dota 2 install: the install tests, the visual check and the browser checks cannot run; full output: $logs/import.log)"
        exit 1
    fi
    echo "   no usable Dota 2 install: the tests that need the cache fail"
    skipped=1
fi
step test cargo test --profile test-release
# Formatting changes no code, so it is the last step: a gate that fails only here needs `cargo fmt` and an amend, not a rerun.
if [ "${GATE_LEAN:-0}" = 1 ]; then
    step fmt cargo fmt --check
    echo "GATE-OK (lean: not a pass$([ "$skipped" = 1 ] && echo ', with skipped steps'))"
    [ "$skipped" = 1 ] || stamp_put "$tree" "$mode"
    exit 0
fi
step build cargo build --release
step deny cargo deny check

# With an install (the asset cache above), the steps that draw from it.
if [ "$have_install" = 1 ]; then
    # The importer's golden tests also compare with a reference cache (see crates/dbota_import/tests/common): made once from
    # the install, kept beside the asset caches.
    step reference flock "$DBOTA_REFERENCE.lock" tools/reference_cache.sh
    step install-tests cargo test --release -p dbota_import -- --ignored
    step tip-content cargo test --release -p dbota tip_content -- --ignored
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
    step browser tools/browser_check.sh
fi

if [ -n "${GATE_FPS_CMD:-}" ]; then
    # shellcheck disable=SC2086
    step fps $GATE_FPS_CMD "${GATE_FPS_BASE:?GATE_FPS_BASE: the git revision to compare with}" HEAD
fi

step fmt cargo fmt --check

if [ "$skipped" = 1 ]; then
    echo "GATE-FAIL steps were skipped (GATE_SKIP): that is not a pass"
    exit 1
fi
echo "GATE-OK"
stamp_put "$tree" "$mode"
