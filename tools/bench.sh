#!/usr/bin/env bash
# Measures dbota: binary sizes, cold and warm start, frame times, CPU, RAM and
# VRAM in a replay and in a live match against a local bota_server. Prints a
# Markdown table; every number comes from one `bench:` line of dbota's
# `--bench` (see src/framestats.rs) or from the files on disk.
#
#   tools/bench.sh REPLAY.brp
#
# Settings (environment):
#   BENCH_TICK=12000        replay tick the timed stretch starts at (lanes, towers and both heroes)
#   BENCH_DISTANCE=6500     camera distance: the widest view, the most drawn
#   BENCH_SECONDS=20        seconds of frames timed per run
#   BENCH_RUNS=3            timed replay runs; the table shows their median
#   BENCH_VSYNC=1           0: skip the run with vsync
#   BENCH_SERVER=../bota/target/release/bota_server   for the live run ("" skips it)
#   BENCH_ARGS=""           extra dbota arguments for every run (quality settings)
#   BENCH_WRAP=""           a command every dbota run goes through, e.g. a
#                           memory-capped `systemd-run --user --scope -p MemoryMax=8G --`
#                           (it must exec dbota, so its pid is dbota's)
#   BENCH_PROFILE=release   the cargo profile built and timed (`dist` is what ships)
#   BENCH_BUILD=1           0: use the binary already built
#   DBOTA_BIN               the binary to time (default target/release/dbota; implies BENCH_BUILD=0)
#   BENCH_WASM=0            1: also build for wasm32 (profile `web`) and report its size
#   DBOTA_ASSETS            the asset cache (default ~/.cache/dbota/v<assets-manifest/version>)
# A display is needed (DISPLAY or WAYLAND_DISPLAY).
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
replay=${1:-}
if [ -z "$replay" ] || [ ! -f "$replay" ]; then
    echo "usage: tools/bench.sh REPLAY.brp" >&2
    exit 2
fi
replay=$(cd -- "$(dirname -- "$replay")" && pwd -P)/$(basename -- "$replay")
tick=${BENCH_TICK:-12000}
distance=${BENCH_DISTANCE:-6500}
seconds=${BENCH_SECONDS:-20}
runs=${BENCH_RUNS:-3}
server=${BENCH_SERVER-$here/../bota/target/release/bota_server}
read -r -a extra <<< "${BENCH_ARGS:-}"
read -r -a wrap <<< "${BENCH_WRAP:-}"
cache=${DBOTA_ASSETS:-$HOME/.cache/dbota/v$(tr -d '[:space:]' < "$here/assets-manifest/version")}
profile=${BENCH_PROFILE:-release}
bin=${DBOTA_BIN:-$here/target/$profile/dbota}
out=$here/temp/bench
mkdir -p "$out"

if [ "${BENCH_BUILD:-1}" = 1 ] && [ -z "${DBOTA_BIN:-}" ]; then
    cargo build --profile "$profile" --quiet --manifest-path "$here/Cargo.toml"
fi
if [ "${BENCH_WASM:-0}" = 1 ]; then
    cargo build --profile web --quiet --target wasm32-unknown-unknown --manifest-path "$here/Cargo.toml"
fi

# Drops the asset cache and the binary from the page cache: a cold start.
drop_caches() {
    python3 -I -c 'import os,sys
for top in sys.argv[1:]:
    paths=[top] if os.path.isfile(top) else [os.path.join(d,f) for d,_,fs in os.walk(top) for f in fs]
    for p in paths:
        try:
            fd=os.open(p,os.O_RDONLY); os.posix_fadvise(fd,0,0,os.POSIX_FADV_DONTNEED); os.close(fd)
        except OSError: pass' "$cache" "$bin"
}

# Highest GPU memory, in MiB, nvidia-smi shows for a process while it runs.
watch_vram() {
    local pid=$1 file=$2 peak=0 now
    command -v nvidia-smi > /dev/null || { echo "-" > "$file"; return; }
    while kill -0 "$pid" 2> /dev/null; do
        now=$(nvidia-smi 2> /dev/null | awk -v pid="$pid" '$5 == pid { gsub("MiB", "", $(NF-1)); print $(NF-1) }' | head -n1)
        if [ -n "$now" ] && [ "$now" -gt "$peak" ]; then peak=$now; fi
        sleep 1
    done
    echo "$peak" > "$file"
}

# Runs dbota with the given arguments; prints its bench line with the peak
# VRAM appended.
run() {
    local name=$1
    shift
    "${wrap[@]}" "$bin" --mute "${extra[@]}" "$@" > "$out/$name.out" 2> "$out/$name.log" &
    local pid=$!
    watch_vram "$pid" "$out/$name.vram" &
    local watcher=$!
    local status=0
    wait "$pid" || status=$?
    wait "$watcher"
    local line
    line=$(grep -m1 '^bench:' "$out/$name.out" || true)
    if [ "$status" -ne 0 ] || [ -z "$line" ]; then
        echo "bench: $name failed (exit $status), see $out/$name.log" >&2
        return 1
    fi
    echo "$line vram_mb=$(cat "$out/$name.vram")"
}

field() { sed -n "s/.* $2=\([^ ]*\).*/\1/p" <<< "$1"; }

# The median run of several, by mean frame time.
median() {
    printf '%s\n' "$@" | awk '{ for (i = 1; i <= NF; i++) if ($i ~ /^mean_ms=/) { split($i, kv, "="); print kv[2] "\t" $0 } }' \
        | sort -n | awk -v n="$#" 'NR == int((n + 1) / 2) { sub(/^[^\t]*\t/, ""); print }'
}

size=$(stat -c %s "$bin")
strip -o "$out/dbota.stripped" "$bin"
stripped=$(stat -c %s "$out/dbota.stripped")
wasm=$here/target/wasm32-unknown-unknown/web/dbota.wasm
wasm_size=$([ -f "$wasm" ] && stat -c %s "$wasm" || echo "-")

scene=(--replay "$replay" --seek "$tick" --follow 0 --distance "$distance" --bench "$seconds")
drop_caches
cold=$(run cold "${scene[@]}" --no-vsync)
lines=()
for i in $(seq 1 "$runs"); do
    lines+=("$(run "replay-$i" "${scene[@]}" --no-vsync)")
done
replay_line=$(median "${lines[@]}")
vsync="-"
if [ "${BENCH_VSYNC:-1}" = 1 ]; then
    vsync=$(run replay-vsync "${scene[@]}")
fi
live="-"
if [ -n "$server" ] && [ -x "$server" ]; then
    port=$((20000 + RANDOM % 20000))
    "$server" --map 2 --players 2 --dummy-seat --port "$port" > "$out/server.log" 2>&1 &
    server_pid=$!
    sleep 1
    live=$(DBOTA_DRILL=bench run live --addr "127.0.0.1:$port" --no-vsync \
        --distance "$distance" --bench "$seconds" --bench-warmup 40 || echo "-")
    kill "$server_pid" 2> /dev/null || true
    wait "$server_pid" 2> /dev/null || true
fi

mb() { awk -v b="$1" 'BEGIN { if (b == "-") print "-"; else printf "%.1f", b / 1048576 }'; }
echo "| metric | value |"
echo "| --- | --- |"
echo "| binary (MiB, stripped) | $(mb "$size") ($(mb "$stripped")) |"
echo "| wasm (MiB) | $(mb "$wasm_size") |"
echo "| cold start: first frame / scene whole (s) | $(field "$cold" first_frame_s) / $(field "$cold" ready_s) |"
echo "| warm start: first frame / scene whole (s) | $(field "$replay_line" first_frame_s) / $(field "$replay_line" ready_s) |"
for row in "replay|$replay_line" "replay, vsync|$vsync" "live|$live"; do
    name=${row%%|*}
    line=${row#*|}
    if [ "$line" = "-" ]; then
        echo "| $name | - |"
        continue
    fi
    echo "| $name: frame mean / p99 (ms) | $(field "$line" mean_ms) / $(field "$line" p99_ms) |"
    echo "| $name: CPU (% of a core) | $(field "$line" cpu_pct) |"
    echo "| $name: RAM now / peak (MiB) / VRAM peak (MiB) | $(field "$line" rss_mb) / $(field "$line" peak_rss_mb) / $(field "$line" vram_mb) |"
done
