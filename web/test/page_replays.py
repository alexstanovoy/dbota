"""`--replays-page`: a build without a replay: a match through radovid is kept in OPFS.
"""

import base64
import os
import pathlib
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
from playwright.sync_api import Error as PlaywrightError, sync_playwright

from radovid_service import radovid_config
from browser import (
    BROWSER_ARGS,
    HERE,
    after_frames,
    assert_launch_labels_hidden,
    cache_folder,
    click_drawn,
    serve,
    wait_console,
    without_steering_variables,
)

FLAG = "--replays-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (210, ('--replays-page',), dict(type=pathlib.Path, help="a build without a replay: a match through radovid is kept in OPFS")),
)
ORDER = 190


def run(args):
    if not (args.server and args.radovid):
        sys.exit("--replays-page needs --server and --radovid")
    check_replays_page(args.replays_page, args.server, args.radovid, args.shots)


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


# The page is drawn at the viewport's size with the interface at its default scale, which grows with the window's
# height: what is anchored at the left (the top bar) is where it is at 1280x720 times the height over 720.
# Demo Hero is entered in a 1920x1080 page (positions read off its screenshot) and everything after it is looked at
# in a 1280x720 one. The practice lobby's buttons were read off the native client's layout of the same window
# (their named widgets' centres: e2e tests lay the same menu out), MY GAMES' rows off the first page that showed them.
CLICK_CREATE_ACCOUNT = (960, 588)
# The way into a demo: the Heroes tab, a hero's card, the DEMO HERO button of its page (no tab or page of its own). The
# tab is where the browser's bar has it (no LOCAL LOBBY before it); the card and the button are the native client's
# laid-out widgets "Pudge" and "demo hero" at 1920x1080.
CLICK_HEROES_TAB = (588, 28)
CLICK_PUDGE_CARD = (79.5, 207.5)
CLICK_DEMO_HERO_BUTTON = (181.5, 578)
CLICK_PLAY_TAB = (159, 18.5)
CLICK_CUSTOM_LOBBIES = (1142, 454)
CLICK_CREATE_LOBBY = (1142, 551.5)
CLICK_START_GAME = (1136, 690.5)
CLICK_MY_GAMES = (220.5, 18.5)
CLICK_LOG_OUT = (1132, 16)
# The sign-in form at 1280x720 (two thirds of its positions in the 1920x1080 screenshot).
CLICK_LOGIN_FIELD = (640, 255)
CLICK_PASSWORD_FIELD = (640, 309)
CLICK_SIGN_IN = (640, 354)
CLICK_CREATE_ACCOUNT_SMALL = (640, 392)
CLICK_WATCH_REPLAY = (646, 150)
CLICK_DELETE_REPLAY = (788, 150)
CLICK_CONFIRM_DELETE = (578, 388)


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

OPFS_REPLAY_INDEX = """async () => {
  const store = globalThis.dbota_replays;
  if (!store) return '';
  if (store.kind !== 'opfs') throw new Error('the replay store must use OPFS');
  return (await store.load()).index ?? '';
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
    """Through a real radovid: a Demo Hero match draws the game's idle reason when the server stops hearing the
    client; a seated match (a practice lobby) is kept in OPFS when the player has left it, is listed in MY GAMES,
    played from there, marked broken when its file is damaged, hidden from other accounts, and deleted with
    its confirmation (the file and its index row)."""
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
                drive_replays_page(directory, work, port, relay.server_address[1], silent, evidence if shots else None)
            finally:
                relay.shutdown()
                relay.server_close()
                service.terminate()
                service.wait(timeout=20)
    print("replays: a Demo Hero match drew the idle reason; a seated match was kept in OPFS, listed, played, "
          "marked broken, hidden from other accounts and deleted")


def recorded_replay(work, match_id):
    # The files can appear before the service publishes the match result.
    deadline = time.monotonic() + 300
    replay = work / "replays" / f"replay-{match_id}.brp"
    with sqlite3.connect((work / "radovid.db").as_uri() + "?mode=ro", uri=True) as database:
        while time.monotonic() < deadline:
            if replay.is_file() and database.execute(
                "SELECT 1 FROM matches WHERE id = ? AND finished = 1", (match_id,),
            ).fetchone():
                size = replay.stat().st_size
                assert size >= 1024, f"replay {match_id} contains only {size} bytes"
                return size
            time.sleep(0.25)
    raise AssertionError(f"the service did not record the result and replay of match {match_id}")


def start_demo(page, console, silent, shot, evidence):
    """Create the account, enter a Demo Hero match from a hero's page and cut the client's replies."""
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
    shot(page, "demo-0-play")
    for step, at in (("heroes", CLICK_HEROES_TAB), ("hero-page", CLICK_PUDGE_CARD), (None, CLICK_DEMO_HERO_BUTTON)):
        click_drawn(page, at)  # Heroes, Pudge, DEMO HERO
        if step:
            shot(page, f"demo-1-{step}")
    wait_console(page, console, "loading: done in", 600, "the demo match")
    shot(page, "6-game")
    assert_game_timeout(page, console, silent, evidence / "6-idle-timeout.png" if evidence else None)
    shot(page, "6-idle-menu")


def wait_for_sign_in(page):
    wait_browser_state(page, lambda: page.evaluate("window.smokeSignedIn === true"), "sign-in", 120)
    after_frames(page)


def start_practice(page, console, shot):
    """From Play: a custom lobby of the 1x1 mid ruleset, alone, started; returns the match id the client noted
    for fetching its replay."""
    for at in (CLICK_PLAY_TAB, CLICK_CUSTOM_LOBBIES, CLICK_CREATE_LOBBY):
        click_drawn(page, at)
    wait_console(page, console, "lobby: creation confirmed", 300, "lobby creation")
    after_frames(page)
    shot(page, "5-lobby")
    click_drawn(page, CLICK_START_GAME)
    wait_console(page, console, r"match known: tick \d+", 600, "the practice match")
    claim = wait_browser_state(
        page, lambda: re.search(r"^w\t(\d+)\t\d+\t[^\t\n]+", page.evaluate(OPFS_REPLAY_INDEX), re.MULTILINE),
        "the practice match's persisted replay claim", 300,
    )
    shot(page, "6-practice")
    return int(claim[1])


def saved_replay(page, match_id):
    index = page.evaluate(OPFS_REPLAY_INDEX)
    return re.search(rf"^r\t\d+\t\d+\t{match_id}\t[^\t\n]+\t([^\t\n]+)\t", index, re.MULTILINE)


def register_replay_player(page, login):
    click_drawn(page, CLICK_CREATE_ACCOUNT_SMALL)
    page.keyboard.type(login)
    page.keyboard.press("Tab")
    after_frames(page)
    page.keyboard.type("correct horse battery")
    page.keyboard.press("Tab")
    after_frames(page)
    page.keyboard.type("correct horse battery")
    page.keyboard.press("Enter")
    wait_for_sign_in(page)


def change_replay_account(page, console, name):
    """The saved file stays in OPFS while the second account and the guest see an empty shelf."""
    def logout():
        console.clear()
        click_drawn(page, CLICK_LOG_OUT)
        wait_browser_state(page, lambda: page.evaluate("window.smokeSignedIn === false"), "sign-out", 30)
        wait_console(page, console, r"replays: 0 kept", 30, "the guest shelf")
        assert any(file == name for file, _ in page.evaluate(OPFS_REPLAYS)), "the replay stays on disk"

    logout()
    console.clear()
    register_replay_player(page, "other")
    wait_console(page, console, r"replays: 0 kept", 30, "the second account's shelf")
    click_drawn(page, CLICK_MY_GAMES)
    logout()

    console.clear()
    click_drawn(page, CLICK_LOGIN_FIELD)
    for _ in "other":
        page.keyboard.press("Backspace")
        after_frames(page)
    page.keyboard.type("smoker")
    click_drawn(page, CLICK_PASSWORD_FIELD)
    page.keyboard.type("correct horse battery")
    click_drawn(page, CLICK_SIGN_IN)
    wait_for_sign_in(page)
    wait_console(page, console, r"replays: 1 kept", 30, "Alice's shelf")
    print("replay shelf: account switching hides the saved file until its owner returns")


def drive_replays_page(directory, work, port, relay_port, silent, evidence):
    with sync_playwright() as p:
        browser = p.chromium.launch(args=[*BROWSER_ARGS, "--ignore-certificate-errors"])
        context = browser.new_context(viewport={"width": 1920, "height": 1080}, ignore_https_errors=True)
        check_replay_probe(directory, context)
        # Each visit has to receive its own successful login, even when a token is already saved.
        context.add_init_script("""(() => {
          const save = Storage.prototype.setItem;
          const remove = Storage.prototype.removeItem;
          Storage.prototype.removeItem = function(key) {
            remove.call(this, key);
            if (key === 'dbota.session') window.smokeSignedIn = false;
          };
          Storage.prototype.setItem = function(key, value) {
            save.call(this, key, value);
            if (key === 'dbota.session') window.smokeSignedIn = true;
          };
        })()""")
        # The configured service is checked before the sign-in form opens.
        context.add_init_script(f"""
            if (!localStorage.getItem('dbota.menu'))
                localStorage.setItem('dbota.menu', 'service = wss://127.0.0.1:{port}');
        """)
        url = f"https://127.0.0.1:{port}/?cache=http&gpu=webgl2"

        def visit(width=1280, height=720, first=False):
            page = context.new_page()
            page.set_viewport_size({"width": width, "height": height})
            console, problems = [], []
            page.on("console", lambda m: console.append(m.text))
            page.on("pageerror", lambda e: problems.append(f"page error: {e}"))
            if first:
                watch_game_close(page, relay_port)
                page.add_init_script("Element.prototype.requestPointerLock = () => Promise.resolve();")
            page.goto(url)
            wait_console(page, console, "dbota: running", 600, "the live menu")
            if first:
                assert_launch_labels_hidden(page)
            after_frames(page)
            return page, console, problems

        def shot(page, name):
            if evidence is not None:
                page.screenshot(path=str(evidence / f"{name}.png"), timeout=600000)

        # Demo Hero: the idle reason; the match leaves nothing to keep.
        page, console, problems = visit(1920, 1080, first=True)
        start_demo(page, console, silent, shot, evidence)
        page.close()
        assert not problems, f"demo visit: {problems}"

        # A seated match, left at once: the replay is the player's to keep.
        page, console, problems = visit()
        wait_for_sign_in(page)
        match_id = start_practice(page, console, shot)
        page.close()
        assert not problems, f"practice visit: {problems}"
        replay_size = recorded_replay(work, match_id)

        page, console, problems = visit()
        wait_for_sign_in(page)
        wait_console(page, console, r"replays: 1 kept", 300, "the replay kept in OPFS")
        record = wait_browser_state(page, lambda: saved_replay(page, match_id), "the replay's OPFS index row", 300)
        stored = page.evaluate(OPFS_REPLAYS)
        replay = [(name, size) for name, size in stored if name.endswith(".brp")]
        assert len(replay) == 1, f"the replay is stored in OPFS: {stored}"
        name, size = replay[0]
        assert name == record[1], "the index names the stored replay"
        assert size == replay_size, "the stored replay is the service's file"
        assert any(file == "index.txt" for file, _ in stored), "the index is beside it"
        assert not problems, f"fetch visit: {problems}"
        print(f"stored: {name} ({size} bytes) in OPFS")
        page.close()

        page, console, problems = visit()
        wait_for_sign_in(page)
        wait_console(page, console, r"replays: 1 kept", 600, "the replay listed again")
        click_drawn(page, CLICK_MY_GAMES)
        shot(page, "8-my-games")
        click_drawn(page, CLICK_WATCH_REPLAY)
        wait_console(page, console, r"match known: tick \d+", 600, "the stored replay to play")
        shot(page, "8-watch")
        bad = [line for line in console if "cannot be read" in line or "panicked" in line or "RuntimeError" in line]
        assert not bad and not problems, (bad, problems)
        print("watch: the stored replay plays from MY GAMES")
        # Damage the owned file after the successful playback. Its index and owner stay put.
        page.evaluate("""async name => {
            const root = await navigator.storage.getDirectory();
            const replays = await root.getDirectoryHandle('dbota-replays');
            const write = await (await replays.getFileHandle(name)).createWritable();
            await write.write(new Uint8Array(2048).fill(7));
            await write.close();
        }""", name)
        page.close()

        page, console, problems = visit()
        wait_for_sign_in(page)
        wait_console(page, console, r"replays: 1 kept", 300, "the replay available for inspection")
        click_drawn(page, CLICK_MY_GAMES)
        wait_console(page, console, r"replays: a kept replay is unreadable", 30, "the broken replay row")
        shot(page, "9-broken-replay")
        click_drawn(page, CLICK_WATCH_REPLAY)
        assert not any("match known:" in line for line in console), "a broken replay has no watch action"
        print("replay shelf: an unreadable file is marked broken")

        change_replay_account(page, console, name)
        click_drawn(page, CLICK_MY_GAMES)
        wait_console(page, console, r"replays: a kept replay is unreadable", 30, "Alice's broken replay after signing back in")
        size = 2048
        click_drawn(page, CLICK_DELETE_REPLAY)
        shot(page, "9-delete-confirm")
        assert (name, size) in map(tuple, page.evaluate(OPFS_REPLAYS)), "one click keeps the file"
        page.keyboard.press("Escape")
        after_frames(page)
        assert (name, size) in map(tuple, page.evaluate(OPFS_REPLAYS)), "cancellation keeps the file"
        click_drawn(page, CLICK_DELETE_REPLAY)
        click_drawn(page, CLICK_CONFIRM_DELETE)
        wait_browser_state(
            page,
            lambda: not saved_replay(page, match_id) and all(file != name for file, _ in page.evaluate(OPFS_REPLAYS)),
            "removal of the kept replay and its index row after the confirmed deletion", 120,
        )
        assert not problems, f"delete visit: {problems}"
        print("delete: OPFS keeps a replay until deletion is confirmed; confirmed, the file and its index row go")
        browser.close()


