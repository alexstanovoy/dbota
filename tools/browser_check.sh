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
# shellcheck source=tools/client_env.sh
. "$here/tools/client_env.sh"
cd "$here"
bota=${DBOTA_GATE_BOTA:-$here/../bota}
radovid=${DBOTA_GATE_RADOVID:-$here/../radovid}
: "${DBOTA_ASSETS:?DBOTA_ASSETS: the asset cache of this build}"
work=${GATE_LOGS:-$here/temp/gate}/browser
mkdir -p "$work"
pwlib=${DBOTA_GATE_PWLIB:-${XDG_CACHE_HOME:-$HOME/.cache}/dbota-gate/pwlib}
export PYTHONPATH=$pwlib PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1
in_private_environment python3 -m unittest discover -s web/test -p test_optimize.py
if ! python3 -I -c 'import sys; sys.path.insert(0, sys.argv[1]); import playwright' "$pwlib" 2> /dev/null; then
    python3 -m pip install --quiet --target "$pwlib" playwright
fi
python3 -m playwright install chromium firefox > "$work/playwright-install.log" 2>&1 \
    || { tail -n 20 "$work/playwright-install.log"; echo "browser_check: the browsers cannot be installed" >&2; exit 1; }

# The servers: bota's (a match with a replay) and radovid's (the replays page).
server_target=$here/target/browser-servers/bota
radovid_target=$here/target/browser-servers/radovid
(cd -- "$bota" && cargo build --locked --release --quiet --manifest-path Cargo.toml -p bota_server --target-dir "$server_target")
(cd -- "$radovid" && cargo build --locked --release --quiet --manifest-path Cargo.toml -p radovid --target-dir "$radovid_target")
server=$server_target/release/bota_server
radovid_bin=$radovid_target/release/radovid

# A replay with chat lines (the replay page checks that they are heard): a fast match, one headless client that says six lines.
chat=$work/chat.brp
rm -f "$chat" "$work/server.out"
cargo build --locked --release --quiet
steering_unset_arguments
env "${STEERING_UNSET[@]}" "$server" --port 0 --bind 127.0.0.1 --mode realtime --tick-rate 120 --map 2 --players 2 --dummy-seat --pick-secs 1 \
    --tactical-secs 0 --seed 7 --empty-after-secs 4 --replay "$chat" > "$work/server.out" 2>&1 &
srv=$!
addr=
for _ in $(seq 100); do
    addr=$(sed -n 's/^bota_server listening on //p' "$work/server.out" | head -1)
    [ -n "$addr" ] && break
    sleep 0.3
done
[ -n "$addr" ] || { kill "$srv" 2> /dev/null; echo "browser_check: the server did not start"; cat "$work/server.out"; exit 1; }
"$here/tools/headless.sh" "$here/tools/test_client.sh" --addr "$addr" --config "$work/chat.conf" --mute --no-vsync --display-mode windowed \
    --script "await:game;wait:30;say:one;wait:1;say:two;wait:1;say:three;wait:1;say:four;wait:1;say:five;wait:1;say:six;wait:20;exit" \
    > "$work/chat-client.out" 2>&1 || { kill "$srv" 2> /dev/null; tail -n 20 "$work/chat-client.out"; echo "browser_check: the chat client failed"; exit 1; }
wait "$srv" || true
[ -s "$chat" ] || { echo "browser_check: no replay was recorded"; exit 1; }

# Reuse the identical plain clients built by the gate. Replays and startup arguments remain
# compile-time inputs, so their page variants are built with the original interface.
export PROFILE=${DBOTA_WEB_TEST_PROFILE:-release}
export WASM_OPT=${WASM_OPT:-0}
pages=$here/temp/gate-pages
rm -rf "$pages"
mkdir -p "$pages"
if [ -n "${DBOTA_GATE_WEB_PAGE:-}" ]; then
    mkdir "$pages/plain"
    # Owned build outputs require nlink=1; reflinks preserve that and work across filesystems.
    cp -a --reflink=auto -- "$DBOTA_GATE_WEB_PAGE/." "$pages/plain/"
else
    OUT=$pages/plain web/build.sh
fi
DBOTA_INDEX_PAGE="$pages/plain" in_private_environment python3 -m unittest discover -s web/test -p test_cache_index.py -v
DBOTA_INDEX_PAGE="$pages/plain" in_private_environment python3 -m unittest discover -s web/test -p test_voice_worker.py -v
DBOTA_WEB_REPLAY=$chat DBOTA_WEB_ARGS="--replay built-in --pov 0 --seek 4600" OUT=$pages/replay web/build.sh
DBOTA_WEB_REPLAY=$chat DBOTA_WEB_ARGS="--replay built-in --pov 0 --settings video" APIS=webgpu OUT=$pages/video web/build.sh
PROFILE=web DBOTA_WEB_REPLAY=$chat DBOTA_WEB_ARGS="--replay built-in --pov 0 --seek 4600" OUT=$pages/web web/build.sh
rm -f "$work/cache.zip"
"$here/tools/test_client.sh" export-cache "$work/cache.zip"

in_private_environment python3 web/test/smoke.py --import-page "$pages/replay" --reference-cache "$DBOTA_ASSETS" --start-page "$pages/plain" \
    --prepared-page "$pages/replay" --cache-zip "$work/cache.zip" --cache-folder "$DBOTA_ASSETS" \
    --served-page "$pages/plain" --startup-page "$pages/plain" --keys-page "$pages/plain" --boot-page "$pages/plain" --busy-page "$pages/plain" \
    --fonts-page "$pages/plain" \
    --replay-page "$pages/replay" --scene-page "$pages/web" --cursor-page "$pages/replay" --pixels-page "$pages/replay" \
    --fullscreen-page "$pages/video" \
    --replays-page "$pages/plain" --server "$server" --radovid "$radovid_bin"
# The real mouse: a headful Chromium on an X server (Xwayland) in a private compositor, moved through XTEST.
DBOTA_HEADLESS_XWAYLAND=1 "$here/tools/headless.sh" python3 web/test/smoke.py --only-wire --cursor-input-page "$pages/replay"
