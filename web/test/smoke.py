#!/usr/bin/env python3
"""Smoke test of the page in a headless Chromium (Playwright): picks a Dota 2 folder, has the
worker import it into the browser's private storage, starts the client on it, reloads to see
the cache is kept, and deletes it. Also checks nothing but the page's own files is requested.

    python3 -m pip install --target temp/pwlib playwright && PYTHONPATH=temp/pwlib python3 -m playwright install chromium
    web/build.sh                                  # builds web/out
    PYTHONPATH=temp/pwlib python3 web/test/smoke.py [--install "<dota 2 beta dir>"] [--no-game]

The folder given to the page is a farm of symlinks to the install's package files, so the test
shows the "pick a folder, read only what is needed" path without copying anything. Needs the
Dota 2 install (DBOTA_DOTA_PATH or the usual Steam places); the game run needs WebGL2/WebGPU
(SwiftShader does). Without an install that part is skipped, with a message.

Five more checks need no install and run on pages built with stand-ins (`?assets=none`):

    DBOTA_WEB_REPLAY=$PWD/chat.brp DBOTA_WEB_ARGS="--replay built-in --pov 0 --seek 4100" \
        PROFILE=release OUT=temp/web-replay web/build.sh
    PROFILE=release OUT=temp/web-live web/build.sh
    CARGO_TARGET_DIR=temp/bota-target cargo build --release --manifest-path ../bota/Cargo.toml \
        -p bota_server --features websocket
    PYTHONPATH=temp/pwlib python3 web/test/smoke.py --replay-page temp/web-replay \
        --live-page temp/web-live --server temp/bota-target/release/bota_server --startup-page temp/web-live

* `--startup-page` (any build): the page loads the WebGPU client, or the WebGL2 one with
  `?gpu=webgl2`, and each draws; a browser without graphics, a renderer that cannot be made (the
  page then offers the WebGL2 client, which runs) and a client out of memory each end with a
  message on the page.
* `--replay-page`: the page plays a Zstandard-packed replay built into it (read through to the
  seek tick, as the packed file is decoded record by record) and the client says it knows the
  match and heard the chat lines, with no read error.
* `--keys-page` (any page, even `web/` unbuilt): over the canvas the browser keeps its Ctrl and
  Cmd chords and F12, the game its keys (F5, Tab, Space, letters, right click, wheel).
* `--pixels-page` (a replay build): at a device scale of 1.5, and after the page is resized, the
  canvas's drawing buffer and the client's surface are the canvas's device-pixel size.
* `--fullscreen-page`: a replay build made with `DBOTA_WEB_ARGS="--replay built-in --pov 0 --settings video"`;
  the display mode and F11 put the canvas fullscreen and leaving it turns the setting back.
* `--live-page` and `--server`: the page joins a bota_server over a WebSocket through the main
  menu's direct connection, asks for compression, takes the compressed snapshots in (the match
  goes on past its start), and chats a line of several scripts that comes back through the
  server. The page's Content-Security-Policy gets that one socket for this run only.
"""
import argparse
import functools
import http.server
import json
import os
import pathlib
import re
import subprocess
import sys
import threading
import time

from playwright.sync_api import sync_playwright

HERE = pathlib.Path(__file__).resolve().parent
OUT = HERE.parent / "out"
STEAM = [
    "~/.var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/common/dota 2 beta",
    "~/.steam/steam/steamapps/common/dota 2 beta",
    "~/.local/share/Steam/steamapps/common/dota 2 beta",
]


def find_install(given):
    for c in [given, os.environ.get("DBOTA_DOTA_PATH"), *STEAM]:
        if c and (pathlib.Path(c).expanduser() / "game/dota/pak01_dir.vpk").is_file():
            return pathlib.Path(c).expanduser()
    return None


def farm(install, root):
    """`root/dota 2 beta/game/dota/*.vpk` as hard links (Chromium's folder upload skips symlinks;
    nothing is copied), plus a decoy file the page must ignore."""
    dota = root / "dota 2 beta/game/dota"
    (dota / "maps").mkdir(parents=True, exist_ok=True)
    src = install / "game/dota"
    for vpk in src.glob("*.vpk"):
        link = dota / vpk.name
        if not link.exists():
            os.link(vpk, link)
    for vpk in (src / "maps").glob("*.vpk"):
        link = dota / "maps" / vpk.name
        if not link.exists():
            os.link(vpk, link)
    (dota / "gameinfo.gi").write_text("not a package")
    return root / "dota 2 beta"


class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


def serve(directory=OUT):
    handler = functools.partial(Quiet, directory=str(directory))
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


GRANT_LOGIC = """async () => {
  const grant = await import('./grant.js');
  const fake = (path) => ({ webkitRelativePath: path, name: path.split('/').pop() });
  const keys = (m) => [...m.keys()].sort().join(',');
  const out = {};
  // The fallback picker's list: every file below the folder; only packages are kept, relative
  // to the folder holding pak01_dir.vpk closest to the top.
  out.list = keys(grant.filesFromList([
    fake('dota 2 beta/game/dota/pak01_dir.vpk'), fake('dota 2 beta/game/dota/pak01_000.vpk'),
    fake('dota 2 beta/game/dota/maps/dota.vpk'), fake('dota 2 beta/game/dota/maps/deep/x.vpk'),
    fake('dota 2 beta/game/dota/addons/pak01_dir.vpk'), fake('dota 2 beta/game/dota/cfg.txt'),
    fake('dota 2 beta/game/bin/pak01_001.vpk'), fake('dota 2 beta/game/dota/../x.vpk'),
  ]));
  out.none = keys(grant.filesFromList([fake('dota 2 beta/readme.txt')]));
  out.direct = keys(grant.filesFromList([fake('dota/pak01_dir.vpk'), fake('dota/pak01_002.vpk')]));
  // The directory picker: a folder tree in private storage stands in for the player's folder.
  const root = await navigator.storage.getDirectory();
  const base = await root.getDirectoryHandle('grant-test', { create: true });
  const dota = await (await (await base.getDirectoryHandle('dota 2 beta', { create: true }))
    .getDirectoryHandle('game', { create: true })).getDirectoryHandle('dota', { create: true });
  const put = async (dir, name) => (await dir.getFileHandle(name, { create: true }));
  for (const name of ['pak01_dir.vpk', 'pak01_000.vpk', 'notes.txt']) await put(dota, name);
  await put(await dota.getDirectoryHandle('maps', { create: true }), 'dota.vpk');
  out.fromRoot = keys(await grant.filesOf(await base.getDirectoryHandle('dota 2 beta')));
  out.fromGame = keys(await grant.filesOf(await (await base.getDirectoryHandle('dota 2 beta')).getDirectoryHandle('game')));
  out.fromDota = keys(await grant.filesOf(dota));
  out.wrong = keys(await grant.filesOf(base));
  await root.removeEntry('grant-test', { recursive: true });
  return out;
}"""


def check_grant_logic(page):
    got = page.evaluate(GRANT_LOGIC)
    packages = "maps/dota.vpk,pak01_000.vpk,pak01_dir.vpk"
    want = {
        "list": "maps/dota.vpk,pak01_000.vpk,pak01_dir.vpk",
        "none": "",
        "direct": "pak01_002.vpk,pak01_dir.vpk",
        "fromRoot": packages,
        "fromGame": packages,
        "fromDota": packages,
        "wrong": "",
    }
    assert got == want, f"folder grant logic: {got}"
    print("folder grant logic ok")


def import_smoke(args, install):
    folder = farm(install, HERE.parent.parent / "temp/smoke-install")
    server = serve()
    base = f"http://127.0.0.1:{server.server_address[1]}/"
    requests, console, failures = [], [], []

    def report():
        print("console:", *[c[:300] for c in console[-25:]], sep="\n  ", file=sys.stderr)
        print("errors:", failures, file=sys.stderr)

    with sync_playwright() as p:
        browser = p.chromium.launch(
            # a software WebGPU adapter (SwiftShader's Vulkan) for the client
            args=["--enable-unsafe-webgpu", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--enable-features=Vulkan"]
        )
        page = browser.new_context(viewport={"width": 1280, "height": 720}).new_page()
        page.on("request", lambda r: requests.append((r.method, r.url, r.post_data)))
        page.on("console", lambda m: console.append(m.text))
        page.on("pageerror", lambda e: failures.append(f"page error: {e}"))
        # The fallback picker (`<input webkitdirectory>`) is the path every browser has.
        page.add_init_script("delete window.showDirectoryPicker")

        page.goto(base)
        page.wait_for_selector("#grant:not([hidden])", timeout=20000)
        assert page.is_visible("#standins"), "stand-ins must always be offered"
        assert "never leave your computer" in page.inner_text("#grant"), "the privacy promise is on the page"

        check_grant_logic(page)

        # Cancelling an import leaves nothing behind and the question comes back.
        with page.expect_file_chooser() as chooser:
            page.click("#pick")
        chooser.value.set_files(str(folder))
        page.wait_for_selector("#progress:not([hidden])", timeout=120000)
        page.wait_for_function("() => document.getElementById('bar').value > 0", timeout=120000)
        page.click("#cancel")
        page.wait_for_selector("#grant:not([hidden])", timeout=60000)
        assert "cancelled" in page.inner_text("#status"), page.inner_text("#status")
        assert page.evaluate("async () => (await (await import('./cache.js')).readState()) === null")
        print("cancel leaves no copy")

        started = time.time()
        with page.expect_file_chooser() as chooser:
            page.click("#pick")
        chooser.value.set_files(str(folder))
        try:
            page.wait_for_selector("#progress:not([hidden]), #ready:not([hidden])", timeout=120000)
            page.wait_for_selector("#ready:not([hidden])", timeout=300000)
        except Exception:
            print("status:", page.inner_text("#status"), file=sys.stderr)
            report()
            raise
        print(f"ready after {time.time() - started:.0f} s:", page.inner_text("#readyText"))
        state = page.evaluate(
            """async () => { const c = await import('./cache.js'); const s = await c.readState();
               return s && {stamp: s.stamp, files: s.files.size, small: s.small.length,
                            head: await c.readCache('models/hero/nevermore_head/nevermore_head.glb').then(b => b && b.length)}; }"""
        )
        print("cache:", state)
        assert state and state["files"] > 800 and state["head"] and state["head"] > 1000, state

        page.reload()
        page.wait_for_selector("#ready:not([hidden])", timeout=20000)
        print("after reload: ready without picking again")

        # A copy another dbota made (its stamp of another asset fingerprint) is not played: the
        # page holds at the question, publishes nothing to the client, and makes it again.
        page.evaluate(
            """async () => { const c = await import('./cache.js');
               const h = await (await c.dir([], false)).getFileHandle('stamp.txt');
               const w = await h.createWritable(); await w.write('v1/rev1/0000000000000000/1/2'); await w.close(); }"""
        )
        page.reload()
        page.wait_for_selector("#stale:not([hidden])", timeout=20000)
        assert page.is_hidden("#ready") and page.is_hidden("#play"), "no Play on a stale copy"
        assert page.evaluate("() => globalThis.dbota_cache === undefined"), "a stale copy is not published"
        with page.expect_file_chooser() as chooser:
            page.click("#reimport")
        chooser.value.set_files(str(folder))
        page.wait_for_selector("#ready:not([hidden])", timeout=300000)
        print("a stale copy holds the page and is imported again:", page.inner_text("#readyText"))

        if not args.no_game:
            page.click("#play")
            try:
                page.wait_for_function(
                    "() => globalThis.dbota_cache && globalThis.dbota_cache.reads && globalThis.dbota_cache.reads.count > 20",
                    timeout=180000,
                )
                # The map: its glTF, its geometry and its textures.
                page.wait_for_function(
                    """() => { const s = globalThis.dbota_cache.reads.seen;
                       return s.has('map/mid/mid.gltf') && s.has('map/mid/mid.bin')
                         && [...s].some((p) => p.startsWith('map/mid/') && p.endsWith('.ktx2')); }""",
                    timeout=300000,
                )
            except Exception:
                print("status:", page.inner_text("#status"), file=sys.stderr)
                report()
                page.screenshot(path=str(HERE.parent.parent / "temp/smoke-fail.png"))
                raise
            reads = page.evaluate("() => globalThis.dbota_cache.reads")
            seen = page.evaluate("() => globalThis.dbota_cache.reads.seen.size")
            print("the client read", reads["count"], "times,", seen, "cache files, last:", reads["last"][-3:])
            assert any(p.endswith((".png", ".ktx2", ".mp3", ".wav")) for p in reads["last"]), reads
            # The map's chunks are merged and uploaded frame by frame after they are read.
            page.wait_for_timeout(args.settle * 1000)
            # A software GPU draws the whole map slowly: a frame may take minutes.
            page.screenshot(path=str(HERE.parent.parent / "temp/smoke.png"), timeout=600000)

        bad = [(m, u) for m, u, d in requests if not u.startswith(base) and not u.startswith("blob:") and not u.startswith("data:")]
        posts = [(m, u) for m, u, d in requests if m != "GET" or d]
        assert not bad, f"requests outside the page's own origin: {bad}"
        assert not posts, f"the page sent data: {posts}"
        errs = [c for c in console if "panicked" in c or "RuntimeError" in c]
        assert not errs, errs
        # The models' and the map's textures are block-compressed KTX2 files: drawn as they are
        # where the GPU samples BC, else decoded as they load; either way none fails.
        decoded = any("samples no BC textures" in c for c in console)
        print("textures:", "decoded on load (no BC on this GPU)" if decoded else "block-compressed on the GPU")
        ktx2 = [c for c in console if ".ktx2" in c and ("rror" in c or "ailed" in c)]
        assert not ktx2, ktx2[:5]

        if not args.keep:
            page.goto(base)
            page.wait_for_selector("#ready:not([hidden])", timeout=20000)
            page.click("#remove")
            page.wait_for_selector("#grant:not([hidden])", timeout=20000)
            gone = page.evaluate("async () => { const c = await import('./cache.js'); return (await c.readState()) === null; }")
            assert gone, "the copy is deleted"
            print("deleted")
        browser.close()
    server.shutdown()
    if failures:
        sys.exit("\n".join(failures))
    print("smoke test passed:", len(requests), "requests, all to", base)


BROWSER_ARGS = [
    # a software WebGPU adapter (SwiftShader's Vulkan) for the client
    "--enable-unsafe-webgpu", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--enable-features=Vulkan",
    # The test server and the page are both on this machine, which a recent Chromium treats as a
    # local-network request needing a permission nobody is there to give.
    "--disable-features=LocalNetworkAccessChecks,BlockInsecurePrivateNetworkRequests",
]
# postcard's tag of `ServerMsg::Compressed` (the registry is append-only): every frame of a
# compressed link that is not chat carries it, after the frame's four bytes of length.
COMPRESSED_TAG = 14
# The local lobby page, where a game is joined by address, is the first page without a service;
# its address field is the fourth stop of Tab (port, players, CREATE, CREATE FOR THE LAN), and Enter
# there joins.
TABS_TO_ADDRESS = 4


def wait_console(page, console, pattern, timeout, what):
    """Waits for a console line matching `pattern`; returns its match."""
    seen = re.compile(pattern)
    deadline = time.time() + timeout
    while time.time() < deadline:
        for line in list(console):
            if m := seen.search(line):
                return m
        page.wait_for_timeout(500)
    print("console:", *[c[:300] for c in console[-25:]], sep="\n  ", file=sys.stderr)
    sys.exit(f"timed out waiting for {what}")


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
        page.goto(base + "?assets=none")
        wait_console(page, console, r"match known: tick \d+", 600, "the client to know the match")
        heard = wait_console(page, console, r"chat: (\d+) lines heard", 120, "the chat lines of the replay")
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


def check_fullscreen_page(directory):
    """Windowed fullscreen in Settings puts the canvas fullscreen (inside the click's gesture), the
    page leaving it (Esc) turns the setting back, and F11 is the same fullscreen. Headless Chromium
    keeps its viewport, so the canvas filling a bigger screen is not seen here."""
    with sync_playwright() as p:
        server, base, browser, page, console, problems = opened(p, directory)
        page.goto(base + "?assets=none")
        wait_console(page, console, r"match known: tick \d+", 600, "the client to know the match")
        page.wait_for_timeout(4000)
        fullscreen = lambda: page.evaluate("!!document.fullscreenElement")
        shot = lambda name: page.screenshot(path=str(HERE.parent.parent / f"temp/smoke-{name}.png"), timeout=600000)
        # The Video tab is open (--settings video): the display mode's list, its second option.
        page.mouse.click(775, 115)
        page.wait_for_timeout(1500)
        page.mouse.click(775, 164)
        page.wait_for_timeout(2500)
        assert fullscreen(), "picking windowed fullscreen puts the canvas fullscreen"
        shot("fullscreen-picked")
        page.evaluate("document.exitFullscreen()")
        page.wait_for_timeout(2500)
        assert not fullscreen()
        shot("fullscreen-left")  # the setting shows Windowed again
        page.keyboard.press("F11")
        page.wait_for_timeout(2500)
        assert fullscreen(), "F11 is the canvas's fullscreen"
        page.keyboard.press("F11")
        page.wait_for_timeout(2500)
        assert not fullscreen()
        assert not problems, problems
        print("fullscreen: the setting and F11 enter it, leaving it works")
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
        page.goto(base + "?assets=none")
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
        page.goto(base + "?assets=none")
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


def free_port():
    import socket

    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def check_live_page(directory, server_bin):
    """The page joins a bota_server over a WebSocket and asks for compression; the snapshots it is
    sent are compressed and the match plays on in it."""
    tcp, ws = free_port(), free_port()
    game = subprocess.Popen(
        [server_bin, "--port", str(tcp), "--ws-port", str(ws), "--mode", "realtime", "--map", "2",
         "--players", "2", "--dummy-seat", "--pick-secs", "1", "--tactical-secs", "0", "--seed", "7"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    try:
        with sync_playwright() as p:
            server, base, browser, page, console, problems = opened(p, directory)
            sent, received = [], []

            def on_socket(sock):
                sock.on("framesent", lambda f: sent.append(f if isinstance(f, bytes) else f.encode()))
                sock.on("framereceived", lambda f: received.append(f if isinstance(f, bytes) else f.encode()))

            page.on("websocket", on_socket)
            # The one socket of this run goes into the page's policy, as a page with networking has.
            def with_socket(route):
                response = route.fetch()
                body = response.text().replace("connect-src 'self'", f"connect-src 'self' ws://127.0.0.1:{ws}")
                route.fulfill(response=response, body=body)

            page.route(re.compile(r".*/(index\.html)?(\?.*)?$"), with_socket)
            menu = f"direct = ws://127.0.0.1:{ws}\nname = smoke\nplain_ok = 127.0.0.1\n"
            page.add_init_script("localStorage.setItem('dbota.menu', " + json.dumps(menu) + ")")
            page.goto(base + "?assets=none")
            page.wait_for_timeout(15000)
            for _ in range(TABS_TO_ADDRESS):
                page.keyboard.press("Tab")
                page.wait_for_timeout(1500)
            page.keyboard.press("Enter")
            wait_console(page, console, r"match known: tick \d+", 240, "the live match")
            page.wait_for_timeout(8000)
            # A software GPU draws a frame in seconds: each step gets one to itself.
            page.keyboard.press("Enter")
            page.wait_for_timeout(3000)
            page.keyboard.type("hello from the browser")
            page.wait_for_timeout(3000)
            page.keyboard.press("Enter")
            wait_console(page, console, r"chat: [1-9]\d* lines heard", 60, "the chat line to come back")
            page.screenshot(path=str(HERE.parent.parent / "temp/smoke-live.png"), timeout=600000)
            assert sent and received, "the page talked to the server"
            packed = [f for f in received if f[4:5] == bytes([COMPRESSED_TAG])]
            assert len(packed) >= 20, f"compressed parts received: {len(packed)} of {len(received)} frames"
            assert any(f[4:5] != bytes([COMPRESSED_TAG]) for f in received), "the hello's answers came plain"
            bad = [c for c in console if "panicked" in c or "RuntimeError" in c]
            assert not bad and not problems, (bad, problems)
            print(f"live: match known, {len(packed)} of {len(received)} server frames were compressed parts, "
                  f"chat line heard, {len(sent)} frames sent")
            browser.close()
        server.shutdown()
    finally:
        game.terminate()
        game.wait(timeout=10)


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


def check_startup_page(directory):
    """The page picks the client the browser can draw with, and a start that fails says so on the
    page: no graphics, a renderer that cannot be made (offering the WebGL2 client), no memory."""
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

    for query, api in (("?assets=none", "webgpu"), ("?assets=none&gpu=webgl2", "webgl2")):
        for page, console, problems in run(query):
            wait_console(page, console, "dbota: drawing with " + api, 60, f"the {api} client")
            wait_console(page, console, "dbota: running", 600, f"the {api} client's first frame")
            assert page.is_hidden("#gate") and not problems, problems
            print(f"{api}: the client runs")
    for page, console, problems in run("?assets=none", NO_GRAPHICS):
        failed(page, "does not let the page use the graphics card")
        assert not any("drawing with" in c for c in console), "no client is loaded without graphics"
        print("no graphics: said on the page")
    for page, console, problems in run("?assets=none", NO_WEBGPU_CANVAS):
        failed(page, "could not start its graphics")
        assert "wgpu surface" in page.text_content("#failDetail"), "the panic is in the details"
        page.click("#otherApi")
        wait_console(page, console, "dbota: drawing with webgl2", 60, "the WebGL2 client offered instead")
        wait_console(page, console, "dbota: running", 600, "the WebGL2 client's first frame")
        assert "gpu=webgl2" in page.url, page.url
        print("renderer not made: said on the page, the WebGL2 client runs instead")
    # The client's memory capped at 96 MiB: the stand-ins' menu needs more.
    for page, console, problems in run("?assets=none", args=["--js-flags=--wasm-max-mem-pages=1536"]):
        failed(page, "ran out of memory")
        print("out of memory: said on the page")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--install")
    ap.add_argument("--no-game", action="store_true")
    ap.add_argument("--keep", action="store_true", help="leave the imported copy in place")
    ap.add_argument("--settle", type=int, default=5, help="seconds the client draws before the screenshot")
    ap.add_argument("--replay-page", type=pathlib.Path, help="a build with a packed replay built in")
    ap.add_argument("--fullscreen-page", type=pathlib.Path, help="a replay build opened on the Video settings")
    ap.add_argument("--live-page", type=pathlib.Path, help="a build without a replay (the main menu)")
    ap.add_argument("--server", help="a bota_server built with the websocket feature, for --live-page")
    ap.add_argument("--startup-page", type=pathlib.Path, help="any build: the client picked, failures shown")
    ap.add_argument("--keys-page", type=pathlib.Path, help="any page, even web/ unbuilt: whose keys are whose")
    ap.add_argument("--pixels-page", type=pathlib.Path, help="a replay build: the canvas drawn at its device pixels")
    ap.add_argument("--only-wire", action="store_true", help="skip the import test even where an install is found")
    args = ap.parse_args()
    install = None if args.only_wire else find_install(args.install)
    if install:
        import_smoke(args, install)
    else:
        print("skipped: the import test needs a Dota 2 install (DBOTA_DOTA_PATH or --install)")
    if args.startup_page:
        check_startup_page(args.startup_page)
    if args.keys_page:
        check_browser_keys(args.keys_page)
    if args.replay_page:
        check_replay_page(args.replay_page)
    if args.fullscreen_page:
        check_fullscreen_page(args.fullscreen_page)
    if args.pixels_page:
        check_canvas_pixels(args.pixels_page)
    if args.live_page:
        if not args.server:
            sys.exit("--live-page needs --server")
        check_live_page(args.live_page, args.server)


if __name__ == "__main__":
    main()
