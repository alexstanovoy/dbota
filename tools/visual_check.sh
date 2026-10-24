#!/usr/bin/env bash
# Measures dbota's picture of a recorded match (src/visual.rs): the fog's
# density and border, heroes drawn with their models, buildings that never
# turn, no range rings without Alt, no gold or souls in the corner, round buff
# icons, the river one surface without seams. Takes a bota replay (.brp, first
# argument or VISUAL_REPLAY) of a Map 2 match whose heroes are on the map at
# VISUAL_TICK, the asset cache, and a display (DISPLAY or WAYLAND_DISPLAY).
# Exits non-zero when a check fails.
# The fog checks compare each ground cell on screen drawn with the fog and
# without it (a second shot of the same held frame), so they hold at any
# VISUAL_TICK (4000, day, is the default; 12000 is night).
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
replay=${1:-${VISUAL_REPLAY:-}}
out=${VISUAL_DIR:-$here/temp/visual}
tick=${VISUAL_TICK:-4000}
if [ -z "$replay" ] || [ ! -f "$replay" ]; then
    echo "usage: tools/visual_check.sh REPLAY.brp (or VISUAL_REPLAY=...)" >&2
    exit 2
fi
mkdir -p "$out"
cargo build --release --quiet --manifest-path "$here/Cargo.toml"
status=0
"$here/target/release/dbota" --replay "$replay" --pov 0 --screenshot-at "$tick" \
    --screenshot-dir "$out" --screenshot-prefix check- --distance 6500 \
    --visual-check --mute --no-vsync --display-mode windowed 2> "$out/dbota.log" | tee "$out/dbota.out" || status=$?
[ "$status" -eq 0 ] || exit "$status"
# The same seen by the other side, one window more.
dire=$("$here/target/release/dbota" --replay "$replay" --pov 1 --screenshot-at "$tick" \
    --screenshot-dir "$out" --screenshot-prefix check-dire- --distance 6500 \
    --visual-check --mute --no-vsync --display-mode windowed 2>> "$out/dbota.log" || true)
echo "$dire" | sed 's/^visual check: /visual check (Dire): /'
if echo "$dire" | grep -q "FAIL"; then exit 1; fi

# The river is one surface: the camera on it where the mid lane crosses it
# (VISUAL_RIVER, bota world units), at the same tick, its seams measured.
river=$("$here/target/release/dbota" --replay "$replay" --look "${VISUAL_RIVER:-8000,9600}" --distance 2500 \
    --screenshot-at "$tick" --screenshot-dir "$out" --screenshot-prefix check-river- \
    --visual-check --water-check --mute --no-vsync --display-mode windowed 2>> "$out/dbota.log" || true)
echo "$river"
if ! echo "$river" | grep -q "PASS the river"; then exit 1; fi

# Every hero model loads whole every time, on its hero's own skeleton: the
# client is run again, cold (the asset cache dropped from memory) and warm
# by turns, with the animated portrait (a second copy of the model) on and
# off by turns, and each run's count of hero meshes must match the first's
# and its hero-part checks pass.
runs=${VISUAL_RUNS:-1}
cache=${DBOTA_ASSETS:-$HOME/.cache/dbota/v$(tr -d '[:space:]' < "$here/assets-manifest/version")}
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
    portrait=$([ $((run % 4)) -ge 2 ] && echo true || echo false)
    checks=$("$here/target/release/dbota" --replay "$replay" --pov 0 --screenshot-at "$tick" \
        --screenshot-dir "$out" --screenshot-prefix repeat- --distance 6500 \
        --set animate_portrait="$portrait" --visual-check --mute --no-vsync --display-mode windowed 2>/dev/null || true)
    line=$(echo "$checks" | grep -m1 "hero parts:" || true)
    if [ "$line" != "$first" ]; then
        echo "visual check: FAIL hero parts differ on run $run: '$line' against '$first'"
        exit 1
    fi
    failed=$(echo "$checks" | grep -E "FAIL (hero parts|no model scene)" || true)
    if [ -n "$failed" ]; then
        echo "visual check: FAIL on run $run (animated portrait $portrait): $failed"
        exit 1
    fi
done
echo "visual check: PASS hero parts identical and on their skeletons over $runs runs: ${first#*hero parts: }"

# A system whose fontconfig lists no font at all still starts, on the shipped fonts: the CJK
# fallback is registered and the picture is taken.
echo '<?xml version="1.0"?><fontconfig></fontconfig>' > "$out/no-fonts.conf"
if FONTCONFIG_FILE="$out/no-fonts.conf" "$here/target/release/dbota" --replay "$replay" --pov 0 \
    --screenshot-at "$tick" --screenshot-dir "$out" --screenshot-prefix nofonts- --mute --no-vsync \
    > "$out/nofonts.out" 2> "$out/nofonts.log" \
    && grep -q "stands behind the Latin ones" "$out/nofonts.log" \
    && ! grep -q panicked "$out/nofonts.log"; then
    echo "visual check: PASS starts without any system font, on the shipped ones"
else
    echo "visual check: FAIL no start without system fonts (see $out/nofonts.log)"
    exit 1
fi

# A replay the client refuses ends it cleanly, never a crash, however early it is refused.
if cargo test --release --quiet --manifest-path "$here/Cargo.toml" --test refused_replays \
    -- --ignored > "$out/refused.log" 2>&1; then
    echo "visual check: PASS refused replays end the client cleanly"
else
    echo "visual check: FAIL a refused replay did not end the client cleanly (see $out/refused.log)"
    exit 1
fi
