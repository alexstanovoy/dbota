#!/usr/bin/env bash
# Builds the fuzz targets with libFuzzer's coverage instrumentation on stable
# Rust (no sanitizer: that needs nightly) and runs them.
#
#   fuzz/fuzz.sh build                     build every target
#   fuzz/fuzz.sh run <target> [args...]    fuzz one target; extra arguments go to libFuzzer
#                                          (e.g. -max_total_time=3600, or a crash file to replay)
#   fuzz/fuzz.sh campaign <seconds> [jobs] every target at once for that long, each in fork mode
#                                          with `jobs` processes (default 1), past crashes, hangs
#                                          and memory blowups, all of which land in artifacts/
#
# The corpora and findings live outside the repository, in $FUZZ_DATA
# (default ~/.cache/dbota-fuzz): seeds are the user's own Valve files. Each
# target keeps its corpus in corpus/<target> there and writes what it finds
# to artifacts/<target>. An input slower than FUZZ_TIMEOUT seconds (30) is a
# hang. A process may map 4 GiB at most: without a sanitizer libFuzzer sees
# no single allocation, and its RSS limit misses zeroed pages never touched,
# so an oversized allocation fails (and aborts) instead.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
data=${FUZZ_DATA:-${XDG_CACHE_HOME:-$HOME/.cache}/dbota-fuzz}
export RUSTFLAGS="-Cpasses=sancov-module -Cllvm-args=-sanitizer-coverage-level=4 \
-Cllvm-args=-sanitizer-coverage-inline-8bit-counters -Cllvm-args=-sanitizer-coverage-pc-table \
-Cllvm-args=-sanitizer-coverage-trace-compares --cfg fuzzing -Cdebug-assertions -Coverflow-checks"
# An explicit target keeps the flags off build scripts and proc macros.
triple=$(rustc -vV | sed -n 's/^host: //p')
limits=(-timeout="${FUZZ_TIMEOUT:-30}" -rss_limit_mb=2048)

build() {
    cargo build --manifest-path "$here/Cargo.toml" --release --bins --target "$triple"
}

# The longest input worth mutating: a few real files of the format fit.
max_len() {
    case "$1" in
    model | world | import) echo 1048576 ;;
    resource | vtex | cache_image | anim | entities | tooltips | portraits | shop) echo 262144 ;;
    kv3 | material | vbib) echo 131072 ;;
    *) echo 65536 ;;
    esac
}

# Kills the fork-mode child of a campaign whose log has not moved for four
# timeouts: without a sanitizer runtime a timed-out child can deadlock in
# libFuzzer's alarm handler and hold its target forever.
watchdog() {
    local stale=$((4 * ${FUZZ_TIMEOUT:-30}))
    while sleep 60; do
        for pid in "${!logs[@]}"; do
            kill -0 "$pid" 2>/dev/null || continue
            local age=$(($(date +%s) - $(stat -c %Y "${logs[$pid]}")))
            if ((age > stale)); then
                pkill -9 -P "$pid" || true
            fi
        done
    done
}

case "${1:-}" in
build) build ;;
run)
    target=${2:?which target}
    shift 2
    build
    mkdir -p "$data/corpus/$target" "$data/artifacts/$target"
    ulimit -v $((4 << 20))
    exec "$here/target/$triple/release/$target" -artifact_prefix="$data/artifacts/$target/" \
        "${limits[@]}" -max_len="$(max_len "$target")" "$data/corpus/$target" "$@"
    ;;
campaign)
    seconds=${2:?how long}
    jobs=${3:-1}
    build
    ulimit -v $((4 << 20))
    declare -A logs
    for bin in "$here"/fuzz_targets/*.rs; do
        target=$(basename "$bin" .rs)
        mkdir -p "$data/corpus/$target" "$data/artifacts/$target"
        log="$data/artifacts/$target.log"
        "$here/target/$triple/release/$target" -artifact_prefix="$data/artifacts/$target/" \
            -fork="$jobs" -ignore_crashes=1 -ignore_timeouts=1 -ignore_ooms=1 \
            -max_total_time="$seconds" "${limits[@]}" -max_len="$(max_len "$target")" \
            "$data/corpus/$target" >"$log" 2>&1 &
        logs[$!]=$log
    done
    watchdog &
    dog=$!
    for pid in "${!logs[@]}"; do
        wait "$pid" || true
    done
    pkill -P "$dog" || true
    kill "$dog" || true
    find "$data/artifacts" -mindepth 2 -type f | sort
    ;;
*)
    sed -n '2,19p' "$0"
    exit 2
    ;;
esac
