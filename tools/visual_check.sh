#!/usr/bin/env bash
# Records a short match of two bota bots and measures dbota's picture of it
# (src/visual.rs): the fog's density and border, heroes drawn with their
# models, buildings that never turn, no range rings without Alt, no gold or
# souls in the corner, round buff icons. Needs bota checked out next to
# dbota, the asset cache, and a display (DISPLAY or WAYLAND_DISPLAY).
# Exits non-zero when a check fails.
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
bota=${BOTA_DIR:-$here/../bota}
out=${VISUAL_DIR:-$here/temp/visual}
port=${VISUAL_PORT:-4597}
tick=${VISUAL_TICK:-4000}
mkdir -p "$out"
cargo build --release --quiet --manifest-path "$bota/Cargo.toml" -p bota-server -p bota-bot
cargo build --release --quiet --manifest-path "$here/Cargo.toml"
replay=$out/visual.brp
rm -f "$replay"
"$bota/target/release/bota-server" --port "$port" --mode lockstep --players 2 --map 2 \
    --seed 424242 --replay "$replay" > "$out/server.log" 2>&1 &
server=$!
trap 'kill $server 2>/dev/null || true' EXIT
sleep 0.5
fiend=$("$bota/target/release/bota-bot" --help | sed -n 's/.*\([0-9]\) Shadow Fiend.*/\1/p')
bots=()
for seat in 1 2; do
    "$bota/target/release/bota-bot" --addr "127.0.0.1:$port" --name "bot$seat" \
        --hero "${fiend:-1}" --limit $((tick + 300)) > "$out/bot$seat.log" 2>&1 &
    bots+=($!)
done
wait "${bots[@]}" || true
kill $server 2>/dev/null || true
wait $server 2>/dev/null || true
status=0
"$here/target/release/dbota" --replay "$replay" --pov 0 --screenshot-at "$tick" \
    --screenshot-dir "$out" --screenshot-prefix check- --distance 6500 \
    --visual-check --mute --no-vsync 2> "$out/dbota.log" | tee "$out/dbota.out" || status=$?
[ "$status" -eq 0 ] || exit "$status"

# Every hero model loads whole every time: the client is run again, cold
# (the asset cache dropped from memory) and warm by turns, and each run's
# count of hero meshes must match the first's.
runs=${VISUAL_RUNS:-1}
cache=${DBOTA_ASSETS:-$HOME/.cache/dbota}
first=$(grep -m1 "hero parts:" "$out/dbota.out" 2>/dev/null || true)
for run in $(seq 2 "$runs"); do
    if [ $((run % 2)) -eq 0 ]; then
        python3 -c 'import os,sys
for d,_,fs in os.walk(sys.argv[1]):
    for f in fs:
        try:
            fd=os.open(os.path.join(d,f),os.O_RDONLY); os.posix_fadvise(fd,0,0,os.POSIX_FADV_DONTNEED); os.close(fd)
        except OSError: pass' "$cache"
    fi
    line=$("$here/target/release/dbota" --replay "$replay" --pov 0 --screenshot-at "$tick" \
        --screenshot-dir "$out" --screenshot-prefix repeat- --distance 6500 \
        --visual-check --mute --no-vsync 2>/dev/null | grep -m1 "hero parts:" || true)
    if [ "$line" != "$first" ]; then
        echo "visual check: FAIL hero parts differ on run $run: '$line' against '$first'"
        exit 1
    fi
done
echo "visual check: PASS hero parts identical over $runs runs: ${first#*hero parts: }"
