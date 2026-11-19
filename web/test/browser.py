"""What the browser checks share (web/test/page_*.py, smoke.py, and the scripts beside them): the page served, the
browsers started, waits on the console. A page's own helpers are in its module."""

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


# Marks the test's own pointer events, which stand for the mouse's (a locked pointer's motion cannot
# be injected): the page's re-dispatched events are the ones without it.
OWN_EVENT = 7
HANDED = """window.handed = [];
for (const type of ['pointermove', 'pointerdown', 'pointerup'])
    addEventListener(type, (e) => { if (!e.isTrusted && e.detail !== %d) handed.push([e.clientX, e.clientY]); }, true);""" % OWN_EVENT


def assert_launch_labels_hidden(page):
    assert page.is_hidden("#booting"), "the page's loading label covers the game canvas"
    assert page.is_hidden("#starting"), "the startup label covers the game canvas"
    assert page.is_hidden("#gate"), "the asset gate covers the game canvas"


def assert_in(text, part):
    assert part in text, f"{part!r} not in {text!r}"


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
