"""`--replays-page`: a build without a replay: a match through radovid is kept in OPFS.
"""

import base64
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from playwright.sync_api import Error as PlaywrightError, sync_playwright

from browser import (
    BROWSER_ARGS,
    HERE,
    after_frames,
    assert_launch_labels_hidden,
    cache_folder,
    click_drawn,
    serve,
    wait_console,
)

FLAG = "--replays-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (210, ('--replays-page',), dict(type=pathlib.Path, help="a build without a replay: a match through radovid is kept in OPFS")),
    (80, ('--server',), dict(help="a bota_server built with the websocket feature, for --replays-page")),
    (220, ('--radovid',), dict(help="the radovid binary, for --replays-page (with --server)")),
)
ORDER = 170


def run(args):
    if not (args.server and args.radovid):
        sys.exit("--replays-page needs --server and --radovid")
    check_replays_page(args.replays_page, args.server, args.radovid, args.shots)


# The environment of the services the checks start, which start game servers: without the variables that steer
# a client or a server from outside (tools/client_environment.txt), such as a seat list in the caller's shell.
def without_steering_variables():
    listed = (HERE.parent.parent / "tools/client_environment.txt").read_text().splitlines()
    steering = {line.strip() for line in listed if line.strip() and not line.startswith("#")}
    return {name: value for name, value in os.environ.items() if name not in steering}


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


def free_port():
    import socket

    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


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
    # The page's own service is the one chosen, and it is checked (through the browser's
    # WebSocket) before the sign-in form opens: the form below is typed into only then.
    wait_console(page, console, r"service wss://127\.0\.0\.1:\d+ passed its check", 600,
                 "the page's own service checked")
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
