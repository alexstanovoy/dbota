#!/usr/bin/env bash
# Measures dbota's picture of a recorded match (src/visual.rs): the fog's
# density and border, heroes drawn with their models, buildings that never
# turn, no range rings without Alt, only the menu in the corner, health bars
# in their side's colour with their health marks and blocks, round buff icons, the river one surface without
# seams. Takes a bota replay (.brp, first
# argument or VISUAL_REPLAY) of a Map 2 match whose heroes are on the map at
# VISUAL_TICK and the asset cache. Every window it opens is in a private
# headless compositor on the GPU (tools/headless.sh): none shows on the
# desktop; DBOTA_SHOW=1 shows them. Exits non-zero when a check fails.
# The fog checks compare each ground cell on screen drawn with the fog and
# without it (a second shot of the same held frame), so they hold at any
# VISUAL_TICK (4000, day, is the default; 12000 is night).
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
# shellcheck source=tools/client_env.sh
. "$here/tools/client_env.sh"
# The whole check in one headless compositor: the clients it starts and the tests' share it.
[ -n "${DBOTA_HEADLESS:-}" ] || [ "${DBOTA_SHOW:-0}" = 1 ] || exec "$here/tools/headless.sh" "$0" "$@"
replay=${1:-${VISUAL_REPLAY:-}}
out=${VISUAL_DIR:-$here/temp/visual}
tick=${VISUAL_TICK:-4000}
if [ -z "$replay" ] || [ ! -f "$replay" ]; then
    echo "usage: tools/visual_check.sh REPLAY.brp (or VISUAL_REPLAY=...)" >&2
    exit 2
fi
mkdir -p "$out"
: "${DBOTA_ASSETS:?DBOTA_ASSETS must name a test asset cache}"
export DBOTA_ASSETS
cargo build --release --quiet --manifest-path "$here/Cargo.toml"
# A run's checks hold when it said at least one passed check and no failed one: a client that
# ended before its verdict says neither.
held() { echo "$1" | grep -q "^visual check: PASS" && ! echo "$1" | grep -q "FAIL"; }
# An ignored-test run holds when each of its test binaries ran a test and all passed: a filter or
# a target that matches nothing passes with none run.
ran() { awk '/^test result:/ { n++; if ($4 + 0 < 1) none = 1 } END { exit !(n > 0 && !none) }' "$1"; }
# Every world indicator uses this production stroke configuration.
if in_private_environment cargo test --profile test-release --quiet --manifest-path "$here/Cargo.toml" \
    --bin dbota overlay::render_tests::indicator_strokes_are_two_pixels_at_a_fixed_camera \
    -- --ignored --exact --nocapture > "$out/indicator_strokes.log" 2>&1 \
    && grep -Fq 'test result: ok. 1 passed; 0 failed; 0 ignored;' "$out/indicator_strokes.log"; then
    echo "visual check: PASS indicator stroke pixels"
else
    echo "visual check: FAIL indicator stroke pixels (see $out/indicator_strokes.log)"
    exit 1
fi
# Minimap borders are measured after UI scaling and rasterization, including odd widths.
if in_private_environment cargo test --profile test-release --quiet --manifest-path "$here/Cargo.toml" \
    --bin dbota minimap::render_tests::minimap_camera_ping_and_teleport_strokes_halve_physical_widths_above_one_pixel \
    -- --ignored --exact --nocapture > "$out/minimap_strokes.log" 2>&1 \
    && grep -Fq 'test result: ok. 1 passed; 0 failed; 0 ignored;' "$out/minimap_strokes.log"; then
    echo "visual check: PASS minimap stroke pixels"
else
    echo "visual check: FAIL minimap stroke pixels (see $out/minimap_strokes.log)"
    exit 1
fi
# The objective effects contribute visible pixels in each actual recorded state.
if in_private_environment cargo test --profile test-release --quiet --manifest-path "$here/Cargo.toml" \
    --bin dbota e2e::ground_effect_pixels::four_objective_states_have_visible_authored_effects \
    -- --ignored --exact --nocapture > "$out/objective_pixels.log" 2>&1 \
    && grep -Fq 'test result: ok. 1 passed; 0 failed; 0 ignored;' "$out/objective_pixels.log"; then
    echo "visual check: PASS objective effect pixels"
else
    echo "visual check: FAIL objective effect pixels (see $out/objective_pixels.log)"
    exit 1
fi
# Full readability is baked into the production material, including with its retired uniform at zero.
if in_private_environment cargo test --profile test-release --quiet --manifest-path "$here/Cargo.toml" \
    --bin dbota fog::clarity_render::full_visual_clarity_reaches_the_ground_and_figure_pixels \
    -- --ignored --exact --nocapture > "$out/clarity.log" 2>&1 \
    && grep -Fq 'test result: ok. 1 passed; 0 failed; 0 ignored;' "$out/clarity.log"; then
    echo "visual check: PASS full readability reaches ground and figure pixels"
else
    echo "visual check: FAIL full readability material (see $out/clarity.log)"
    exit 1
fi
# The tooltip's header band, drawn from the asset cache: its art starts at the band's left end, unscaled.
if in_private_environment cargo test --profile test-release --quiet --manifest-path "$here/Cargo.toml" \
    --bin dbota tip_band::render::each_band_draws_its_art_from_the_left_end_unscaled_at_every_scale \
    -- --ignored --exact --nocapture > "$out/tip_band.log" 2>&1 \
    && grep -Fq 'test result: ok. 1 passed; 0 failed; 0 ignored;' "$out/tip_band.log"; then
    echo "visual check: PASS the tooltip's header bands start at their left end"
else
    echo "visual check: FAIL the tooltip's header bands (see $out/tip_band.log)"
    exit 1
fi
status=0
"$here/tools/test_client.sh" --replay "$replay" --pov 0 --screenshot-at "$tick" \
    --screenshot-dir "$out" --screenshot-prefix check- --distance 6500 \
    --visual-check --mute --no-vsync --display-mode windowed 2> "$out/dbota.log" | tee "$out/dbota.out" || status=$?
[ "$status" -eq 0 ] || exit "$status"
held "$(cat "$out/dbota.out")" || { echo "visual check: FAIL the first seat's checks gave no verdict"; exit 1; }
# The same seen by the other side, one window more.
dire_status=0
dire=$("$here/tools/test_client.sh" --replay "$replay" --pov 1 --screenshot-at "$tick" \
    --screenshot-dir "$out" --screenshot-prefix check-dire- --distance 6500 \
    --visual-check --mute --no-vsync --display-mode windowed 2>> "$out/dbota.log") || dire_status=$?
echo "$dire" | sed 's/^visual check: /visual check (Dire): /'
[ "$dire_status" -eq 0 ] || { echo "visual check: FAIL the Dire seat's client ended with $dire_status"; exit 1; }
held "$dire" || { echo "visual check: FAIL the Dire seat's checks did not all pass"; exit 1; }

# The cheapest preset is measured as the full one is: its scene drawn below the window's
# resolution, without shadows or edge smoothing, must still pass the fog, hero and interface checks. A client that ends or draws a black scene fails them; one
# that says no check at all fails too.
for preset in low; do
    shown=$("$here/tools/test_client.sh" --replay "$replay" --pov 0 --screenshot-at "$tick" \
        --screenshot-dir "$out" --screenshot-prefix "check-$preset-" --distance 6500 --set quality="$preset" \
        --visual-check --mute --no-vsync --display-mode windowed 2>> "$out/dbota.log" || echo "visual check: FAIL the client ended")
    echo "$shown" | sed "s/^visual check: /visual check ($preset): /"
    held "$shown" || exit 1
done

# The river is one surface: the camera on it where the mid lane crosses it
# (VISUAL_RIVER, bota world units), nearer than the camera starts, at the
# same tick, its seams measured.
river=$("$here/tools/test_client.sh" --replay "$replay" --look "${VISUAL_RIVER:-8000,9600}" --distance 2500 \
    --screenshot-at "$tick" --screenshot-dir "$out" --screenshot-prefix check-river- \
    --visual-check --water-check --mute --no-vsync --display-mode windowed 2>> "$out/dbota.log" || true)
echo "$river"
if ! echo "$river" | grep -q "PASS the river"; then exit 1; fi

# The health bars near: the camera on the first seat's courier, whose health is counted in hits,
# every bar's marks and every bar of blocks measured.
bars=$("$here/tools/test_client.sh" --replay "$replay" --pov 0 --bars-check --distance 2500 \
    --screenshot-at "$tick" --screenshot-dir "$out" --screenshot-prefix check-bars- \
    --visual-check --mute --no-vsync --display-mode windowed 2>> "$out/dbota.log" || true)
echo "$bars"
if ! echo "$bars" | grep -q "PASS health counted in hits"; then exit 1; fi

# Every hero model loads whole every time, on its hero's own skeleton: the
# client is run again, cold (the asset cache dropped from memory) and warm
# by turns, with the animated portrait (a second copy of the model) on and
# off by turns, and each run's count of hero meshes must match the first's
# and its hero-part checks pass.
runs=${VISUAL_RUNS:-1}
cache=$DBOTA_ASSETS
grep -q "PASS hero parts" "$out/dbota.out" || { echo "visual check: FAIL the first run's hero parts did not pass"; exit 1; }
first=$(grep -m1 "visual check: hero parts:" "$out/dbota.out" 2>/dev/null || true)
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
    repeat_status=0
    checks=$("$here/tools/test_client.sh" --replay "$replay" --pov 0 --screenshot-at "$tick" \
        --screenshot-dir "$out" --screenshot-prefix repeat- --distance 6500 \
        --set animate_portrait="$portrait" --visual-check --mute --no-vsync --display-mode windowed 2>/dev/null) \
        || repeat_status=$?
    if [ "$repeat_status" -ne 0 ] || ! echo "$checks" | grep -q "PASS hero parts"; then
        echo "visual check: FAIL run $run (animated portrait $portrait) ended with $repeat_status, hero parts not passed"
        exit 1
    fi
    line=$(echo "$checks" | grep -m1 "visual check: hero parts:" || true)
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

# Each hero's portrait says a line with its face (src/talk_check.rs); its frames idle,
# talking and checked are saved in the output folder.
for hero in pudge nevermore; do
    talk=$("$here/tools/test_client.sh" --replay "$replay" --pov 0 --talk-check "$hero" \
        --screenshot-dir "$out" --screenshot-prefix portrait- --mute --no-vsync \
        --display-mode windowed 2>> "$out/dbota.log" || true)
    echo "$talk" | grep "talking portrait" || echo "visual check: FAIL talking portrait $hero: no verdict"
    if ! echo "$talk" | grep -q "PASS talking portrait"; then exit 1; fi
done

# A system whose fontconfig lists no font at all still starts, on the shipped fonts: the CJK
# fallback is registered and the picture is taken.
echo '<?xml version="1.0"?><fontconfig></fontconfig>' > "$out/no-fonts.conf"
if FONTCONFIG_FILE="$out/no-fonts.conf" "$here/tools/test_client.sh" --replay "$replay" --pov 0 \
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
if in_private_environment cargo test --profile test-release --quiet --manifest-path "$here/Cargo.toml" --test refused_replays \
    -- --ignored > "$out/refused.log" 2>&1 && ran "$out/refused.log"; then
    echo "visual check: PASS refused replays end the client cleanly"
else
    echo "visual check: FAIL a refused replay did not end the client cleanly (see $out/refused.log)"
    exit 1
fi

# The models of a match load on one core: Bevy's loaders nest on the few loader threads that
# machine gets, and overflowed their default stacks (src/io_pool.rs).
if VISUAL_REPLAY="$(realpath "$replay")" in_private_environment cargo test --profile test-release --quiet --manifest-path "$here/Cargo.toml" \
    --test io_stack -- --ignored > "$out/io_stack.log" 2>&1 && ran "$out/io_stack.log"; then
    echo "visual check: PASS a match loads on one core"
else
    echo "visual check: FAIL a match did not load on one core (see $out/io_stack.log)"
    exit 1
fi

# The way out: Escape on the menu asks to quit and OK ends the client with exit code 0; Escape in
# a live game opens the dashboard, DISCONNECT leaves for the menu and RECONNECT takes the seat
# again; a seat that LEAVES GAME ends the match as its loss and the other seat wins; a replay is left for the
# menu; Demo Hero plays a hero with every cheat; a prepared asset cache (a folder, a zip) is taken by
# `dbota import-cache` and by a path typed into the menu, and a cache of another build is refused. One test at a time: each opens a window of its own
# (in this check's compositor).
if VISUAL_REPLAY="$(realpath "$replay")" DBOTA_QUIT_SHOTS="$out/quit" in_private_environment cargo test --profile test-release --quiet \
    --manifest-path "$here/Cargo.toml" --test menu_quit -- --ignored --test-threads 1 \
    > "$out/quit.log" 2>&1 && ran "$out/quit.log"; then
    echo "visual check: PASS the menu, a game and a replay are quit and left as Dota's are; Demo Hero cheats"
else
    echo "visual check: FAIL the way out (see $out/quit.log)"
    exit 1
fi

# How an attack looks and sounds: a swing broken off is a cancel, clips cross-fade over 0.2 s, melee
# creeps whoosh and hit, the Ogre Bruiser plays its Smash through its cast point. Each test records a
# match on a real bota_server and plays its replay with the client's trace.
if in_private_environment cargo test --profile test-release --quiet --manifest-path "$here/Cargo.toml" --test attack_feel \
    -- --ignored --test-threads 1 > "$out/attack_feel.log" 2>&1 && ran "$out/attack_feel.log"; then
    echo "visual check: PASS a broken-off swing is a cancel, clips fade, creeps sound, the Smash plays through"
else
    echo "visual check: FAIL how an attack looks and sounds (see $out/attack_feel.log)"
    exit 1
fi

# Selling in the strategy phase: the item menu's Sell takes a chosen item back, so the match begins
# with the one item left.
if in_private_environment cargo test --profile test-release --quiet --manifest-path "$here/Cargo.toml" --test item_sell \
    -- --ignored > "$out/sell.log" 2>&1 && ran "$out/sell.log"; then
    echo "visual check: PASS an item sold in the strategy phase is not bought"
else
    echo "visual check: FAIL selling in the strategy phase (see $out/sell.log)"
    exit 1
fi

# What a match draws of the items and effects owners reported missing, asked of the running client
# (one test at a time, each in a window of its own): a placed ward in Dota's ward model with its
# ambient effect, a dropped item on the ground with its model, a Blink Dagger's two effects and a
# Blood Grenade's throw, landing and splatter, a tower's line to a hero in its reach, and the red
# miss notice over a hero struck uphill.
if in_private_environment cargo test --profile test-release --quiet --manifest-path "$here/Cargo.toml" --test wards --test ground_items \
    --test blink --test blood_grenade --test tower_line --test miss_notice -- --ignored --test-threads 1 > "$out/effects.log" 2>&1 && ran "$out/effects.log"; then
    echo "visual check: PASS wards, items on the ground, a blink, a grenade, a tower's line and a miss notice are drawn and sounded"
else
    echo "visual check: FAIL wards, ground items, a blink, a grenade, a tower's line or a miss notice (see $out/effects.log)"
    exit 1
fi

# The ping in the corner is the line's: a server behind a delayed line, read off the drawn text.
if in_private_environment cargo test --profile test-release --quiet --manifest-path "$here/Cargo.toml" --test ping \
    -- --ignored --test-threads 1 > "$out/ping.log" 2>&1 && ran "$out/ping.log"; then
    echo "visual check: PASS the corner tells the line's ping, not a multiple of the tick"
else
    echo "visual check: FAIL the ping in the corner (see $out/ping.log)"
    exit 1
fi

# The talking portrait in a game: one's hero answers a move and an attack order with a voice line,
# and the portrait's jaw moves with the line's phonemes and rests once it has ended, for each hero.
if in_private_environment cargo test --profile test-release --quiet --manifest-path "$here/Cargo.toml" --test portrait_talk \
    -- --ignored --test-threads 1 --nocapture > "$out/talk.log" 2>&1 && ran "$out/talk.log"; then
    echo "visual check: PASS each hero says its move and attack lines with its face"
else
    echo "visual check: FAIL the talking portrait in a game (see $out/talk.log)"
    exit 1
fi
