#!/usr/bin/env python3
"""Checks of the page in a headless Chromium (Playwright), each on a build named by its flag. The first run
(the import of a Dota 2 install in the client, its screens and refusals) is web/test/first_run.py's.

The pages that need no import play on a native import of this build (`dbota import`, DBOTA_ASSETS names it)
through `?cache=http`, served under /cache beside the page:

    DBOTA_WEB_REPLAY=$PWD/chat.brp DBOTA_WEB_ARGS="--replay built-in --pov 0 --seek 4600" \
        PROFILE=release OUT=temp/web-replay web/build.sh
    PROFILE=release OUT=temp/web-live web/build.sh
    CARGO_TARGET_DIR=temp/bota-target cargo build --release --manifest-path ../bota/Cargo.toml \
        -p bota_server --features websocket
    PYTHONPATH=temp/pwlib python3 web/test/smoke.py --replay-page temp/web-replay \
        --replays-page temp/web-live --server temp/bota-target/release/bota_server --radovid RADOVID \
        --startup-page temp/web-live

* `--launch-page` (any build): the WebGL2 client starts and draws its first frames with no panic; a panic fails
  at once with its message.
* `--startup-page` (any build): the page loads the WebGPU client, or the WebGL2 one with
  `?gpu=webgl2`, and each draws; a renderer that cannot be made (the page then offers the WebGL2 client, which
  runs) and a client out of memory each end with a message on the page.
* `--replay-page`: the page plays a Zstandard-packed replay built into it (read through to the
  seek tick, as the packed file is decoded record by record) and the client says it knows the
  match and heard the chat lines, with no read error.
* `--scene-page`: a replay reaches the drawn forest and heroes on WebGPU and WebGL2, with a
  visible canvas and no page error. A client panic fails the check immediately.
* `--fonts-page` (any build): the page serves the interface fonts out of its own folder (a link out
  of it is not followed, as a service's static directory does not) and the client takes them.
* `--keys-page` (any page, even `web/` unbuilt): over the canvas the browser keeps its Ctrl and
  Cmd chords and F12, the game its keys (F5, Tab, Space, letters, right click, wheel); in fullscreen
  Chromium's canvas locks Esc (and lets it go on leaving), Firefox's page says once that it cannot.
* `--pixels-page` (a replay build): at a device scale of 1.5, and after the page is resized, the
  canvas's drawing buffer and the client's surface are the canvas's device-pixel size.
* `--cursor-page` (a replay build, e.g. the `--replay-page` one): in a match a press on the canvas
  locks the pointer, the game's cursor moves by the mouse's motion and stays inside the canvas, a
  press there is the game's at that place (the dashboard button opens the settings, which let the
  pointer go), and closing them or F11 after an Esc takes it again.
* `--cursor-input-page` (a replay build, both clients as web/build.sh makes them): the same with a real mouse,
  under `DBOTA_HEADLESS_XWAYLAND=1 tools/headless.sh`: a Chromium window on its X server at a device scale of
  1.5, moved and clicked through the X server's own input (XTEST), the way a mouse's events reach the browser.
  The game's cursor goes as far as the mouse moves, and a click lands where the cursor is drawn: the dashboard
  button opens the settings and the client reports the cursor at the place the page holds.
* `--boot-page` (any build): the page never spins for ever: a part that does not load, code the
  browser cannot run, a page that never arrives, a game download that stalls or is refused each end on a
  screen that says the stage, while a slow download that keeps moving is waited for and the game starts
  (Chromium and Firefox).
* `--busy-page` (any build): a server answering 503 with Retry-After (busy) is told on the page and
  tried once more after the wait: the page's own scripts (Chromium and Firefox), the game's download and the
  import worker's module; a download refused twice ends on the failure screen.
* `--fullscreen-page`: a replay build made with `DBOTA_WEB_ARGS="--replay built-in --pov 0 --settings video"`;
  the display mode and F11 put the canvas fullscreen and leaving it turns the setting back.
"""
import argparse
import base64
import ctypes
import ctypes.util
import functools
import http.server
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time

from playwright.sync_api import Error as PlaywrightError, TimeoutError as PlaywrightTimeout, sync_playwright

from render_errors import RENDERER_ERROR

HERE = pathlib.Path(__file__).resolve().parent
OUT = HERE.parent / "out"
# The asset cache the pages that need no import play on (`?cache=http`): a native import of this
# build, served under /cache beside the page. DBOTA_ASSETS must name it: the user's own cache is no default.
def cache_folder():
    given = os.environ.get("DBOTA_ASSETS")
    if not given:
        sys.exit("DBOTA_ASSETS must name a native import of this build for the pages that play on a served cache")
    return pathlib.Path(given)


# The environment of the services the checks start, which start game servers: without the variables that steer
# a client or a server from outside (tools/client_environment.txt), such as a seat list in the caller's shell.
def without_steering_variables():
    listed = (HERE.parent.parent / "tools/client_environment.txt").read_text().splitlines()
    steering = {line.strip() for line in listed if line.strip() and not line.startswith("#")}
    return {name: value for name, value in os.environ.items() if name not in steering}


class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def translate_path(self, path):
        if path.split("?")[0].startswith("/cache/"):
            relative = pathlib.PurePosixPath(path.split("?")[0][len("/cache/"):])
            if ".." in relative.parts:
                return "/nonexistent"
            return str(cache_folder().joinpath(*relative.parts))
        found = super().translate_path(path)
        # A file reached through a link out of the folder is not served, as a matchmaking service
        # serves its static directory (`web/README.md`): the page must hold what it needs itself.
        root = os.path.realpath(self.directory)
        if os.path.commonpath([os.path.realpath(found), root]) != root:
            return "/nonexistent"
        return found


def serve(directory=OUT, handler_class=Quiet):
    handler = functools.partial(handler_class, directory=str(directory))
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def browser_arguments():
    graphics = {
        "software": ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"],
        "hardware": ["--enable-gpu", "--use-angle=vulkan"],
    }
    selected = os.environ.get("DBOTA_BROWSER_GPU", "software")
    if selected not in graphics:
        sys.exit(f"DBOTA_BROWSER_GPU must be software or hardware, got {selected!r}")
    return [
        "--enable-unsafe-webgpu", *graphics[selected], "--enable-features=Vulkan",
        # Local test servers must not open a browser permission prompt.
        "--disable-features=LocalNetworkAccessChecks,BlockInsecurePrivateNetworkRequests",
        # The page plays its sounds to the audio graph and nobody: a check never reaches the machine's speakers.
        "--mute-audio",
    ]


BROWSER_ARGS = browser_arguments()
# Firefox's way of the same: no volume.
FIREFOX_PREFERENCES = {"media.volume_scale": "0.0"}


def launch_engine(p, engine):
    """The browser of an engine, started so that its page plays to no speakers."""
    if engine == "chromium":
        return p.chromium.launch(args=BROWSER_ARGS)
    return getattr(p, engine).launch(firefox_user_prefs=FIREFOX_PREFERENCES)
# The dashboard button's middle in a replay page of 1280x720 (logical pixels).
DASHBOARD = (15, 15)


def wait_console(page, console, pattern, timeout, what):
    """Waits for a console line matching `pattern`; returns its match."""
    seen = re.compile(pattern)
    deadline = time.time() + timeout
    while time.time() < deadline:
        for line in list(console):
            if m := seen.search(line):
                return m
        page.wait_for_timeout(500)
    print("console:", *[c[:300] for c in [c for c in console if "CommandQueue" not in c and not c.startswith("log.")][-25:]], sep="\n  ", file=sys.stderr)
    # The first lines say what went wrong first; the tail only says what followed.
    full = HERE.parent.parent / "temp/smoke-console.log"
    full.parent.mkdir(parents=True, exist_ok=True)
    full.write_text("\n".join(console), encoding="utf-8")
    print(f"the whole console is in {full}", file=sys.stderr)
    try:
        page.screenshot(path=str(HERE.parent.parent / "temp/smoke-timeout.png"), timeout=60000)
    except Exception as e:  # the page may be gone or too busy to draw
        print("no screenshot:", e, file=sys.stderr)
    sys.exit(f"timed out waiting for {what}")


def wait_loading_cover(page, console):
    """Wait until the loading cover no longer blocks the cursor tests' HUD buttons."""
    wait_console(page, console, r"loading: done in [\d.]+ s", 300, "the loading cover to be removed")
    fallback = any("loading: still waiting" in line for line in console)
    print("cursor: loading cover removed" + (" (fallback)" if fallback else ""))


def opened(p, directory, args=()):
    """A page of the build in `directory` in a fresh browser: (server, base, browser, page, console, problems)."""
    server = serve(directory)
    base = f"http://127.0.0.1:{server.server_address[1]}/"
    browser = p.chromium.launch(args=[*BROWSER_ARGS, *args])
    page = browser.new_context(viewport={"width": 1280, "height": 720}).new_page()
    console, problems = [], []
    page.on("console", lambda m: console.append(m.text))
    page.on("pageerror", lambda e: problems.append(f"page error: {e}"))
    return server, base, browser, page, console, problems


def check_replay_page(directory):
    """A packed (Zstandard) replay built into the page is read through and played."""
    with sync_playwright() as p:
        server, base, browser, page, console, problems = opened(p, directory)
        page.goto(base + "?cache=http")
        wait_console(page, console, r"match known: tick \d+", 600, "the client to know the match")
        # The lines arrive as the replay is read through to its seek tick.
        heard = wait_console(page, console, r"chat: ([5-9]|\d\d+) lines heard", 300, "the chat lines of the replay")
        page.wait_for_timeout(3000)
        page.screenshot(path=str(HERE.parent.parent / "temp/smoke-replay.png"), timeout=600000)
        assert any("stands behind the Latin ones" in c for c in console), "the CJK fallback is registered"
        bad = [c for c in console if "cannot be read" in c or "panicked" in c or "RuntimeError" in c]
        assert not bad, bad
        assert not problems, problems
        lines = max(int(m.group(1)) for c in console if (m := re.search(r"chat: (\d+) lines heard", c)))
        assert lines >= 5, f"the packed replay's chat lines: {heard.group(0)}"
        print(f"packed replay: match known, {lines} chat lines heard, no read error")
        browser.close()
    server.shutdown()


def check_scene_page(directory, apis=("webgpu", "webgl2"), *, terrain=False, shots=None, refused_shot=None):
    """Play a full-map replay and check the rendered scene on each selected graphics API."""
    with sync_playwright() as p:
        for api in apis:
            server, base, browser, page, console, problems = opened(p, directory)
            page.on("console", lambda message: problems.append(message.text) if message.type == "error" and RENDERER_ERROR.search(message.text) else None)
            try:
                if refused_shot:
                    file = "icons/abilities/nevermore_shadowraze1.png"
                    header = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR" + b"\0\0\x10\0" * 2 + bytes([8, 6, 0, 0, 0])
                    page.route(base + "cache/" + file, lambda route: route.fulfill(
                        status=200, content_type="image/png", body=header
                    ))
                if terrain:
                    page.set_viewport_size({"width": 1920, "height": 1080})
                    # These probes use absolute pointer positions. The cursor check exercises locking.
                    page.add_init_script("HTMLCanvasElement.prototype.requestPointerLock = () => Promise.resolve()")
                page.add_init_script("""globalThis.dbota_scene_drawn = false;
                  globalThis.dbota_scene_failure = null;
                  const log = console.log.bind(console);
                  console.log = (...args) => {
                    for (const message of args) {
                      if (typeof message !== 'string') continue;
                      if (message.includes('scene: the game drawn whole')) globalThis.dbota_scene_drawn = true;
                      if (message.includes('the replay cannot be read')) globalThis.dbota_scene_failure = message;
                    }
                    log(...args);
                  };""")
                query = "?cache=http" + ("&gpu=webgl2" if api == "webgl2" else "")
                page.goto(base + query)
                if terrain:
                    page.mouse.move(960, 540)
                page.wait_for_function(
                    "() => globalThis.dbota_scene_drawn || globalThis.dbota_scene_failure || document.querySelector('#failed:not([hidden])')",
                    polling=100, timeout=600000,
                )
                assert page.evaluate("globalThis.dbota_scene_drawn"), (
                    api, page.evaluate("globalThis.dbota_scene_failure"),
                    page.text_content("#failTitle"), page.text_content("#failDetail")
                )
                assert any(f"dbota: drawing with {api}" in c for c in console), f"the {api} client was selected"
                assert page.locator("#dbota").is_visible(), "the game's canvas is visible"
                assert page.locator("#dbota").evaluate("c => c.width > 0 && c.height > 0"), "the canvas has pixels"
                if terrain:
                    wait_loading_cover(page, console)
                    # The status label still appears on served replays and can cover the minimap.
                    page.locator("#booting").evaluate("label => label.style.pointerEvents = 'none'")
                    if refused_shot:
                        wait_console(page, console, r"WARN.*" + re.escape(file) + r".*per-image budget", 60,
                                     "the oversized ability icon warning")
                        move_terrain_camera(page, 2950, 3390)
                        page.screenshot(path=refused_shot, timeout=600000)
                    else:
                        terrain_pixels(page, shots, api)
                        refused = [
                            message for message in console if "image budget" in message and "exceeded" in message
                        ]
                        assert not refused, refused
                else:
                    page.wait_for_timeout(2000)
                assert not [line for line in console if "ERROR" in line and RENDERER_ERROR.search(line)], console
                assert not problems, problems
                print(f"scene: {api} draws the forest and heroes")
            finally:
                if shots:
                    (shots / f"{api}-console.log").write_text("\n".join(console + problems), encoding="utf-8")
                browser.close()
                server.shutdown()
                server.server_close()


def after_frames(page, count=3):
    page.wait_for_function("""count => new Promise(done => {
      function frame() { if (--count) requestAnimationFrame(frame); else done(true); }
      requestAnimationFrame(frame);
    })""", arg=count, timeout=60000)


def click_drawn(page, at):
    page.mouse.move(*at)
    after_frames(page)
    page.mouse.click(*at)
    after_frames(page)


def wait_browser_state(page, observe, what, timeout):
    """Observe state through the page's strict CSP, using time only as a hang guard."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if value := observe():
            return value
        after_frames(page)
    try:
        page.screenshot(path=str(HERE.parent.parent / "temp/smoke-timeout.png"), timeout=20000)
    except PlaywrightError:
        pass
    raise AssertionError(f"timed out waiting for {what}")


# At 1920x1080, the minimap is 308 interface pixels, scaled by 1080/1200.
# Clicking it puts the game camera directly on a point of the 18432-unit map.
# The last field is the allowed blue lead over red. The river bank's slate is blue-grey.
TERRAIN_PROBES = (
    ("radiant_base", 2950, 3390, .45, .42, 16),
    ("mid_river_bank", 8850, 9350, .63, .31, 64),
    ("bottom_lane", 12600, 2780, .45, .42, 16),
    ("dire_base", 14700, 14500, .46, .22, 16),
    ("jungle_camp", 7762, 5860, .45, .42, 16),
)


def move_terrain_camera(page, x, y):
    side = 308 * 0.9
    mouse_x, mouse_y = x * side / 18432, 1080 - y * side / 18432
    assert page.evaluate("([x, y]) => document.elementFromPoint(x, y)?.id === 'dbota'", [mouse_x, mouse_y])
    page.mouse.move(mouse_x, mouse_y)
    page.mouse.down()
    try:
        page.evaluate("""() => {
          globalThis.dbota_probe_frames = 0;
          function next() {
            if (++globalThis.dbota_probe_frames < 3) requestAnimationFrame(next);
          }
          requestAnimationFrame(next);
        }""")
        page.wait_for_function("() => globalThis.dbota_probe_frames >= 3", timeout=60000)
    finally:
        page.mouse.up()
    return mouse_x, mouse_y


def terrain_pixels(page, shots, api):
    """Sample the composited GPU frame, in patches of ground clear of the HUD."""
    results = {}
    for name, x, y, left, top, blue_lead in TERRAIN_PROBES:
        mouse_x, mouse_y = move_terrain_camera(page, x, y)
        path = shots / f"{api}-{name}.png" if shots else None
        data = page.screenshot(path=path, timeout=600000)
        covered, ratio, detail = page.evaluate("""async ({png, left, top, mouseX, mouseY, blueLead}) => {
          const bytes = Uint8Array.from(atob(png), character => character.charCodeAt(0));
          const bitmap = await createImageBitmap(new Blob([bytes], {type: "image/png"}));
          const width = bitmap.width, height = bitmap.height;
          const canvas = new OffscreenCanvas(width, height);
          const context = canvas.getContext('2d', {willReadFrequently: true});
          context.drawImage(bitmap, 0, 0);

          // The minimap draws a white viewport outline around the camera's focus.
          const marker = context.getImageData(mouseX - 8, mouseY - 36, 17, 72);
          const edges = [false, false];
          for (let y = 0; y < marker.height; y++) {
            let white = 0;
            for (let x = 0; x < marker.width; x++) {
              const i = (y * marker.width + x) * 4;
              if (Math.min(marker.data[i], marker.data[i + 1], marker.data[i + 2]) >= 210) white++;
            }
            if (white >= 15) edges[Number(y >= 36)] = true;
          }
          const covered = edges.every(Boolean);
          const patch = context.getImageData(width * left, height * top, width * .10, height * .16);
          let ground = 0;
          for (let i = 0; i < patch.data.length; i += 4) {
            const [r, g, b] = patch.data.subarray(i, i + 3);
            if (g > 22 && r > 18 && b < r + blueLead) ground++;
          }
          // Median tile microcontrast rejects a flat fallback and discounts props
          // and wide gradients. A second difference samples the texture grain.
          function luma(x, y) {
            const i = (y * patch.width + x) * 4;
            return patch.data[i] + 2 * patch.data[i + 1] + patch.data[i + 2];
          }
          const tiles = [];
          for (let y = 0; y < patch.height - 16; y += 16) {
            for (let x = 0; x < patch.width - 16; x += 16) {
              let detail = 0;
              for (let row = y; row < y + 16; row++) {
                for (let column = x + 2; column < x + 14; column++) {
                  detail += Math.abs(2 * luma(column, row) - luma(column - 2, row) - luma(column + 2, row));
                }
              }
              tiles.push(detail);
            }
          }
          tiles.sort((a, b) => a - b);
          return [covered, ground / (patch.width * patch.height), tiles[Math.floor(tiles.length / 2)] / (16 * 12 * 4)];
        }""", {"png": base64.b64encode(data).decode("ascii"), "left": left, "top": top,
                "mouseX": mouse_x, "mouseY": mouse_y, "blueLead": blue_lead})
        assert covered, f"{api}: the minimap viewport did not reach {name}"
        results[name] = (round(ratio, 3), round(detail, 3))
    assert all(ratio >= .70 and detail > 1.25 for ratio, detail in results.values()), (
        api, "expected textured stone, earth or grass at each landmark (coverage, detail)", results
    )
    print(f"terrain: {api} coverage and detail at all landmarks: {results}")


def check_fullscreen_page(directory, shots=False):
    """Windowed fullscreen in Settings puts the canvas fullscreen (inside the selection gesture), the
    page leaving it (Esc) turns the setting back, and F11 is the same fullscreen. Headless Chromium
    keeps its viewport, so the canvas filling a bigger screen is not seen here."""
    with sync_playwright() as p:
        server, base, browser, page, console, problems = opened(p, directory)
        page.goto(base + "?cache=http")
        wait_console(page, console, r"match known: tick \d+", 600, "the client to know the match")
        page.locator("#dbota").wait_for(state="visible", timeout=60000)

        def shot(name):
            if shots:
                page.screenshot(path=str(HERE.parent.parent / f"temp/smoke-{name}.png"), timeout=600000)

        def fullscreen_becomes(entered, what):
            # The state itself, polled: how long a loaded machine takes to draw and handle a click
            # decides nothing, only the 60 s hang guard.
            try:
                page.wait_for_function(
                    "entered => !!document.fullscreenElement === entered", arg=entered, polling=100, timeout=60000,
                )
            except PlaywrightTimeout as error:
                raise AssertionError(what) from error

        # The settings search stays in the sidebar while the video section moves with font and
        # layout changes. Searching leaves one visible control, so the first Tab focuses it.
        click_drawn(page, (302, 75))
        page.keyboard.type("Display mode")
        after_frames(page)
        for key in ("Enter", "Tab", "Enter", "ArrowDown", "Enter"):
            page.keyboard.press(key)
            after_frames(page)
        fullscreen_becomes(True, "picking windowed fullscreen puts the canvas fullscreen")
        shot("fullscreen-picked")
        page.evaluate("document.exitFullscreen()")
        fullscreen_becomes(False, "the page leaves fullscreen")
        after_frames(page)
        shot("fullscreen-left")  # the setting shows Windowed again
        page.keyboard.press("F11")
        fullscreen_becomes(True, "F11 is the canvas's fullscreen")
        page.keyboard.press("F11")
        fullscreen_becomes(False, "F11 leaves fullscreen")
        assert not problems, problems
        print("fullscreen: the setting and F11 enter it, leaving it works")
        browser.close()
    server.shutdown()


ESC_LOCK_SPY = """(() => {
  // Records what the page asks of the Keyboard Lock API and passes it on to the real one.
  globalThis.keyboardLock = [];
  const keyboard = navigator.keyboard;
  if (!keyboard) return;
  for (const name of ['lock', 'unlock']) {
    const real = keyboard[name].bind(keyboard);
    keyboard[name] = (...args) => { keyboardLock.push([name, ...args]); return real(...args); };
  }
  globalThis.escapes = [];
  addEventListener('keydown', (e) => { if (e.key === 'Escape') escapes.push(document.fullscreenElement !== null); }, true);
})()"""


def check_escape_in_fullscreen(directory):
    """Esc is a game key, which a browser takes for leaving fullscreen. Where the Keyboard Lock API
    exists (Chromium) the canvas locks Esc when it goes fullscreen and lets it go when it leaves
    (web/keys.js); where it does not (Firefox) the page says so once and Esc is left to the
    browser. Needs no client, so any page serves. A press of Esc injected by the test never goes
    through the browser's own handling of the key (which leaves fullscreen, or with the lock asks
    for it to be held), so what the player's short press does cannot be seen here: only what the
    page asks for, and that the page sees the key."""
    with sync_playwright() as p:
        server = serve(directory)
        base = f"http://127.0.0.1:{server.server_address[1]}/"
        for engine in ("chromium", "firefox"):
            browser = launch_engine(p, engine)
            page = browser.new_context(viewport={"width": 1280, "height": 720}).new_page()
            console = []
            page.on("console", lambda m: console.append(m.text))
            page.add_init_script(ESC_LOCK_SPY)
            # The load event comes after the page's modules (main.js, which imports keys.js) have run,
            # so F11 below meets its handler; goto's own timeout bounds the wait.
            page.goto(base + "?cache=http", wait_until="load")
            page.wait_for_function("() => document.getElementById('dbota') !== null")
            fullscreen = lambda: page.evaluate("document.fullscreenElement === document.getElementById('dbota')")
            page.keyboard.press("F11")
            page.wait_for_function("() => document.fullscreenElement !== null", timeout=60000)
            if engine == "chromium":
                wait_console(page, console, "Esc is locked to the game while fullscreen", 60, "the browser to grant the Esc lock")
                calls = page.evaluate("keyboardLock")
                assert calls == [["lock", ["Escape"]]], f"the page locks Esc, and only Esc: {calls}"
                page.keyboard.press("Escape")
                assert page.evaluate("escapes") == [True], "the page sees a short Esc in fullscreen"
                # Only a precondition: an injected Esc never reaches the browser's own handling, so this
                # holds with or without the lock. The assertions of this check are the lock's grant above
                # and its release below.
                assert fullscreen(), "the canvas is still fullscreen after the injected Esc"
                page.keyboard.press("F11")
                page.wait_for_function("() => document.fullscreenElement === null", timeout=60000)
                page.wait_for_function("() => keyboardLock.length === 2", timeout=60000)
                calls = page.evaluate("keyboardLock")
                assert calls[1][0] == "unlock", f"the page lets Esc go on leaving fullscreen: {calls}"
                print("escape: Chromium locks Esc in fullscreen and lets it go on leaving")
            else:
                wait_console(page, console, "this browser cannot keep Esc in the game", 60, "the page to say Esc cannot be kept")
                page.keyboard.press("F11")
                page.wait_for_function("() => document.fullscreenElement === null", timeout=60000)
                said = [c for c in console if "cannot keep Esc" in c]
                assert len(said) == 1, f"the page says it once: {said}"
                print("escape: Firefox has no Esc lock, the page says so once")
            browser.close()
    server.shutdown()


KEYS_LOGIC = """() => {
  const canvas = document.getElementById('dbota');
  const kept = (type, init) => {
    const e = type === 'wheel'
      ? new WheelEvent(type, { ...init, cancelable: true, bubbles: true })
      : type === 'contextmenu'
        ? new MouseEvent(type, { ...init, cancelable: true, bubbles: true })
        : new KeyboardEvent(type, { ...init, cancelable: true, bubbles: true });
    canvas.dispatchEvent(e);
    return !e.defaultPrevented;
  };
  return {
    browser: [
      ['Ctrl+R', kept('keydown', { key: 'r', code: 'KeyR', ctrlKey: true })],
      ['Ctrl+T', kept('keydown', { key: 't', code: 'KeyT', ctrlKey: true })],
      ['Ctrl+W', kept('keydown', { key: 'w', code: 'KeyW', ctrlKey: true })],
      ['Ctrl+Tab', kept('keydown', { key: 'Tab', code: 'Tab', ctrlKey: true })],
      ['Cmd+F', kept('keydown', { key: 'f', code: 'KeyF', metaKey: true })],
      ['F12', kept('keydown', { key: 'F12', code: 'F12' })],
    ],
    game: [
      ['F5', kept('keydown', { key: 'F5', code: 'F5' })],
      ['Tab', kept('keydown', { key: 'Tab', code: 'Tab' })],
      ['Space', kept('keydown', { key: ' ', code: 'Space' })],
      ['Q', kept('keydown', { key: 'q', code: 'KeyQ' })],
      ['Q up', kept('keyup', { key: 'q', code: 'KeyQ' })],
      ['right click', kept('contextmenu', { button: 2 })],
      ['wheel', kept('wheel', { deltaY: 100 })],
    ],
  };
}"""


def check_browser_keys(directory):
    """The page's keys (web/keys.js): over the canvas the browser keeps every Ctrl or Cmd chord
    and F12 (reload, tabs, find, tools), and the game keeps its own (F5 is Dota's quick buy, Tab
    and Space are bound, a right click moves, the wheel zooms). Events are made on the canvas
    and whether the page let them through to the browser is read; needs no client, so any page
    (even web/ as it is in the repository) serves."""
    with sync_playwright() as p:
        server, base, browser, page, console, problems = opened(p, directory)
        page.goto(base + "?cache=http")
        page.wait_for_function("document.getElementById('dbota') !== null")
        page.wait_for_timeout(1000)
        got = page.evaluate(KEYS_LOGIC)
        taken = [name for name, kept in got["browser"] if not kept]
        assert not taken, f"the browser's keys taken by the page: {taken}"
        let_through = [name for name, kept in got["game"] if kept]
        assert not let_through, f"the game's keys let through to the browser: {let_through}"
        print("browser keys: the browser keeps its chords and F12, the game its keys")
        browser.close()
    server.shutdown()


DEVICE_BOX = """() => new Promise((done) => { const c = document.getElementById('dbota');
  new ResizeObserver((es) => { const d = es[0].devicePixelContentBoxSize[0];
    done({box: [d.inlineSize, d.blockSize], backing: [c.width, c.height]}); }).observe(c, {box: 'device-pixel-content-box'}); })"""


def check_canvas_pixels(directory):
    """The canvas is drawn at the screen's own pixels, at a fractional scale too and after the page
    is resized: its drawing buffer and the client's surface are the canvas's device-pixel size, so
    the browser shows the picture 1:1 (as sharp as the desktop client) wherever it shows the page
    1:1."""
    with sync_playwright() as p:
        server = serve(directory)
        base = f"http://127.0.0.1:{server.server_address[1]}/"
        browser = p.chromium.launch(args=BROWSER_ARGS)
        page = browser.new_context(viewport={"width": 1280, "height": 720}, device_scale_factor=1.5).new_page()
        console = []
        page.on("console", lambda m: console.append(m.text))
        page.goto(base + "?cache=http")
        wait_console(page, console, r"match known: tick \d+", 600, "the client to know the match")
        for width, height in ((1280, 720), (1001, 667)):
            page.set_viewport_size({"width": width, "height": height})
            page.wait_for_timeout(4000)
            got = page.evaluate(DEVICE_BOX)
            assert got["backing"] == got["box"], f"the canvas draws at its device pixels: {got}"
            sizes = [m.groups() for c in console if (m := re.search(r"window: scale_factor=([\d.]+) .* physical=(\d+)x(\d+)", c))]
            assert sizes and sizes[-1] == ("1.5", str(got["box"][0]), str(got["box"][1])), f"the client's surface: {sizes[-1:]}, the canvas: {got}"
        print("canvas: drawn at its device pixels at a scale of 1.5, also after a resize")
        browser.close()
    server.shutdown()


# Marks the test's own pointer events, which stand for the mouse's (a locked pointer's motion cannot
# be injected): the page's re-dispatched events are the ones without it.
OWN_EVENT = 7
HANDED = """window.handed = [];
for (const type of ['pointermove', 'pointerdown', 'pointerup'])
    addEventListener(type, (e) => { if (!e.isTrusted && e.detail !== %d) handed.push([e.clientX, e.clientY]); }, true);""" % OWN_EVENT


def check_cursor_page(directory):
    """The game keeps the cursor in a match as the desktop confines it: a press on the canvas locks
    the pointer, the page moves the game's cursor by the mouse's motion held inside the canvas, a
    press there is the game's at that place (the dashboard button opens the settings, which let the
    pointer go), closing them takes it again, and so does F11 after an Esc."""
    with sync_playwright() as p:
        server, base, browser, page, console, problems = opened(p, directory)
        page.add_init_script(HANDED)
        page.goto(base + "?cache=http")
        wait_console(page, console, r"match known: tick \d+", 600, "the client to know the match")
        wait_loading_cover(page, console)
        page.wait_for_timeout(4000)
        locked = lambda: page.evaluate("document.pointerLockElement === document.getElementById('dbota')")
        size = page.evaluate("[innerWidth, innerHeight, devicePixelRatio]")

        def mouse(kind, dx=0, dy=0, button=-1, buttons=0):
            page.evaluate(
                f"""document.getElementById('dbota').dispatchEvent(new PointerEvent('{kind}', {{bubbles: true,
                detail: {OWN_EVENT}, movementX: {dx}, movementY: {dy}, button: {button}, buttons: {buttons},
                pointerId: 1, pointerType: 'mouse', isPrimary: true}}))"""
            )
            page.wait_for_timeout(100)

        def wait_lock(want, what):
            deadline = time.time() + 60
            while locked() != want and time.time() < deadline:
                page.wait_for_timeout(500)
            assert locked() == want, what

        page.mouse.click(size[0] // 2, size[1] // 2)
        wait_lock(True, "a press on the canvas in a match locks the pointer")
        wait_console(page, console, "cursor locked to the canvas", 60, "the client to see the lock")
        far = 10 * max(size[0], size[1])
        mouse("pointermove", far, far)
        last = [size[0] - 1 / size[2], size[1] - 1 / size[2]]
        got = page.evaluate("handed[handed.length - 1]")
        assert got == last, f"the cursor stays inside the canvas: {got}, not {last}"
        mouse("pointermove", -far, -far)
        assert page.evaluate("handed[handed.length - 1]") == [0, 0], "the cursor stops at the top-left corner"
        print("cursor: locked by a press, held inside the canvas")
        # The dashboard button, the top-left corner's first: in a replay page it opens the settings.
        mouse("pointermove", *DASHBOARD)
        # Only a move is the cursor's motion: a press and a release with a movement on them stay where the
        # cursor is.
        mouse("pointerdown", -far, -far, button=0, buttons=1)
        mouse("pointerup", -far, -far, button=0)
        at = page.evaluate("handed.slice(-2)")
        assert at == [list(DASHBOARD)] * 2, f"a press and a release are at the cursor, not moved by their travel: {at}"
        released = wait_console(page, console, r"cursor released at (\d+),(\d+)", 120, "the settings to let the pointer go")
        assert [int(released.group(1)), int(released.group(2))] == list(DASHBOARD), released.group(0)
        wait_lock(False, "the settings let the pointer go")
        print("cursor: a press at the game's cursor opens the settings, which let it go")
        page.keyboard.press("Escape")
        wait_lock(True, "closing the settings takes the pointer again")
        page.evaluate("document.exitPointerLock()")  # what the player's Esc does
        wait_lock(False, "the browser lets go")
        # The next press on the canvas takes the cursor again, and the game's cursor is held in the
        # canvas whatever travel the pointer reports after it.
        page.mouse.click(size[0] // 2, size[1] // 2)
        wait_lock(True, "a press after the lock was lost takes it again")
        # Whatever else the real click brings (a busy page delivers its events late), every place the
        # page hands on is inside the canvas, however far the pointer reports it travelled.
        before = page.evaluate("handed.length")
        for dx, dy in ((-far, far), (far, -far), (-far, -far)):
            mouse("pointermove", dx, dy)
        page.wait_for_timeout(2000)
        placed = page.evaluate(f"handed.slice({before})")
        assert len(placed) >= 3, placed
        outside = [at for at in placed if not (0 <= at[0] <= last[0] and 0 <= at[1] <= last[1])]
        assert not outside, f"held in the canvas after the re-lock: {outside}"
        page.evaluate("document.exitPointerLock()")
        wait_lock(False, "the browser lets go again")
        page.keyboard.press("F11")
        wait_lock(True, "F11 takes the pointer again")
        page.wait_for_function("() => !!document.fullscreenElement", timeout=60000)
        assert not problems, problems
        print("cursor: the settings closed and F11 take the pointer again")
        browser.close()
    server.shutdown()


class XMouse:
    """The mouse of the X server in DISPLAY, moved and pressed through XTEST: the events reach the browser as a
    real mouse's do (through the server and the window system), which no page-made event or DevTools input does
    while the pointer is locked."""

    def __init__(self):
        self.x11 = ctypes.CDLL(ctypes.util.find_library("X11") or "libX11.so.6")
        self.xtst = ctypes.CDLL(ctypes.util.find_library("Xtst") or "libXtst.so.6")
        self.x11.XOpenDisplay.restype = ctypes.c_void_p
        self.x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
        self.x11.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
        self.x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
        self.xtst.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
        self.xtst.XTestFakeRelativeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
        self.xtst.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
        self.display = self.x11.XOpenDisplay(os.environ["DISPLAY"].encode())
        assert self.display, f"no X server at DISPLAY={os.environ['DISPLAY']}"

    def to(self, x, y):
        self.xtst.XTestFakeMotionEvent(self.display, -1, x, y, 0)
        self.x11.XSync(self.display, 0)

    def by(self, dx, dy):
        self.xtst.XTestFakeRelativeMotionEvent(self.display, dx, dy, 0)
        self.x11.XSync(self.display, 0)

    def click(self, button=1):
        for down in (1, 0):
            self.xtst.XTestFakeButtonEvent(self.display, button, down, 0)
            self.x11.XSync(self.display, 0)
            time.sleep(0.1)

    def close(self):
        self.x11.XCloseDisplay(self.display)


# Every place the page hands the client while the pointer is locked: [type, clientX, clientY].
HANDED_ALL = """window.handed = [];
for (const type of ['pointermove', 'pointerdown', 'pointerup'])
    addEventListener(type, (e) => { if (!e.isTrusted) handed.push([type, e.clientX, e.clientY]); }, true);"""


def check_cursor_input(directory):
    """A real mouse in a match, at a device scale of 1.5: the game's cursor moves as far as the mouse does, and a
    click lands where the cursor is drawn (the dashboard button opens the settings, and the client reports the
    cursor at the page's place, not moved by the press)."""
    if not os.environ.get("DISPLAY"):
        sys.exit("--cursor-input-page needs an X server: run it under DBOTA_HEADLESS_XWAYLAND=1 tools/headless.sh")
    scale = 1.5
    mouse = XMouse()
    with sync_playwright() as p:
        server = serve(directory)
        base = f"http://127.0.0.1:{server.server_address[1]}/"
        # A window of the X server (not Wayland, where nothing could move the mouse), at the scale of a desktop
        # set to 150 %: the browser's own device scale, not an emulated one.
        browser = p.chromium.launch(
            headless=False,
            args=[*BROWSER_ARGS, "--ozone-platform=x11", f"--force-device-scale-factor={scale}",
                  "--window-position=0,0", "--window-size=1000,560"],
            env={k: v for k, v in os.environ.items() if k != "WAYLAND_DISPLAY"},
        )
        page = browser.new_context(no_viewport=True).new_page()
        console, problems = [], []
        page.on("console", lambda m: console.append(m.text))
        page.on("pageerror", lambda e: problems.append(f"page error: {e}"))
        page.add_init_script(HANDED_ALL)
        page.add_init_script("""addEventListener('pointermove', e => {
          if (e.isTrusted) window.pointerOnCanvas = e.target.id === 'dbota';
        }, true)""")
        page.goto(base + "?cache=http")
        wait_console(page, console, r"match known: tick \d+", 600, "the client to know the match")
        wait_loading_cover(page, console)
        assert page.evaluate("devicePixelRatio") == scale, page.evaluate("devicePixelRatio")
        last = lambda: page.evaluate("handed.length ? handed[handed.length - 1] : null")

        # A click in the window takes the pointer.
        mouse.to(400, 300)
        page.wait_for_function("() => window.pointerOnCanvas", polling=100, timeout=60000)
        mouse.click()
        page.wait_for_function(
            "() => document.pointerLockElement === document.getElementById('dbota')", polling=100, timeout=60000,
        )
        wait_console(page, console, "cursor locked to the canvas", 60, "the client to see the lock")
        print("cursor input: a real click locks the pointer")

        # The game's cursor goes as far as the mouse: 30 device pixels right and 15 down are 20 and 10 of the
        # page's (CSS) pixels at a scale of 1.5.
        # A single device pixel can round away at this scale. Wait for a genuine handed move
        # before taking the starting position; pointer lock alone does not acknowledge input.
        def move_mouse(x, y):
            before = page.evaluate("handed.length")
            mouse.by(x, y)
            page.wait_for_function(
                "before => handed.slice(before).some(([type]) => type === 'pointermove')",
                arg=before, polling=100, timeout=60000,
            )

        move_mouse(3, 3)
        start = last()
        for _ in range(3):
            move_mouse(10, 5)
        # Xwayland and the browser can coalesce several moves; await their delivery.
        page.wait_for_function("""([x, y]) => {
          const at = handed.at(-1);
          return at && at[1] - x >= 19 && at[2] - y >= 9;
        }""", arg=start[1:], polling=100, timeout=60000)
        end = last()
        moved = [end[1] - start[1], end[2] - start[2]]
        failures = []
        if not (abs(moved[0] - 20) <= 1 and abs(moved[1] - 10) <= 1):
            failures.append(f"the mouse's 30x15 device pixels move the cursor 20x10 CSS pixels, not {moved}")

        # Into the top-left corner, then onto the dashboard button (the interface scales with the page's
        # height, at least to 0.6 of a 720 pixel high one's), a whole number of device pixels away.
        middle = DASHBOARD[0] * max(page.evaluate("innerHeight") / 720, 0.6)
        steps = round(middle * scale)
        target = [steps / scale] * 2
        for _ in range(60):
            if last()[1:] == [0, 0]:
                break
            move_mouse(-60, -60)
        assert last()[1:] == [0, 0], f"the cursor stops at the top-left corner: {last()}"
        before_move = page.evaluate("handed.length")
        mouse.by(steps, steps)
        # The move reaches the page through Xwayland and the browser, when it does: as late as the machine is busy
        # (a 500 ms sleep read the corner on a loaded run, and the press that followed was already at the right place).
        # So the state is awaited, not a span: the wait ends the moment the cursor is within a pixel of where it
        # must end, and its 60 s only decides that it never came.
        try:
            page.wait_for_function("""([before, x, y]) => {
              const at = handed.at(-1);
              return handed.length > before && at[0] === 'pointermove'
                && Math.abs(at[1] - x) <= 1 && Math.abs(at[2] - y) <= 1;
            }""", arg=[before_move, *target], polling=100, timeout=60000)
        except PlaywrightTimeout as error:
            raise AssertionError(
                f"the cursor never reached {target} CSS pixels within 60 s of the mouse's {steps}x{steps} device "
                f"pixels from the corner; the last place handed to the client is {last()}") from error
        at = last()[1:]
        # A real click there: the press and the release are handed at the cursor, the client's cursor is that
        # place (it reports it when the settings the button opens let the pointer go), and the button opens.
        before = page.evaluate("handed.length")
        mouse.click()
        released, deadline = None, time.time() + 120
        while not released and time.time() < deadline:
            page.wait_for_timeout(500)
            released = next((m for c in console if (m := re.search(r"cursor released at (\d+),(\d+)", c))), None)
        presses = [h for h in page.evaluate(f"handed.slice({before})") if h[0] != "pointermove"]
        if not (presses and all(h[1:] == at for h in presses)):
            failures.append(f"the press and the release are at the cursor {at}: {presses}")
        got = released and [int(released.group(1)), int(released.group(2))]
        if not released:
            failures.append(f"a click with the cursor drawn on the dashboard button ({at}) opens the settings: it did not")
        elif got != [round(v) for v in at]:
            failures.append(f"the client's cursor at the click is where the cursor is drawn, {at}, not {got}")
        failures += problems
        assert not failures, "\n".join(failures)
        print(f"cursor input: the cursor moves as far as the mouse ({moved}); a real click lands where it is drawn ({got})")
        browser.close()
        server.shutdown()
    mouse.close()


def free_port():
    import socket

    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


NO_GRAPHICS = """
  delete Navigator.prototype.gpu;
  const get = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function (type, options) {
    return type === "webgl2" ? null : get.call(this, type, options);
  };"""
# A WebGPU adapter but no WebGPU canvas: the client's renderer cannot be made.
NO_WEBGPU_CANVAS = """
  const get = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function (type, options) {
    return type === "webgpu" ? null : get.call(this, type, options);
  };"""


FONTS = ("bota-sans-bold.ttf", "bota-sans-regular.ttf", "bota-sans-cjk.otf")


def check_fonts_page(directory):
    """The page holds the fonts every word of the interface is drawn in, and the client takes them:
    a page whose fonts do not arrive draws buttons, tabs and labels with no text at all."""
    with sync_playwright() as p:
        server, base, browser, page, console, problems = opened(p, directory)
        page.goto(base + "?cache=http")
        for name in FONTS:
            got = page.evaluate(
                "async (url) => { const r = await fetch(url); return [r.status, (await r.arrayBuffer()).byteLength]; }",
                f"assets/fonts/{name}",
            )
            assert got[0] == 200 and got[1] > 10000, f"the page serves no {name}: {got}"
        wait_console(page, console, "interface fonts: .* stands behind the Latin ones", 600, "the client to take its fonts")
        assert not problems, problems
        print("fonts: the page serves the interface fonts and the client has them")
        browser.close()
    server.shutdown()


def assert_launch_labels_hidden(page):
    assert page.is_hidden("#booting"), "the page's loading label covers the game canvas"
    assert page.is_hidden("#starting"), "the startup label covers the game canvas"
    assert page.is_hidden("#gate"), "the asset gate covers the game canvas"


def check_startup_page(directory):
    """The page picks the client the browser can draw with, and a start that fails says so on the
    page: no graphics (the page's screen of what the browser lacks), a renderer that cannot be made
    (offering the WebGL2 client), no memory."""
    def run(query, init=None, args=()):
        with sync_playwright() as p:
            server, base, browser, page, console, problems = opened(p, directory, args)
            if init:
                page.add_init_script(init)
            page.goto(base + query)
            yield page, console, problems
            browser.close()
        server.shutdown()

    def failed(page, title):
        page.wait_for_selector("#failed:not([hidden])", timeout=600000)
        got = page.inner_text("#failTitle")
        assert title in got, got

    for query, api in (("?cache=http", "webgpu"), ("?cache=http&gpu=webgl2", "webgl2")):
        for page, console, problems in run(query):
            wait_console(page, console, "dbota: drawing with " + api, 60, f"the {api} client")
            wait_console(page, console, "dbota: running", 600, f"the {api} client's first frame")
            assert_launch_labels_hidden(page)
            assert not problems, problems
            print(f"{api}: the client runs")
    for page, console, problems in run("?cache=http", NO_GRAPHICS):
        page.wait_for_selector("#unsupported", state="visible", timeout=600000)
        assert "WebGPU or WebGL 2" in page.inner_text("#lacks"), page.inner_text("#lacks")
        assert not any("drawing with" in c for c in console), "no client is loaded without graphics"
        print("no graphics: said on the page")
    for page, console, problems in run("?cache=http", NO_WEBGPU_CANVAS):
        failed(page, "could not start its graphics")
        assert "wgpu surface" in page.text_content("#failDetail"), "the panic is in the details"
        page.click("#otherApi")
        wait_console(page, console, "dbota: drawing with webgl2", 60, "the WebGL2 client offered instead")
        wait_console(page, console, "dbota: running", 600, "the WebGL2 client's first frame")
        assert "gpu=webgl2" in page.url, page.url
        print("renderer not made: said on the page, the WebGL2 client runs instead")
    # The client's memory capped at 96 MiB: the menu needs more.
    for page, console, problems in run("?cache=http", args=["--js-flags=--wasm-max-mem-pages=1536"]):
        failed(page, "ran out of memory")
        print("out of memory: said on the page")


def check_launch_page(directory):
    """The client starts and draws its first frames on the browser's own clock and storage: a panic on
    the way (a wall-clock read the browser's wasm does not have, say) fails at once with its message,
    not after the long wait for a first frame that never comes."""
    with sync_playwright() as p:
        server, base, browser, page, console, problems = opened(p, directory)
        page.goto(base + "?cache=http&gpu=webgl2")
        deadline = time.time() + 600
        while time.time() < deadline and not any("dbota: running" in c for c in console):
            panics = [c for c in console if "panicked at" in c]
            assert not panics and not problems, (panics, problems)
            page.wait_for_timeout(250)
        assert any("dbota: running" in c for c in console), "the client never drew a first frame"
        page.wait_for_timeout(3000)
        panics = [c for c in console if "panicked at" in c]
        assert not panics and not problems, (panics, problems)
        print("launch: the client draws its first frames, no panic")
        browser.close()
    server.shutdown()


def check_sound_page(directory):
    """The game's sound is not lost to the autoplay policy: a client started without a user gesture
    (a context the browser leaves suspended; Safari does so even after a gesture that ended before
    the context was made) is resumed by the player's first click or key, and the page says the
    state of its audio contexts (`dbota_audio_state()`) for a player who hears nothing."""
    spy = """
    window.__contexts = [];
    for (const name of ['AudioContext', 'webkitAudioContext']) {
      const Base = window[name];
      if (!Base) continue;
      window[name] = class extends Base { constructor(...a) { super(...a); window.__contexts.push(this); } };
    }
    """
    with sync_playwright() as p:
        server, base, browser, page, console, problems = opened(p, directory)
        page.add_init_script(spy)
        page.goto(base + "?cache=http&gpu=webgl2")
        wait_console(page, console, "dbota: running", 600, "the client's first frame")
        states = lambda: page.evaluate("window.__contexts.map(c => c.state)")
        assert states() == ["suspended"], f"no gesture yet: the context waits, got {states()}"
        page.mouse.click(640, 360)
        deadline = time.time() + 20
        while time.time() < deadline and states() != ["running"]:
            page.wait_for_timeout(250)
        assert states() == ["running"], f"the first click must start the sound, got {states()}"
        said = page.evaluate("globalThis.dbota_audio_state()")
        assert said == "running", f"the console one-liner says the state, got {said!r}"
        assert not problems, problems
        print("sound: a context left suspended is resumed by the first click")
        browser.close()
    server.shutdown()


# Interior samples at the smoke viewport's default HUD scale and unpositioned cursor.
# The two left samples are from the D and F ability slots. The strip sample avoids its label.
HUD_ART_REGIONS = (
    ("Presence of the Dark Lord", (650, 635, 22, 25)),
    ("Feast of Souls", (682, 635, 23, 25)),
    ("Magic Stick active strip", (130, 175, 60, 11)),
)


def check_ui_art_page(directory):
    """Both hero icons and the open item tooltip colour the rendered match canvas."""
    with sync_playwright() as p:
        server, base, browser, page, console, problems = opened(p, directory)
        try:
            page.goto(base + "?cache=http&gpu=webgl2")
            wait_loading_cover(page, console)
            assert any("dbota: drawing with webgl2" in line for line in console), console
            screenshot = page.locator("#dbota").screenshot(
                path=str(HERE.parent.parent / "temp/smoke-ui-art.png"), timeout=60000,
            )
            samples = page.evaluate("""async ({png, regions}) => {
                const frame = new Image();
                frame.src = 'data:image/png;base64,' + png;
                await frame.decode();
                const canvas = document.createElement('canvas');
                canvas.width = frame.width;
                canvas.height = frame.height;
                const ctx = canvas.getContext('2d');
                ctx.drawImage(frame, 0, 0);
                return regions.map(([x, y, width, height], region) => {
                    const rgba = ctx.getImageData(x, y, width, height).data;
                    let coloured = 0;
                    for (let at = 0; at < rgba.length; at += 4) {
                        const [r, g, b, a] = rgba.subarray(at, at + 4);
                        const red = r >= 25 && r >= Math.max(g, b) + 12;
                        const blue = b >= 58 && b >= r + 24 && b >= g + 20;
                        if (a === 255 && (region === regions.length - 1 ? blue : red)) coloured++;
                    }
                    return [coloured, width * height];
                });
            }""", {"png": base64.b64encode(screenshot).decode(), "regions": [rect for _, rect in HUD_ART_REGIONS]})
            for (name, _), (coloured, total) in zip(HUD_ART_REGIONS, samples, strict=True):
                assert coloured * 25 >= total, f"{name} art colour in {coloured}/{total} pixels"
            assert not problems, problems
            print(f"match HUD: coloured pixels in both icons and the active tooltip strip {samples}")
        finally:
            browser.close()
            server.shutdown()


def check_speech_page(directory):
    """A queued voice leaves the drawn mouth still until the audio context can run."""
    with sync_playwright() as p:
        server, base, browser, page, console, problems = opened(
            p, directory, args=("--autoplay-policy=document-user-activation-required",),
        )
        try:
            # Runtime evaluations and screenshots must not grant implicit activation.
            session = page.context.new_cdp_session(page)

            def audio_state():
                return session.send("Runtime.evaluate", {
                    "expression": "dbota_audio_state()", "returnByValue": True, "userGesture": False,
                })["result"].get("value")

            page.goto(base + "?cache=http&gpu=webgl2")
            wait_console(page, console, r"speech-check: queued", 180, "a resident voice to queue")
            rectangle = wait_console(
                page, console, r"speech-check: crop ([0-9.]+) ([0-9.]+) ([0-9.]+) ([0-9.]+)",
                60, "the visible portrait bounds",
            )
            x, y, w, h = map(float, rectangle.groups())
            clip = {"x": x + 0.20 * w, "y": y + 0.42 * h, "width": 0.60 * w, "height": 0.42 * h}
            state = audio_state()
            assert state == "suspended", f"before the first gesture: expected suspended audio, got {state}"
            def snapshot():
                return session.send("Page.captureScreenshot", {
                    "format": "png", "clip": {**clip, "scale": 1},
                })["data"]
            frames = lambda: [match[0] for line in console if (
                match := re.search(r"face: 0 \S+(?: -?\d+\.\d+)+", line)
            )]
            def more_frames(count):
                for _ in range(120):
                    if len(frames()) >= count:
                        return
                    page.wait_for_timeout(50)
                raise AssertionError(f"expected {count} drawn frames, got {len(frames())}")

            # Give the newly created portrait texture time to reach the browser's canvas.
            # Check the submitted mouth weights from the moment the voice was queued.
            rest_count = len(frames())
            more_frames(rest_count + 3)
            still = snapshot()
            more_frames(rest_count + 6)
            before_gesture = snapshot()
            resting = frames()[rest_count:]
            assert len(set(resting)) == 1 and resting[0].startswith("face: 0 - "), (
                "the drawn mouth moved before the first gesture", resting[-4:]
            )
            assert audio_state() == "suspended"
            assert not any("speech-check: presented" in line for line in console)
            if before_gesture != still:
                import base64
                scratch = HERE.parent.parent / "temp"
                (scratch / "speech-before-first.png").write_bytes(base64.b64decode(still))
                (scratch / "speech-before-second.png").write_bytes(base64.b64decode(before_gesture))
                raise AssertionError("the mouth pixels changed before the first gesture")

            page.mouse.click(1240, 300)
            wait_console(page, console, r"speech-check: presented [0-9.]+", 30, "the first presented audio")
            images = {still}
            for _ in range(8):
                images.add(snapshot())
                page.wait_for_timeout(25)
            positions = [float(m[1]) for line in console
                         if (m := re.search(r"speech-check: presented ([0-9.]+)", line))]
            assert audio_state() == "running"
            assert len(positions) > 1 and positions[-1] - positions[0] > 0.10, positions
            assert len(images) >= 3, "the rendered mouth region did not change during playback"
            assert len(set(frames())) > 2, "the drawn morph weights did not change"
            assert not problems, problems
            print(f"browser audio: zero pre-gesture mouth changes, {len(images)} distinct mouth crops; "
                  f"presentation advanced {positions[-1] - positions[0]:.3f} s")
        finally:
            browser.close()
            server.shutdown()


def check_boot_page(directory):
    """The page never spins for ever: each way the start can go wrong ends on the failure screen, saying
    which stage failed. A part of the page that does not load, a page that never arrives, code the browser
    cannot run, a game download that stalls or is refused. Chromium for all, Firefox for the stages that
    need no graphics. (A storage that does not answer or is missing: web/test/first_run.py.)"""
    def screen(page, selector, title=None, timeout=180000):
        page.wait_for_selector(selector, state="visible", timeout=timeout)
        if title:
            got = page.inner_text("#failTitle")
            assert title in got, got
        assert page.is_hidden("#booting") and page.is_hidden("#starting"), "no spinner is left over the screen"

    def case(engine, name, query, init=None, routes=(), then=None):
        with sync_playwright() as p:
            server = serve(directory)
            try:
                browser = launch_engine(p, engine)
            except Exception as e:
                # a browser that is not there is a check that did not run: it fails
                sys.exit(f"boot [{engine}]: no browser ({str(e)[:200]})")
            page = browser.new_context(viewport={"width": 1280, "height": 720}).new_page()
            if init:
                page.add_init_script(init)
            for pattern, handler in routes:
                page.route(pattern, handler)
            page.goto(f"http://127.0.0.1:{server.server_address[1]}/{query}", wait_until="commit")
            then(page)
            print(f"boot [{engine}]: {name}")
            browser.close()
        server.shutdown()

    def never(route):
        pass  # the request is left unanswered: a connection that stalls

    for engine in ("chromium", "firefox"):
        case(engine, "a part of the page that does not load is said", "?cache=http",
             routes=[("**/main.js", lambda r: r.abort())],
             then=lambda page: screen(page, "#failed", "A part of the page did not load"))
        case(engine, "code the browser cannot run is said", "?cache=http",
             routes=[("**/main.js", lambda r: r.fulfill(body="this is ( not javascript", content_type="text/javascript"))],
             then=lambda page: screen(page, "#failed", "could not run in this browser"))
        case(engine, "a page that never arrives ends on the failure screen", "?cache=http",
             routes=[("**/main.js", never)],
             then=lambda page: screen(page, "#failed", "did not start"))

    case("chromium", "a download that stalls ends on the failure screen", "?cache=http",
         routes=[("**/dbota_bg.wasm", never)],
         then=lambda page: (screen(page, "#failed", "could not be downloaded"), assert_in(page.text_content("#failDetail"), "dbota_bg.wasm")))
    case("chromium", "a refused download is said", "?cache=http",
         routes=[("**/dbota_bg.wasm", lambda r: r.fulfill(status=404, body="no"))],
         then=lambda page: (screen(page, "#failed", "could not be downloaded"), assert_in(page.text_content("#failDetail"), "404")))

    def slow_but_moving():
        """A download slower than the stall limit as a whole, but never silent for that long, is
        waited for: each piece of the client restarts the clock, and the game then starts."""
        sent = threading.Event()

        class Trickle(Quiet):
            def do_GET(self):
                if not self.path.split("?")[0].endswith("/dbota_bg.wasm"):
                    return super().do_GET()
                body = pathlib.Path(self.directory, "dbota_bg.wasm").read_bytes()
                self.send_response(200)
                self.send_header("Content-Type", "application/wasm")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                piece = 64 * 1024
                try:
                    # Seven pieces 6 s apart: 36 s in all, with a gap far inside the 30 s limit.
                    for i in range(7):
                        self.wfile.write(body[i * piece:(i + 1) * piece])
                        self.wfile.flush()
                        time.sleep(6)
                    self.wfile.write(body[7 * piece:])
                except OSError:
                    pass
                sent.set()

        with sync_playwright() as p:
            server = serve(directory, Trickle)
            browser = p.chromium.launch(args=BROWSER_ARGS)
            page = browser.new_context(viewport={"width": 1280, "height": 720}).new_page()
            console = []
            page.on("console", lambda m: console.append(m.text))
            page.goto(f"http://127.0.0.1:{server.server_address[1]}/?cache=http", wait_until="commit")
            assert sent.wait(600), "the slow download was served"
            assert page.is_hidden("#failed"), page.inner_text("#failDetail")
            wait_console(page, console, "dbota: running", 600, "the game to start after a slow download")
            assert page.is_hidden("#failed")
            print("boot [chromium]: a slow download that keeps moving is waited for")
            browser.close()
        server.shutdown()

    slow_but_moving()


def check_busy_page(directory):
    """A busy server answers 503 with a Retry-After of seconds (as radovid does when the pages it is
    sending hold their share of its connections). The page says it is busy and when it tries
    again, waits that long and tries once more: a part of the page itself (loaded again whole),
    the game's download and the import worker's module each get through on the second try, and a
    download turned away twice ends on the failure screen naming the 503. Retry-After is read as
    seconds or a date, within 1 to 60 s, 5 s when it is missing or no number."""

    class Busy(Quiet):
        # path -> how many of its first requests are turned away, and the Retry-After sent
        refuse = {}
        asked = {}
        index = None

        def do_GET(self):
            path = self.path.split("?")[0]
            Busy.asked[path] = Busy.asked.get(path, 0) + 1
            times, after = Busy.refuse.get(path, (0, ""))
            if Busy.asked[path] <= times:
                self.send_response(503)
                if after:
                    self.send_header("Retry-After", after)
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            if path == "/cache/index.txt":
                body = Busy.index.encode()
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
                return
            super().do_GET()

    # The index of a served copy as this build writes it (its header is the importer's revision).
    source = (HERE.parent.parent / "crates/dbota_import/src/import.rs").read_text()
    revision = re.search(r"pub const IMPORT_REV: u32 = (\d+)", source).group(1)
    Busy.index = f"dbota-cache 1 rev {revision}\nfile 5 text/labels.json small\n"

    def case(name, query, refuse, then, engine="chromium"):
        Busy.refuse, Busy.asked = refuse, {}
        with sync_playwright() as p:
            server = serve(directory, Busy)
            browser = launch_engine(p, engine)
            page = browser.new_context(viewport={"width": 1280, "height": 720}).new_page()
            page.console = []
            page.on("console", lambda m: page.console.append(m.text))
            page.add_init_script(said_log)
            page.goto(f"http://127.0.0.1:{server.server_address[1]}/{query}", wait_until="commit")
            then(page)
            print(f"busy [{engine}]: {name}")
            browser.close()
        server.shutdown()

    # Every line the page's status lines say, kept by the page itself (over a reload too): a
    # busy line shows for as long as the wait, which a look from here could miss.
    said_log = """(() => {
      const said = JSON.parse(sessionStorage.getItem("said") || "[]");
      new MutationObserver(() => {
        for (const id of ["booting", "starting"]) {
          const text = document.getElementById(id)?.textContent;
          if (text && !said.includes(text)) {
            said.push(text);
            sessionStorage.setItem("said", JSON.stringify(said));
          }
        }
      }).observe(document, { subtree: true, childList: true, characterData: true });
    })();"""

    def says(page, words, timeout=60000):
        deadline = time.time() + timeout / 1000
        said = []
        while time.time() < deadline:
            try:
                said = page.evaluate("() => JSON.parse(sessionStorage.getItem('said') || '[]')")
                if any(words in line for line in said):
                    return
            except Exception:
                pass  # the page is loading again
            time.sleep(0.1)
        sys.exit(f"busy: the page never said {words!r} (said {said}; asked {Busy.asked})")

    def page_again(page):
        # The server's 2 s where asking for the file again is turned away too; where the browser
        # had asked for it more than once (Firefox), its record of the 503 and the usual 5 s.
        says(page, "busy: trying again in ")
        said = page.evaluate("() => JSON.parse(sessionStorage.getItem('said') || '[]')")
        assert any(f"trying again in {n} s" in line for line in said for n in (2, 5)), said
        wait_console(page, page.console, r"first run: ready", 600, "the client's first-run screen")
        assert page.is_hidden("#failed"), page.inner_text("#failDetail")

    for engine in ("chromium", "firefox"):
        case("a part of the page turned away is loaded again after the wait", "",
             {"/main.js": (2, "2")}, page_again, engine)

    def download_again(page):
        says(page, "busy: trying again in 1 s")
        says(page, "Downloading the game", 120000)
        page.wait_for_timeout(3000)
        # Whatever the client then does with the page's arguments, the download is not what failed.
        if page.is_visible("#failed"):
            assert "could not be downloaded" not in page.inner_text("#failTitle"), page.text_content("#failDetail")
        assert Busy.asked["/dbota_bg.wasm"] == 2, Busy.asked

    case("the game's download is fetched again after the wait", "?cache=http",
         {"/dbota_bg.wasm": (1, "1")}, download_again)

    def refused_twice(page):
        page.wait_for_selector("#failed", state="visible", timeout=120000)
        assert_in(page.inner_text("#failTitle"), "could not be downloaded")
        assert_in(page.text_content("#failDetail"), "503")
        assert Busy.asked["/dbota_bg.wasm"] == 2, Busy.asked

    case("a download turned away twice is a failure", "?cache=http",
         {"/dbota_bg.wasm": (2, "1")}, refused_twice)

    def worker_wasm_again(page):
        # The import worker on its own (from a page of the origin that starts nothing): its importer's
        # wasm comes on the second try, the page is told of the wait, and the worker then listens.
        got = page.evaluate("""() => new Promise((done) => {
          const worker = new Worker("./import_worker.js", { type: "module" });
          const heard = [];
          worker.onmessage = ({ data }) => {
            heard.push(data);
            if (data.type !== "busy") { worker.terminate(); done(heard); }
          };
          worker.onerror = (e) => { worker.terminate(); done([...heard, { type: "worker error", message: e.message }]); };
        })""")
        assert got == [{"type": "busy", "seconds": 1}, {"type": "ready"}], got
        assert Busy.asked["/import/dbota_import_bg.wasm"] == 2, Busy.asked

    case("the import worker's wasm is fetched again after the wait", "busy.js",
         {"/import/dbota_import_bg.wasm": (1, "1")}, worker_wasm_again)

    def waits(page):
        got = page.evaluate("""() => {
          const busy = (after) => globalThis.dbota_busy.seconds(new Response(null, { status: 503,
            headers: after === null ? {} : { "retry-after": after } }));
          const at = (s) => new Date(Date.now() + s * 1000).toUTCString();
          return [null, "", "soon", "-3", "0", "7", "9999", at(30), at(-30), "1.5"].map(busy);
        }""")
        assert got[:7] == [5, 5, 5, 5, 1, 7, 60], got
        assert 28 <= got[7] <= 31, got
        assert got[8:] == [1, 5], got

    case("Retry-After is read within its bounds", "", {}, lambda page: (
        page.wait_for_function("() => globalThis.dbota_busy !== undefined", timeout=60000), waits(page)))


def assert_in(text, part):
    assert part in text, f"{part!r} not in {text!r}"


# Where the main menu's buttons are in a 1280x720 page (calibrated by `--shots`): the page is
# drawn at the viewport's size with the interface at its default scale, so they stay put.
# The replay smoke enters the Demo Hero match in a 1920x1080 page (read off its screenshot), then looks
# at MY GAMES in a 1280x720 one. The page is drawn at the viewport's size with the interface at its
# default scale, so what is anchored at the left (the top bar) stays put.
CLICK_CREATE_ACCOUNT = (960, 543)
CLICK_DEMO_HERO_TAB = (700, 24)
CLICK_PUDGE = (814, 252)
def wait_port(port, seconds=60):
    import socket

    deadline = time.time() + seconds
    while time.time() < deadline:
        try:
            socket.create_connection(("127.0.0.1", port), timeout=1).close()
            return
        except OSError:
            time.sleep(0.3)
    sys.exit(f"nothing listens on port {port}")


def delay_game_replies(game_port):
    """Pass the real TLS stream both ways until the test cuts the client's replies."""
    import select
    import socket
    import socketserver

    silent = threading.Event()

    class Transfer(socketserver.BaseRequestHandler):
        def handle(self):
            try:
                with socket.create_connection(("127.0.0.1", game_port), timeout=5) as game:
                    client = self.request
                    for _ in range(100000):
                        readable, _, _ = select.select([client, game], [], [], 60)
                        if not readable:
                            return
                        for source in readable:
                            data = source.recv(65536)
                            if not data:
                                return
                            if source is game:
                                client.sendall(data)
                            elif not silent.is_set():
                                game.sendall(data)
            except ConnectionError:
                return

    relay = socketserver.ThreadingTCPServer(("127.0.0.1", 0), Transfer)
    relay.daemon_threads = True
    threading.Thread(target=relay.serve_forever, daemon=True).start()
    return relay, silent


def watch_game_close(page, port):
    page.add_init_script(f"""(() => {{
        const BrowserSocket = globalThis.WebSocket;
        globalThis.WebSocket = class extends BrowserSocket {{
            constructor(url, ...options) {{
                super(url, ...options);
                if (new URL(url).port === '{port}') {{
                    this.addEventListener('close', event => {{
                        globalThis.gameClose = [event.code, event.reason];
                    }});
                }}
            }}
        }};
    }})();""")


def assert_game_timeout(page, console, silent, evidence=None):
    # The network cut is the only stimulus. Chromium and the game client
    # receive the server's bytes, and the Bevy menu draws its own status.
    import base64

    expected = "The game server stopped receiving network replies from this client."
    silent.set()
    for _ in range(500):
        if close := page.evaluate("globalThis.gameClose"):
            break
        page.wait_for_timeout(30)
    else:
        raise AssertionError("the browser received no WebSocket close event")
    assert close == [4001, "game transport idle"], close
    wait_console(page, console, re.escape(f"game: connection ended: {expected}"), 15, "the disconnect toast")
    page.evaluate("new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done)))")
    width, height = page.viewport_size["width"], page.viewport_size["height"]
    clip = {"x": width // 5, "y": round(height * .85), "width": width * 3 // 5,
            "height": round(height * .08)}
    pixels = page.screenshot(path=evidence, clip=clip, timeout=15000)
    ink = page.evaluate("""async (encoded) => {
        const png = Uint8Array.from(atob(encoded), value => value.charCodeAt(0));
        const screenshot = await createImageBitmap(new Blob([png], {type: 'image/png'}));
        const canvas = document.createElement('canvas');
        canvas.width = screenshot.width;
        canvas.height = screenshot.height;
        const graphics = canvas.getContext('2d');
        graphics.drawImage(screenshot, 0, 0);
        const rgba = graphics.getImageData(0, 0, canvas.width, canvas.height).data;
        let ink = 0;
        for (let pixel = 0; pixel < rgba.length; pixel += 4) {
            const [red, green, blue] = rgba.subarray(pixel, pixel + 3);
            if (red >= 230 && green >= 118 && green <= 148 && blue >= 92 && blue <= 130) ink++;
        }
        return ink;
    }""", base64.b64encode(pixels).decode("ascii"))
    assert ink >= 25, f"the game's orange disconnect text is missing ({ink} foreground pixels)"
    print("game WebSocket: the server closed with 4001 and the client drew the idle reason")


def radovid_config(directory, page_dir, server_bin, port, game_port, cert, key, relay_port):
    """A radovid for one demo ruleset whose games keep their replay and end quickly once the
    player is gone, serving `page_dir` on its own origin."""
    fingerprint = subprocess.run([server_bin, "--wire-fingerprint"], capture_output=True, text=True).stdout.strip()
    return f"""
listen = "127.0.0.1:{port}"
database = "{directory}/radovid.db"
tls_cert = "{cert}"
tls_key = "{key}"
game_ports = "{game_port}-{game_port}"
lobby_teams = [5, 5]
[web]
static_dir = "{page_dir}"
[password]
memory_kib = 1024
iterations = 1
[artifacts]
save_replays = true
[limits]
game_capacity = 1
lobby_games_max = 1
[[ruleset]]
id = "demo"
name = "Demo Hero"
instant_solo = true
info = "01"
game = "bota"
game_fingerprint = "{fingerprint}"
teams = [1, 1]
rated = false
lobby_min_players = 1
max_duration_secs = 600
[ruleset.launcher]
kind = "local"
binary = "{server_bin}"
public_url = "wss://127.0.0.1:{relay_port}"
version_probe = ["--wire-fingerprint"]
args = ["--map", "0", "--cheats", "--match-id", "{{match_id}}", "--bind", "127.0.0.1", "--port", "0", "--ws-port", "{{port}}",
        "--tls-cert", "{cert}", "--tls-key", "{key}", "--idle-timeout-secs", "1", "--heroes", "1", "--pick-secs", "1", "--tactical-secs", "0",
        "--start-after-secs", "120", "--abandon-unjoined", "--empty-after-secs", "5"]
env = {{ BOTA_SEATS = "{{seats}}" }}
artifact = "{directory}/replays/replay-{{match_id}}.brp"
artifact_args = ["--replay", "{{artifact}}"]
"""


OPFS_REPLAYS = """async () => {
  const root = await navigator.storage.getDirectory();
  let dir;
  try { dir = await root.getDirectoryHandle('dbota-replays'); } catch (error) {
    if (error.name === 'NotFoundError') return [];
    throw error;
  }
  const out = [];
  for await (const [name, handle] of dir.entries()) {
    if (handle.kind !== 'file') continue;
    try {
      out.push([name, (await handle.getFile()).size]);
    } catch (e) {
      // The client can remove an entry after this independent probe lists it.
      if (!e || e.name !== 'NotFoundError') throw e;
    }
  }
  return out;
}"""

def check_replay_probe(directory, context):
    """An OPFS inspection survives concurrent deletion and still reports unrelated read errors."""
    server = serve(directory)
    try:
        page = context.new_page()
        page.route("**/*", lambda route: route.fulfill(content_type="text/html", body="<html></html>"))
        page.goto(f"http://127.0.0.1:{server.server_address[1]}/probe")
        page.evaluate("""async () => {
          const root = await navigator.storage.getDirectory();
          const dir = await root.getDirectoryHandle('dbota-replays', {create: true});
          await dir.getFileHandle('kept.brp', {create: true});
          await dir.getFileHandle('removed.tmp', {create: true});
          const read = FileSystemFileHandle.prototype.getFile;
          FileSystemFileHandle.prototype.getFile = async function() {
            if (this.name === 'removed.tmp') await dir.removeEntry(this.name);
            return read.call(this);
          };
        }""")
        assert page.evaluate(OPFS_REPLAYS) == [["kept.brp", 0]], "the surviving file is still inspected"
        page.evaluate("""() => {
          FileSystemFileHandle.prototype.getFile = async function() {
            throw new DOMException('probe read refused', 'NotReadableError');
          };
        }""")
        try:
            page.evaluate(OPFS_REPLAYS)
        except PlaywrightError as error:
            assert "NotReadableError: probe read refused" in str(error), error
        else:
            raise AssertionError("the probe must report unexpected I/O errors")
        page.close()
    finally:
        server.shutdown()
    print("replay probe: concurrent deletion is harmless, unexpected read errors are reported")


def check_replays_page(directory, server_bin, radovid_bin, shots=False):
    """A Demo Hero match through radovid: the page registers, enters the match, and draws the game's
    idle reason when the server stops hearing the client.

    KNOWN-INCOMPLETE: this does not yet check that a match is kept in OPFS, listed in MY GAMES, played,
    and deleted. A Demo Hero match is not kept, so those steps need a seated match (a practice lobby):
    follow-up unit."""
    evidence = HERE.parent.parent / "temp/smoke-replays"
    evidence.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="service-", dir=evidence) as folder:
        work = pathlib.Path(folder)
        cert, key = work / "cert.pem", work / "key.pem"
        subprocess.run([radovid_bin, "dev-cert", str(work)], check=True, capture_output=True)
        port, game = free_port(), free_port()
        page_dir = work / "page"
        shutil.copytree(directory, page_dir, copy_function=os.link)
        # Inherit the cache's files by link, leaving this check's directories removable.
        shutil.copytree(cache_folder(), page_dir / "cache", copy_function=os.link)
        for path, _, _ in os.walk(page_dir):
            os.chmod(path, 0o755)
        relay, silent = delay_game_replies(game)
        config = work / "radovid.toml"
        config.write_text(radovid_config(work, page_dir, server_bin, port, game, cert, key, relay.server_address[1]))
        with (evidence / "radovid.log").open("w") as log, subprocess.Popen(
            [radovid_bin, "serve", "--config", str(config)], stdout=subprocess.DEVNULL,
            stderr=log, env=without_steering_variables(),
        ) as service:
            try:
                wait_port(port)
                drive_replays_page(directory, port, relay.server_address[1], silent, evidence if shots else None)
            finally:
                relay.shutdown()
                relay.server_close()
                service.terminate()
                service.wait(timeout=20)
    print("replays page: a Demo Hero match was entered and the idle reason was drawn; "
          "KNOWN-INCOMPLETE: nothing is kept, listed, played or deleted after a demo (needs a seated match)")


def start_demo(page, console, silent, shot, evidence):
    """Create the account, enter a Demo Hero match from the hero catalogue and cut the client's replies."""
    click_drawn(page, CLICK_CREATE_ACCOUNT)
    for text in ("smoker", "correct horse battery", "correct horse battery"):
        page.keyboard.type(text)
        page.keyboard.press("Tab")
        after_frames(page)
    page.keyboard.press("Enter")
    wait_for_sign_in(page)
    for at in (CLICK_DEMO_HERO_TAB, CLICK_PUDGE):  # a hero's card on the Demo Hero page starts its demo
        click_drawn(page, at)
    wait_console(page, console, "loading: done in", 600, "the demo match")
    shot(page, "6-game")
    assert_game_timeout(page, console, silent, evidence / "6-idle-timeout.png" if evidence else None)
    shot(page, "6-idle-menu")


def wait_for_sign_in(page):
    wait_browser_state(page, lambda: page.evaluate("window.smokeSignedIn === true"), "sign-in", 120)
    after_frames(page)


def drive_replays_page(directory, port, relay_port, silent, evidence):
    with sync_playwright() as p:
        browser = p.chromium.launch(args=[*BROWSER_ARGS, "--ignore-certificate-errors"])
        context = browser.new_context(viewport={"width": 1920, "height": 1080}, ignore_https_errors=True)
        check_replay_probe(directory, context)
        # Each visit has to receive its own successful login, even when a token is already saved.
        context.add_init_script("""(() => {
          const save = Storage.prototype.setItem;
          Storage.prototype.setItem = function(key, value) {
            save.call(this, key, value);
            if (key === 'dbota.session') window.smokeSignedIn = true;
          };
        })()""")
        page = context.new_page()
        console, problems = [], []
        page.on("console", lambda m: console.append(m.text))
        page.on("pageerror", lambda e: problems.append(f"page error: {e}"))
        watch_game_close(page, relay_port)
        page.add_init_script("Element.prototype.requestPointerLock = () => Promise.resolve();")
        page.goto(f"https://127.0.0.1:{port}/?cache=http&gpu=webgl2")
        wait_console(page, console, "dbota: running", 600, "the live menu")
        assert_launch_labels_hidden(page)
        after_frames(page)

        def shot(page, name):
            if evidence is not None:
                page.screenshot(path=str(evidence / f"{name}.png"), timeout=600000)

        start_demo(page, console, silent, shot, evidence)
        page.close()
        assert not problems, f"first visit: {problems}"
        browser.close()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--replay-page", type=pathlib.Path, help="a build with a packed replay built in")
    ap.add_argument("--scene-page", type=pathlib.Path, help="a replay build: forest and heroes drawn on both graphics APIs")
    ap.add_argument("--terrain-page", type=pathlib.Path, help="full-map replay build without a seat POV")
    ap.add_argument("--terrain-api", choices=["webgpu", "webgl2"], action="append")
    ap.add_argument("--terrain-shots", type=pathlib.Path)
    ap.add_argument("--terrain-refused-shot", type=pathlib.Path, help="capture a match with an oversized ability icon")
    ap.add_argument("--fullscreen-page", type=pathlib.Path, help="a replay build opened on the Video settings")
    ap.add_argument("--server", help="a bota_server built with the websocket feature, for --replays-page")
    ap.add_argument("--fonts-page", type=pathlib.Path, help="any build: the page holds its interface fonts")
    ap.add_argument("--startup-page", type=pathlib.Path, help="any build: the client picked, failures shown")
    ap.add_argument("--launch-page", type=pathlib.Path, help="any build: the client starts and draws without a panic")
    ap.add_argument("--speech-page", type=pathlib.Path, help="a replay build with --speech-check --face-trace")
    ap.add_argument("--sound-page", type=pathlib.Path, help="any build: the audio context is resumed by the first gesture")
    ap.add_argument("--keys-page", type=pathlib.Path, help="any page, even web/ unbuilt: whose keys are whose")
    ap.add_argument("--cursor-page", type=pathlib.Path, help="a replay build: the cursor kept in a match")
    ap.add_argument("--cursor-input-page", type=pathlib.Path, help="a replay build: the cursor with a real mouse (X server)")
    ap.add_argument("--ui-art-page", type=pathlib.Path, help="a match replay with Shadow Fiend's HUD and an active item tooltip")
    ap.add_argument("--boot-page", type=pathlib.Path, help="any build: every way the start can go wrong ends on a screen")
    ap.add_argument("--busy-page", type=pathlib.Path, help="any build: a busy server's 503 is waited for and tried once more")
    ap.add_argument("--pixels-page", type=pathlib.Path, help="a replay build: the canvas drawn at its device pixels")
    ap.add_argument("--replays-page", type=pathlib.Path, help="a build without a replay: a match through radovid is kept in OPFS")
    ap.add_argument("--radovid", help="the radovid binary, for --replays-page (with --server)")
    ap.add_argument("--shots", action="store_true", help="save screenshots during the replay or fullscreen check")
    args = ap.parse_args()
    if args.sound_page:
        check_sound_page(args.sound_page)
    if args.ui_art_page:
        check_ui_art_page(args.ui_art_page)
    if args.speech_page:
        check_speech_page(args.speech_page)
    if args.launch_page:
        check_launch_page(args.launch_page)
    if args.startup_page:
        check_startup_page(args.startup_page)
    if args.fonts_page:
        check_fonts_page(args.fonts_page)
    if args.keys_page:
        check_browser_keys(args.keys_page)
        check_escape_in_fullscreen(args.keys_page)
    if args.replay_page:
        check_replay_page(args.replay_page)
    if args.scene_page:
        check_scene_page(args.scene_page)
    if args.terrain_page:
        if args.terrain_shots:
            args.terrain_shots.mkdir(parents=True, exist_ok=True)
        check_scene_page(
            args.terrain_page, args.terrain_api or ("webgpu", "webgl2"), terrain=True,
            shots=args.terrain_shots, refused_shot=args.terrain_refused_shot
        )
    if args.fullscreen_page:
        check_fullscreen_page(args.fullscreen_page, args.shots)
    if args.cursor_page:
        check_cursor_page(args.cursor_page)
    if args.cursor_input_page:
        check_cursor_input(args.cursor_input_page)
    if args.boot_page:
        check_boot_page(args.boot_page)
    if args.busy_page:
        check_busy_page(args.busy_page)
    if args.pixels_page:
        check_canvas_pixels(args.pixels_page)
    if args.replays_page:
        if not (args.server and args.radovid):
            sys.exit("--replays-page needs --server and --radovid")
        check_replays_page(args.replays_page, args.server, args.radovid, args.shots)


if __name__ == "__main__":
    main()
