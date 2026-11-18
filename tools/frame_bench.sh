#!/usr/bin/env bash
# Measures how smoothly the client draws a match against a 240 Hz display: frame time percentiles
# against the 4.17 ms period, the frames over it, over twice and four times it (with what was made
# in each: pipelines compiled, assets loaded), and how evenly the frames come. Prints a Markdown
# table; every number comes from one `bench:` line of dbota's `--bench` (see src/framestats.rs).
#
#   tools/frame_bench.sh NAME=REPLAY.brp@TICK[@SEAT] ...
#
# Each scene is a replay seen from SEAT's hero (default 0) from TICK on, at the camera's default
# distance and the default settings (what a player sees first). Every scene is drawn at every size,
# in both modes:
#   vsync   presented on the display's pace (Weston's headless output at FRAME_HZ): the frame
#           intervals a player sees, their spread being the pacing
#   free    presented without vsync: what a frame costs, how far under the period it stays
#
# Settings (environment):
#   FRAME_SIZES="3840x2160 2560x1440"   window sizes (borderless on an output of that size)
#   FRAME_MODES="vsync free"
#   FRAME_HZ=240                         the output's refresh rate and the rate frames are judged by
#   FRAME_SECONDS=10                     seconds of frames timed per run, after 2 s of warm-up
#   FRAME_ROUNDS=1                       runs of each row; the table shows each
#   FRAME_ARGS=""                        extra dbota arguments for every run (`--set shadows=1`)
#   DBOTA_BIN                            the binary (default <CARGO_TARGET_DIR or target>/release/dbota)
#   DBOTA_ASSETS                         the asset cache (required, as for tools/bench.sh)
# Wrap the call in the machine's benchmark lock where one exists: frame times on a busy machine say
# little. Runs use a private home, as tools/bench.sh does.
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
# shellcheck source=tools/client_env.sh
. "$here/tools/client_env.sh"
if [ $# -eq 0 ]; then
    echo "usage: tools/frame_bench.sh NAME=REPLAY.brp@TICK[@SEAT] ..." >&2
    exit 2
fi
read -r -a sizes <<< "${FRAME_SIZES:-3840x2160 2560x1440}"
read -r -a modes <<< "${FRAME_MODES:-vsync free}"
read -r -a extra <<< "${FRAME_ARGS:-}"
hz=${FRAME_HZ:-240}
seconds=${FRAME_SECONDS:-10}
rounds=${FRAME_ROUNDS:-1}
require_test_asset_cache || exit 1
bin=$(client_binary release)
target=${CARGO_TARGET_DIR:-$here/target}
out=$here/temp/frame_bench
mkdir -p "$out" "$target"

# One run inside an output of the row's size; prints its bench line.
run() {
    local name=$1 size=$2 mode=$3 replay=$4 tick=$5 seat=$6 vsync=()
    [ "$mode" = free ] && vsync=(--no-vsync)
    local log="$out/$name-$size-$mode"
    if ! DBOTA_HEADLESS_SIZE=$size DBOTA_HEADLESS_HZ=$hz "$here/tools/headless.sh" \
        bash -c '. "$1/tools/client_env.sh"; shift; home=$(mktemp -d "$1/frame-bench.XXXXXX"); shift
            private_client_environment "$home" "$1/tmp/gpu-cache"; shift
            "$@"; status=$?; rm -r -- "$home"; exit $status' \
        _ "$here" "$target" "$target" "$bin" --mute --display-mode borderless "${vsync[@]}" "${extra[@]}" \
        --replay "$replay" --seek "$tick" --follow "$seat" --bench "$seconds" --bench-hz "$hz" \
        > "$log.out" 2> "$log.log"; then
        echo "frame_bench: $name $size $mode failed, see $log.log" >&2
        return 1
    fi
    grep -m1 '^bench:' "$log.out" || { echo "frame_bench: no bench line in $log.out" >&2; return 1; }
}

field() { sed -n "s/.* $2=\([^ ]*\).*/\1/p" <<< "$1"; }

echo "| scene | size | mode | frames/s | p50 / p99 / p99.9 / max (ms) | over 1x / 2x / 4x | hitches: pipelines / assets / other | interval sd / jump (ms) | main / render (ms) |"
echo "| --- | --- | --- | --- | --- | --- | --- | --- | --- |"
for round in $(seq 1 "$rounds"); do
    for scene in "$@"; do
        name=${scene%%=*}
        spec=${scene#*=}
        IFS=@ read -r replay tick seat <<< "$spec"
        replay=$(realpath -- "$replay")
        for size in "${sizes[@]}"; do
            for mode in "${modes[@]}"; do
                line=$(run "$name" "$size" "$mode" "$replay" "$tick" "${seat:-0}") || continue
                fps=$(awk -v m="$(field "$line" mean_ms)" 'BEGIN { printf "%.0f", 1000 / m }')
                echo "| $name | $size | $mode | $fps | $(field "$line" p50_ms) / $(field "$line" p99_ms) / $(field "$line" p999_ms) / $(field "$line" max_ms) | $(field "$line" over_budget) / $(field "$line" over_2x) / $(field "$line" over_4x) | $(field "$line" hitch_pipelines) / $(field "$line" hitch_assets) / $(field "$line" hitch_other) | $(field "$line" interval_sd_ms) / $(field "$line" interval_jump_ms) | $(field "$line" main_mean_ms) / $(field "$line" render_mean_ms) |"
            done
        done
    done
done
