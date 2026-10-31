#!/usr/bin/env bash
# The browser checks of the merge gate: builds the web page in the variants the checks need and runs every check of
# web/test/smoke.py in one Playwright run (headless Chromium, Firefox for the boot check).
#
#   tools/browser_check.sh
#
# Needs the asset cache of this build (DBOTA_ASSETS, made by `dbota import`), a Dota 2 install for the import test, the
# bota and radovid checkouts beside this one (DBOTA_GATE_BOTA, DBOTA_GATE_RADOVID name others), and the native release
# build of dbota (the replay with chat lines is recorded by a headless client). Playwright and its browsers are fetched once
# into DBOTA_GATE_PWLIB (default ~/.cache/dbota-gate/pwlib). Any check that cannot run fails; nothing is skipped.
# Output goes to GATE_LOGS/browser (default temp/gate/browser).
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$here"
bota=${DBOTA_GATE_BOTA:-$here/../bota}
radovid=${DBOTA_GATE_RADOVID:-$here/../radovid}
: "${DBOTA_ASSETS:?DBOTA_ASSETS: the asset cache of this build}"
work=${GATE_LOGS:-$here/temp/gate}/browser
mkdir -p "$work"
pwlib=${DBOTA_GATE_PWLIB:-${XDG_CACHE_HOME:-$HOME/.cache}/dbota-gate/pwlib}
export PYTHONPATH=$pwlib PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1
if ! python3 -I -c 'import sys; sys.path.insert(0, sys.argv[1]); import playwright' "$pwlib" 2> /dev/null; then
    python3 -m pip install --quiet --target "$pwlib" playwright
fi
python3 -m playwright install chromium firefox > "$work/playwright-install.log" 2>&1 \
    || { tail -n 20 "$work/playwright-install.log"; echo "browser_check: the browsers cannot be installed" >&2; exit 1; }

# The servers: bota's (a match with a replay) and radovid's (the replays page).
cargo build --release --quiet --manifest-path "$bota/Cargo.toml" -p bota_server
cargo build --release --quiet --manifest-path "$radovid/Cargo.toml" -p radovid
target_of() { cargo metadata --format-version 1 --no-deps --manifest-path "$1/Cargo.toml" | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])'; }
server=$(target_of "$bota")/release/bota_server
radovid_bin=$(target_of "$radovid")/release/radovid

# A replay with chat lines (the replay page checks that they are heard): a fast match, one headless client that says six lines.
chat=$work/chat.brp
rm -f "$chat" "$work/server.out"
cargo build --release --quiet
"$server" --port 0 --bind 127.0.0.1 --mode realtime --tick-rate 120 --map 2 --players 2 --dummy-seat --pick-secs 1 \
    --tactical-secs 0 --seed 7 --empty-after-secs 4 --replay "$chat" > "$work/server.out" 2>&1 &
srv=$!
addr=
for _ in $(seq 100); do
    addr=$(sed -n 's/^bota_server listening on //p' "$work/server.out" | head -1)
    [ -n "$addr" ] && break
    sleep 0.3
done
[ -n "$addr" ] || { kill "$srv" 2> /dev/null; echo "browser_check: the server did not start"; cat "$work/server.out"; exit 1; }
"$here/tools/headless.sh" target/release/dbota --addr "$addr" --config "$work/chat.conf" --mute --no-vsync --display-mode windowed \
    --script "await:game;wait:30;say:one;wait:1;say:two;wait:1;say:three;wait:1;say:four;wait:1;say:five;wait:1;say:six;wait:20;exit" \
    > "$work/chat-client.out" 2>&1 || { kill "$srv" 2> /dev/null; tail -n 20 "$work/chat-client.out"; echo "browser_check: the chat client failed"; exit 1; }
wait "$srv" || true
[ -s "$chat" ] || { echo "browser_check: no replay was recorded"; exit 1; }

# The pages: one without a replay (the boot, startup, served-cache and key checks, the radovid replays page),
# one that plays the chat replay (the import test, replay, cursor, real-input cursor, pixels, prepared cache) and one that opens on the Video settings.
export PROFILE=release
pages=temp/gate-pages
rm -rf "$pages"
OUT=$pages/plain web/build.sh
# (both clients: the real-input cursor check runs the page on the WebGL2 and the WebGPU one)
DBOTA_WEB_REPLAY=$chat DBOTA_WEB_ARGS="--replay built-in --pov 0 --seek 4600" OUT=$pages/replay web/build.sh
DBOTA_WEB_REPLAY=$chat DBOTA_WEB_ARGS="--replay built-in --pov 0 --settings video" APIS=webgpu OUT=$pages/video web/build.sh
rm -f "$work/cache.zip"
target/release/dbota export-cache "$work/cache.zip"

python3 web/test/smoke.py --import-page "$pages/replay" --reference-cache "$DBOTA_ASSETS" \
    --prepared-page "$pages/replay" --cache-zip "$work/cache.zip" --cache-folder "$DBOTA_ASSETS" \
    --served-page "$pages/plain" --startup-page "$pages/plain" --keys-page "$pages/plain" --boot-page "$pages/plain" \
    --replay-page "$pages/replay" --cursor-page "$pages/replay" --pixels-page "$pages/replay" \
    --fullscreen-page "$pages/video" \
    --replays-page "$pages/plain" --server "$server" --radovid "$radovid_bin"
# The real mouse: a headful Chromium on an X server (Xwayland) in a private compositor, moved through XTEST.
DBOTA_HEADLESS_XWAYLAND=1 "$here/tools/headless.sh" python3 web/test/smoke.py --only-wire --cursor-input-page "$pages/replay"
