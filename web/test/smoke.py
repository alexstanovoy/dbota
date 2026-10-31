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

One more check needs a prepared asset cache, not an install:

    dbota import          # a native cache of this build (DBOTA_ASSETS=... to keep it private)
    dbota export-cache temp/cache.zip
    DBOTA_WEB_REPLAY=$PWD/match.brp PROFILE=release OUT=temp/page web/build.sh   # a page that plays a replay: the map is drawn
    PYTHONPATH=temp/pwlib python3 web/test/smoke.py --prepared-page temp/page \\
        --cache-zip temp/cache.zip --cache-folder "$DBOTA_ASSETS"

* `--prepared-page` (a build with a replay built in, so the client draws the map): zips the page must refuse (another build's stamp, a path that climbs out, an
  index that does not match) are refused with their reasons and copy nothing; the real zip is
  copied into private storage (files, stamp and bytes as the zip has them), kept over a reload and
  played until the map is read and drawn; the cache folder is then loaded the same way. The page
  requests nothing outside its own origin.

Five more checks need no install, only a native import of this build (`dbota import`, DBOTA_ASSETS
names it) that the pages play on through `?cache=http`, served under /cache beside the page:

    DBOTA_WEB_REPLAY=$PWD/chat.brp DBOTA_WEB_ARGS="--replay built-in --pov 0 --seek 4600" \
        PROFILE=release OUT=temp/web-replay web/build.sh
    PROFILE=release OUT=temp/web-live web/build.sh
    CARGO_TARGET_DIR=temp/bota-target cargo build --release --manifest-path ../bota/Cargo.toml \
        -p bota_server --features websocket
    PYTHONPATH=temp/pwlib python3 web/test/smoke.py --replay-page temp/web-replay \
        --replays-page temp/web-live --server temp/bota-target/release/bota_server --radovid RADOVID \
        --startup-page temp/web-live

* `--served-page` (any build): with `?cache=http` the page fetches the served cache's index.txt and
  publishes it to the client (without one, the cache is taken whole).
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
* `--cursor-page` (a replay build, e.g. the `--replay-page` one): in a match a press on the canvas
  locks the pointer, the game's cursor moves by the mouse's motion and stays inside the canvas, a
  press there is the game's at that place (the dashboard button opens the settings, which let the
  pointer go), and closing them or F11 after an Esc takes it again.
* `--boot-page` (any build): the page never spins for ever: a part that does not load, code the
  browser cannot run, a page that never arrives, a storage that hangs or is missing, a game download
  that stalls or is refused each end on a screen that says the stage, while a slow download that keeps
  moving is waited for and the game starts (Chromium, and Firefox where there is one).
* `--fullscreen-page`: a replay build made with `DBOTA_WEB_ARGS="--replay built-in --pov 0 --settings video"`;
  the display mode and F11 put the canvas fullscreen and leaving it turns the setting back.
"""
import argparse
import functools
import hashlib
import http.server
import json
import os
import pathlib
import re
import shutil
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


# The asset cache the pages that need no import play on (`?cache=http`): a native import of this
# build, served under /cache beside the page (DBOTA_ASSETS, else ~/.cache/dbota/v<N>).
def cache_folder():
    given = os.environ.get("DBOTA_ASSETS")
    if given:
        return pathlib.Path(given)
    version = (HERE.parent.parent / "assets-manifest/version").read_text().strip()
    return pathlib.Path("~/.cache/dbota").expanduser() / f"v{version}"


class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def translate_path(self, path):
        if path.split("?")[0].startswith("/cache/"):
            relative = pathlib.PurePosixPath(path.split("?")[0][len("/cache/"):])
            if ".." in relative.parts:
                return "/nonexistent"
            return str(cache_folder().joinpath(*relative.parts))
        return super().translate_path(path)


def serve(directory=OUT, handler_class=Quiet):
    handler = functools.partial(handler_class, directory=str(directory))
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


ICONS_SUMS = """async (paths) => {
  const c = await import('./cache.js');
  const hex = (b) => [...new Uint8Array(b)].map((v) => v.toString(16).padStart(2, '0')).join('');
  const out = {};
  for (const p of paths) out[p] = hex(await crypto.subtle.digest('SHA-256', await c.readCache(p)));
  return out;
}"""


def check_icons_as_native(page, reference):
    """The importer is the same code in the browser's wasm (32-bit) and on the desktop, and must
    make the same files: a block decoder that kept 32 of a BC3 block's 48 alpha index bits in a
    `usize` drew every icon jagged and dashed in the browser only. Every icon the browser made is
    compared, byte for byte, with the one a desktop `dbota import` made (`--reference-cache`, or
    DBOTA_ASSETS); without one the check is skipped, with a message."""
    reference = reference or os.environ.get("DBOTA_ASSETS")
    root = pathlib.Path(reference).expanduser() if reference else None
    if not root or not (root / "index.txt").is_file():
        print("skipped: the icons are compared with a desktop import (--reference-cache DIR or DBOTA_ASSETS)")
        return
    paths = sorted(str(p.relative_to(root)) for p in (root / "icons").rglob("*.png"))
    assert paths, f"no icons in {root}"
    got = page.evaluate(ICONS_SUMS, paths)
    wrong = [p for p in paths if got[p] != hashlib.sha256((root / p).read_bytes()).hexdigest()]
    assert not wrong, f"{len(wrong)} of {len(paths)} icons differ from the desktop's import, e.g. {wrong[:5]}"
    print(f"icons: all {len(paths)} the same as the desktop's import")
def play_scene(page, report, settle):
    """Plays on the copy in private storage: the client reads the map and the scene is drawn."""
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
    page.wait_for_timeout(settle * 1000)
    # A software GPU draws the whole map slowly: a frame may take minutes.
    page.screenshot(path=str(HERE.parent.parent / "temp/smoke.png"), timeout=600000)


def import_smoke(args, install):
    folder = farm(install, HERE.parent.parent / "temp/smoke-install")
    server = serve()
    base = f"http://127.0.0.1:{server.server_address[1]}/"
    requests, console, failures = [], [], []

    def report():
        print("console:", *[c[:300] for c in [c for c in console if "CommandQueue" not in c and not c.startswith("log.")][-25:]], sep="\n  ", file=sys.stderr)
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
        assert page.is_hidden("#standins") and "stand-in" not in page.inner_text("main"), "no way to play without assets is offered"
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

        # A write the browser refuses ends the import: the copy is unfinished (no index, no
        # stamp), the page says so, and the question comes back. The worker is served with its
        # 300th write failing.
        def failing_worker(route):
            body = route.fetch().text()
            body = body.replace(
                "async function writeFile(path, bytes) {",
                'let writes = 0;\nasync function writeFile(path, bytes) {\n'
                '  if (++writes === 300) throw new Error("injected: the write failed");',
                1,
            )
            assert "injected" in body, "the worker's writeFile changed"
            route.fulfill(body=body, content_type="text/javascript")

        page.route("**/import_worker.js", failing_worker)
        with page.expect_file_chooser() as chooser:
            page.click("#pick")
        chooser.value.set_files(str(folder))
        page.wait_for_selector("#grant:not([hidden])", timeout=300000)
        assert "injected" in page.inner_text("#status"), page.inner_text("#status")
        assert page.evaluate("async () => (await (await import('./cache.js')).readState()) === null")
        assert page.evaluate("() => globalThis.dbota_cache === undefined")
        page.unroute("**/import_worker.js")
        print("a failed write ends the import, unfinished")

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

        check_icons_as_native(page, args.reference_cache)

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
            play_scene(page, report, args.settle)

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
    try:
        page.screenshot(path=str(HERE.parent.parent / "temp/smoke-timeout.png"), timeout=60000)
    except Exception as e:  # the page may be gone or too busy to draw
        print("no screenshot:", e, file=sys.stderr)
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


def check_fullscreen_page(directory):
    """Windowed fullscreen in Settings puts the canvas fullscreen (inside the click's gesture), the
    page leaving it (Esc) turns the setting back, and F11 is the same fullscreen. Headless Chromium
    keeps its viewport, so the canvas filling a bigger screen is not seen here."""
    with sync_playwright() as p:
        server, base, browser, page, console, problems = opened(p, directory)
        page.goto(base + "?cache=http")
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
        far = 10 * max(size[0], size[1]) * size[2]
        mouse("pointermove", far, far)
        last = [size[0] - 1 / size[2], size[1] - 1 / size[2]]
        got = page.evaluate("handed[handed.length - 1]")
        assert got == last, f"the cursor stays inside the canvas: {got}, not {last}"
        mouse("pointermove", -far, -far)
        assert page.evaluate("handed[handed.length - 1]") == [0, 0], "the cursor stops at the top-left corner"
        print("cursor: locked by a press, held inside the canvas")
        # The dashboard button, the top-left corner's first: in a replay page it opens the settings.
        mouse("pointermove", DASHBOARD[0] * size[2], DASHBOARD[1] * size[2])
        # A press carries the pointer's travel too in Chromium (large when the lock re-centres it);
        # only a move is the cursor's motion, so the press stays where the cursor is.
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


def check_served_cache(directory):
    """A cache a developer serves next to the page (`?cache=http`) publishes its index.txt, as
    the private storage's cache does, so the client knows which files are there (the icons of
    every item and ability among them); one without an index is still taken whole."""
    index = "dbota-cache 1 rev 13\nfile 100 icons/items/blink.png\nfile 90 icons/abilities/nevermore_requiem.png small\nfile 5 text/labels.json small\n"

    class Served(Quiet):
        text = None

        def do_GET(self):
            if self.path == "/cache/index.txt" and self.text is not None:
                body = self.text.encode()
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
                return
            super().do_GET()

    for text in (index, None):
        Served.text = text
        with sync_playwright() as p:
            server = serve(directory, Served)
            base = f"http://127.0.0.1:{server.server_address[1]}/"
            browser = p.chromium.launch(args=BROWSER_ARGS)
            page = browser.new_context(viewport={"width": 1280, "height": 720}).new_page()
            page.goto(base + "?cache=http")
            page.wait_for_function("() => globalThis.dbota_cache && globalThis.dbota_cache.mode === 'http'", timeout=60000)
            got = page.evaluate(
                "() => { const f = globalThis.dbota_cache.files; return f ? [...f.entries()] : null; }"
            )
            browser.close()
            server.shutdown()
        if text is None:
            assert got is None, f"an index from a server without one: {got}"
            print("a served cache without an index is taken whole")
        else:
            want = [["icons/items/blink.png", 100], ["icons/abilities/nevermore_requiem.png", 90], ["text/labels.json", 5]]
            assert got == want, got
            print("a served cache publishes its index")


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

    for query, api in (("?cache=http", "webgpu"), ("?cache=http&gpu=webgl2", "webgl2")):
        for page, console, problems in run(query):
            wait_console(page, console, "dbota: drawing with " + api, 60, f"the {api} client")
            wait_console(page, console, "dbota: running", 600, f"the {api} client's first frame")
            assert page.is_hidden("#gate") and not problems, problems
            print(f"{api}: the client runs")
    for page, console, problems in run("?cache=http", NO_GRAPHICS):
        failed(page, "does not let the page use the graphics card")
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


def check_boot_page(directory):
    """The page never spins for ever: each way the start can go wrong ends on the failure screen or the
    asset screen, saying which stage failed. A part of the page that does not load, a page that never
    arrives, code the browser cannot run, a browser storage that does not answer or is missing, a game
    download that stalls or is refused. Chromium for all, Firefox (where one is installed) for the
    stages that need no graphics."""
    def screen(page, selector, title=None, timeout=180000):
        page.wait_for_selector(selector, state="visible", timeout=timeout)
        if title:
            got = page.inner_text("#failTitle")
            assert title in got, got
        assert page.is_hidden("#booting") and page.is_hidden("#starting"), "no spinner is left over the screen"

    hang = "() => new Promise(() => {})"
    no_storage = "Object.defineProperty(navigator, 'storage', {value: undefined});"
    hung_storage = f"navigator.storage.getDirectory = {hang};"

    def case(engine, name, query, init=None, routes=(), then=None):
        with sync_playwright() as p:
            server = serve(directory)
            try:
                browser = getattr(p, engine).launch(args=BROWSER_ARGS if engine == "chromium" else [])
            except Exception as e:
                print(f"boot [{engine}]: skipped, no browser ({str(e)[:60]})")
                return
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

        def asks_for_the_folder(page, said):
            screen(page, "#grant")
            assert said in page.inner_text("#status"), page.inner_text("#status")

        case(engine, "a storage that does not answer leaves the asset screen", "", init=hung_storage,
             then=lambda page: asks_for_the_folder(page, "did not answer"))
        case(engine, "a browser without storage leaves the asset screen", "", init=no_storage,
             then=lambda page: asks_for_the_folder(page, "no private storage"))
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


def assert_in(text, part):
    assert part in text, f"{part!r} not in {text!r}"


# Where the main menu's buttons are in a 1280x720 page (calibrated by `--shots`): the page is
# drawn at the viewport's size with the interface at its default scale, so they stay put.
CLICK_LOBBIES = (329, 22)
CLICK_REPLAYS = (505, 22)
CLICK_START = (310, 567)


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


def radovid_config(directory, page_dir, server_bin, port, game_port, cert, key):
    """A radovid for one practice ruleset whose games keep their replay and end quickly once the
    player is gone, serving `page_dir` on its own origin."""
    fingerprint = subprocess.run([server_bin, "--wire-fingerprint"], capture_output=True, text=True).stdout.strip()
    return f"""
listen = "127.0.0.1:{port}"
database = "{directory}/radovid.db"
tls_cert = "{cert}"
tls_key = "{key}"
game_ports = "{game_port}-{game_port}"
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
id = "practice"
name = "Practice"
game = "bota"
game_fingerprint = "{fingerprint}"
teams = [1, 1]
rated = false
lobby_min_players = 1
max_duration_secs = 600
[ruleset.launcher]
kind = "local"
binary = "{server_bin}"
public_url = "wss://127.0.0.1:{{port}}"
version_probe = ["--wire-fingerprint"]
args = ["--map", "2", "--match-id", "{{match_id}}", "--bind", "127.0.0.1", "--port", "0", "--ws-port", "{{port}}",
        "--tls-cert", "{cert}", "--tls-key", "{key}", "--heroes", "1", "--pick-secs", "1", "--tactical-secs", "0",
        "--start-after-secs", "120", "--abandon-unjoined", "--empty-after-secs", "5"]
env = {{ BOTA_SEATS = "{{seats}}" }}
artifact = "{directory}/replays/replay-{{match_id}}.brp"
artifact_args = ["--replay", "{{artifact}}"]
"""


OPFS_REPLAYS = """async () => {
  const root = await navigator.storage.getDirectory();
  let dir;
  try { dir = await root.getDirectoryHandle('dbota-replays'); } catch (e) { return []; }
  const out = [];
  for await (const [name, handle] of dir.entries()) {
    if (handle.kind === 'file') out.push([name, (await handle.getFile()).size]);
  }
  return out;
}"""


def check_replays_page(directory, server_bin, radovid_bin, shots=False):
    """A match through a real radovid is kept in the browser's private storage: the page enters a
    practice match alone and is closed (the player gone, the game server ends and the service
    records it); the next visit signs in with the kept session, fetches the replay it was away
    for and stores it in OPFS. The REPLAYS tab lists it and plays it; DOWNLOAD .BRP hands over the
    same bytes, OPEN A .BRP FILE plays that file, a new visit plays the stored one with WATCH and DELETE
    removes it."""
    work = HERE.parent.parent / "temp/smoke-replays"
    work.mkdir(parents=True, exist_ok=True)
    cert, key = work / "cert.pem", work / "key.pem"
    subprocess.run([radovid_bin, "dev-cert", str(work)], check=True, capture_output=True)
    for stale in [*work.glob("radovid.db*"), *(work / "replays").glob("*")]:
        stale.unlink()
    port, game = free_port(), free_port()
    # radovid serves the page from a folder of its own (it follows no link out of it).
    page_dir = work / "page"
    shutil.rmtree(page_dir, ignore_errors=True)
    shutil.copytree(directory, page_dir, symlinks=False)
    # The page plays on a native cache (`?cache=http`): hard links to its files, nothing copied.
    shutil.copytree(cache_folder(), page_dir / "cache", copy_function=os.link)
    (work / "radovid.toml").write_text(radovid_config(work, page_dir, server_bin, port, game, cert, key))
    service = subprocess.Popen([radovid_bin, "serve", "--config", str(work / "radovid.toml")],
                               stdout=subprocess.DEVNULL, stderr=open(work / "radovid.log", "w"))
    try:
        wait_port(port)
        with sync_playwright() as p:
            browser = p.chromium.launch(args=[*BROWSER_ARGS, "--ignore-certificate-errors"])
            context = browser.new_context(viewport={"width": 1280, "height": 720}, ignore_https_errors=True,
                                          accept_downloads=True)
            page = context.new_page()
            console, problems = [], []
            page.on("console", lambda m: console.append(m.text))
            page.on("pageerror", lambda e: problems.append(f"page error: {e}"))
            url = f"https://127.0.0.1:{port}/?cache=http"

            def shot(name):
                if shots:
                    page.screenshot(path=str(work / f"{name}.png"), timeout=600000)

            def step(wait=4000):
                page.wait_for_timeout(wait)

            # First visit: register, make a practice lobby, START NOW; then close the page.
            page.goto(url)
            wait_console(page, console, r"replays: kept in the browser's opfs storage", 600, "the replay storage")
            step(15000)
            shot("1-login")
            for _ in range(3):  # login, password, SIGN IN, then REGISTER
                page.keyboard.press("Tab")
            page.keyboard.press("Enter")
            step()
            shot("2-register")
            page.keyboard.type("smoker")
            page.keyboard.press("Tab")
            page.keyboard.type("correct horse battery")
            page.keyboard.press("Tab")
            page.keyboard.type("correct horse battery")
            page.keyboard.press("Enter")
            step(8000)
            shot("3-home")
            page.mouse.click(*CLICK_LOBBIES)
            step()
            shot("4-lobbies")
            page.keyboard.type("alone")
            page.keyboard.press("Tab")
            page.keyboard.press("Tab")
            page.keyboard.press("Enter")
            step(6000)
            shot("5-lobby")
            page.mouse.click(*CLICK_START)
            wait_console(page, console, r"match known: tick \d+", 600, "the practice match")
            step(5000)
            shot("6-game")
            page.close()

            # The game server ends once its only seat has stayed empty; radovid records the match.
            deadline = time.time() + 300
            while time.time() < deadline and not list((work / "replays").glob("replay-*.brp")):
                time.sleep(1)
            files = sorted((work / "replays").glob("replay-*.brp"))
            assert files, "the game server wrote a replay for radovid to keep"
            time.sleep(15)

            # The next visit: signed in again by the kept session, the replay is fetched and stored.
            page = context.new_page()
            console.clear()
            page.on("console", lambda m: console.append(m.text))
            page.on("pageerror", lambda e: problems.append(f"page error: {e}"))
            page.goto(url)
            deadline = time.time() + 600
            stored = []
            while time.time() < deadline and not any(n.endswith(".brp") for n, _ in stored):
                page.wait_for_timeout(3000)
                stored = page.evaluate(OPFS_REPLAYS)
            replay = [(n, size) for n, size in stored if n.endswith(".brp")]
            assert len(replay) == 1, f"the replay is stored in OPFS: {stored}"
            name, size = replay[0]
            assert size == files[0].stat().st_size, "the stored replay is the service's file"
            assert any(n == "index.txt" for n, _ in stored), "the index is beside it"
            shot("7a-stored")
            wait_console(page, console, r"replays: 1 kept", 300, "the replay listed")
            print(f"stored: {name} ({size} bytes) in OPFS")

            # The REPLAYS tab lists it. DOWNLOAD .BRP hands over the stored bytes.
            page.mouse.click(*CLICK_REPLAYS)
            step()
            shot("7-replays")
            page.keyboard.press("Tab")  # the keyboard starts on OPEN A .BRP FILE: WATCH,
            page.keyboard.press("Tab")  # then DOWNLOAD .BRP
            with page.expect_download(timeout=120000) as download:
                page.keyboard.press("Enter")
            saved = work / "downloaded.brp"
            download.value.save_as(str(saved))
            assert download.value.suggested_filename == name, download.value.suggested_filename
            assert saved.read_bytes() == files[0].read_bytes(), "the download is the replay the service kept"
            assert saved.read_bytes()[:4] == b"\x28\xb5\x2f\xfd", "and a Zstandard stream, as a desktop reads"
            print(f"download: {saved.stat().st_size} bytes, the same as the service's file")

            # OPEN A .BRP FILE plays a file from the player's disk (one from a desktop would do).
            page.keyboard.press("Tab")  # DELETE
            page.keyboard.press("Tab")  # OPEN A .BRP FILE
            with page.expect_file_chooser(timeout=120000) as chooser:
                page.keyboard.press("Enter")
            chooser.value.set_files(str(saved))
            wait_console(page, console, r"match known: tick \d+", 600, "the opened file to play")
            print("open from file: the downloaded replay plays")
            page.close()

            # A new visit: WATCH plays the stored replay, DELETE removes it.
            page = context.new_page()
            console.clear()
            page.on("console", lambda m: console.append(m.text))
            page.on("pageerror", lambda e: problems.append(f"page error: {e}"))
            page.goto(url)
            wait_console(page, console, r"replays: 1 kept", 600, "the replay listed again")
            step(10000)
            page.mouse.click(*CLICK_REPLAYS)
            step()
            page.keyboard.press("Tab")
            page.keyboard.press("Enter")
            step(20000)
            shot("8-watch")
            wait_console(page, console, r"match known: tick \d+", 600, "the stored replay to play")
            print("watch: the stored replay plays")
            bad = [c for c in console if "cannot be read" in c or "panicked" in c or "RuntimeError" in c]
            assert not bad and not problems, (bad, problems)
            page.close()
            page = context.new_page()
            page.goto(url)
            step(15000)
            page.mouse.click(*CLICK_REPLAYS)
            step()
            for _ in range(3):  # WATCH, DOWNLOAD .BRP, DELETE
                page.keyboard.press("Tab")
            page.keyboard.press("Enter")
            deadline = time.time() + 120
            left = page.evaluate(OPFS_REPLAYS)
            while time.time() < deadline and any(n.endswith(".brp") for n, _ in left):
                page.wait_for_timeout(2000)
                left = page.evaluate(OPFS_REPLAYS)
            assert not any(n.endswith(".brp") for n, _ in left), f"DELETE removed the replay: {left}"
            print("delete: the replay is gone from OPFS")
            browser.close()
    finally:
        service.terminate()
        service.wait(timeout=20)
    print("replays: kept in OPFS from a match through radovid, listed and played")
def refusing_zips(good_zip, scratch):
    """Zips the page must refuse: (name, path, the words of the reason). Their stamp, index and
    files are those of the real prepared cache but for the one thing wrong."""
    import zipfile

    scratch.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(good_zip) as good:
        stamp = good.read("stamp.txt")
        header = good.read("index.txt").decode().split("\n", 1)[0] + "\n"
        some = next(n for n in good.namelist() if n.startswith("hud/"))
        data = good.read(some)
    # A small cache of one file: valid but for the one thing wrong in each.
    index = header + f"file {len(data)} {some}\n"

    def make(name, members):
        path = scratch / name
        with zipfile.ZipFile(path, "w") as z:
            for member, body in members:
                z.writestr(member, body)
        return path

    # The paths of an archive are written as they are, including ones no tool would make.
    climbing = index + "file 16 ../escape.png\n"
    size = len(data)
    return [
        ("another build", make("stale.zip", [("stamp.txt", "v1/rev1/0000000000000000/1/2"), ("index.txt", index), (some, data)]),
         "is for assets v1/rev1/0000000000000000, but this dbota needs assets"),
        ("a path that climbs out", make("climb.zip", [("stamp.txt", stamp), ("index.txt", climbing), (some, data), ("../escape.png", b"x" * 16)]),
         "holds a file it must not: ../escape.png"),
        ("a file the index does not list", make("unlisted.zip", [("stamp.txt", stamp), ("index.txt", index), (some, data), ("hud/unlisted.png", b"x")]),
         "hud/unlisted.png is in it, but index.txt does not list it"),
        ("a size the index does not say", make("size.zip", [("stamp.txt", stamp), ("index.txt", index), (some, data + b"x")]),
         f"{some} is {size + 1} bytes, but index.txt says {size}"),
    ]


def check_directory_picker(page):
    """Where the browser has `showDirectoryPicker` (Chromium) the folder button uses it: a stub
    hands the page a folder in private storage (a cache of another build), which is walked and
    taken to the worker, and refused there with its reason."""
    page.evaluate(
        """async () => {
          const root = await navigator.storage.getDirectory();
          const top = await root.getDirectoryHandle('picked-cache', { create: true });
          const put = async (dir, name, text) => {
            const w = await (await dir.getFileHandle(name, { create: true })).createWritable();
            await w.write(text); await w.close();
          };
          await put(top, 'stamp.txt', 'v1/rev1/0000000000000000/1/2');
          await put(top, 'index.txt', 'dbota-cache 1 rev 1\\n');
          await put(await top.getDirectoryHandle('hud', { create: true }), 'a.png', 'x');
          window.showDirectoryPicker = async () => top;
        }"""
    )
    page.click("#loadFolder")
    page.wait_for_function(
        "() => document.getElementById('status').textContent.includes('is for assets v1/rev1/0000000000000000')",
        timeout=60000,
    )
    walked = page.evaluate(
        """async () => { const g = await import('./grant.js');
          const root = await navigator.storage.getDirectory();
          const files = await g.filesOfPrepared(await root.getDirectoryHandle('picked-cache'));
          await root.removeEntry('picked-cache', { recursive: true });
          return files.map(([p]) => p).sort().join(','); }"""
    )
    assert walked == "picked-cache/hud/a.png,picked-cache/index.txt,picked-cache/stamp.txt", walked
    print("the directory picker's folder is walked and refused with its reason:", page.inner_text("#status"))


def check_prepared_page(directory, cache_zip, cache_folder):
    """A prepared asset cache (`dbota export-cache`'s zip, or the cache folder itself) is taken
    by the page with no Dota install: a cache that is not usable is refused with its reason
    before anything is copied; a good one is copied into private storage with progress, kept
    over a reload, and played (the client reads it and the scene is drawn)."""
    import zipfile

    with zipfile.ZipFile(cache_zip) as good:
        names = {n for n in good.namelist() if n not in ("stamp.txt", "index.txt")}
        stamp = good.read("stamp.txt").decode()
    refusing = refusing_zips(cache_zip, HERE.parent.parent / "temp/prepared-smoke")
    with sync_playwright() as p:
        server, base, browser, page, console, problems = opened(p, directory)
        requests = []
        page.on("request", lambda r: requests.append((r.method, r.url, r.post_data)))

        def report():
            print("console:", *[c[:300] for c in console[-25:]], sep="\n  ", file=sys.stderr)

        def state():
            return page.evaluate(
                """async () => { const c = await import('./cache.js'); const s = await c.readState();
                   return s && {stamp: s.stamp, files: [...s.files.keys()].sort()}; }"""
            )

        def load(button, path):
            with page.expect_file_chooser() as chooser:
                page.click(button)
            chooser.value.set_files(str(path))

        page.goto(base)
        page.wait_for_selector("#grant:not([hidden])", timeout=20000)
        assert page.is_visible("#loadZip") and page.is_visible("#loadFolder"), "loading a prepared cache is offered"
        for what, path, words in refusing:
            load("#loadZip", path)
            page.wait_for_function(f"() => document.getElementById('status').textContent.includes({json.dumps(words)})", timeout=60000)
            page.wait_for_selector("#grant:not([hidden])", timeout=20000)
            assert state() is None, f"{what}: nothing was copied"
            print(f"refused ({what}):", page.inner_text("#status"))
        check_directory_picker(page)
        started = time.time()
        load("#loadZip", cache_zip)
        page.wait_for_selector("#progress:not([hidden]), #ready:not([hidden])", timeout=120000)
        page.wait_for_selector("#ready:not([hidden])", timeout=600000)
        print(f"zip loaded after {time.time() - started:.0f} s:", page.inner_text("#readyText"))
        got = state()
        assert got and got["stamp"] == stamp and set(got["files"]) == names, "the copy is the zip's files and stamp"
        with zipfile.ZipFile(cache_zip) as good:
            for path in ["text/tooltips.json", "map/mid/trees.txt"]:
                same = page.evaluate(
                    f"async () => {{ const b = await (await import('./cache.js')).readCache({json.dumps(path)});"
                    " return Array.from(b.slice(0, 64)); }"
                )
                assert bytes(same) == good.read(path)[:64], f"{path} is as the zip has it"
        page.reload()
        page.wait_for_selector("#ready:not([hidden])", timeout=20000)
        print("after reload: ready without loading again")
        play_scene(page, report, 5)
        # The same cache as a folder: the copy deleted, then taken from the folder.
        page.goto(base)
        page.wait_for_selector("#ready:not([hidden])", timeout=20000)
        page.click("#remove")
        page.wait_for_selector("#grant:not([hidden])", timeout=20000)
        # Without the directory picker (Firefox, Safari) the same button uses `<input webkitdirectory>`.
        page.evaluate("() => { delete window.showDirectoryPicker; }")
        load("#loadFolder", cache_folder)
        page.wait_for_selector("#ready:not([hidden])", timeout=600000)
        again = state()
        assert again == got, "the folder gives the same copy as the zip"
        print("folder loaded:", page.inner_text("#readyText"))
        bad = [(m, u) for m, u, d in requests if not u.startswith(base) and not u.startswith("blob:") and not u.startswith("data:")]
        assert not bad, f"requests outside the page's own origin: {bad}"
        assert not [(m, u) for m, u, d in requests if m != "GET" or d], "the page sent data"
        errs = [c for c in console if "panicked" in c or "RuntimeError" in c]
        assert not errs and not problems, (errs, problems)
        page.click("#remove")
        page.wait_for_selector("#grant:not([hidden])", timeout=20000)
        assert state() is None, "the copy is deleted"
        browser.close()
        server.shutdown()
    print("prepared cache test passed")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--install")
    ap.add_argument("--no-game", action="store_true")
    ap.add_argument("--reference-cache", help="a cache a desktop `dbota import` made, to compare the browser's icons with")
    ap.add_argument("--keep", action="store_true", help="leave the imported copy in place")
    ap.add_argument("--settle", type=int, default=5, help="seconds the client draws before the screenshot")
    ap.add_argument("--replay-page", type=pathlib.Path, help="a build with a packed replay built in")
    ap.add_argument("--fullscreen-page", type=pathlib.Path, help="a replay build opened on the Video settings")
    ap.add_argument("--server", help="a bota_server built with the websocket feature, for --replays-page")
    ap.add_argument("--served-page", type=pathlib.Path, help="any build: ?cache=http publishes the served cache's index")
    ap.add_argument("--startup-page", type=pathlib.Path, help="any build: the client picked, failures shown")
    ap.add_argument("--keys-page", type=pathlib.Path, help="any page, even web/ unbuilt: whose keys are whose")
    ap.add_argument("--cursor-page", type=pathlib.Path, help="a replay build: the cursor kept in a match")
    ap.add_argument("--boot-page", type=pathlib.Path, help="any build: every way the start can go wrong ends on a screen")
    ap.add_argument("--pixels-page", type=pathlib.Path, help="a replay build: the canvas drawn at its device pixels")
    ap.add_argument("--replays-page", type=pathlib.Path, help="a build without a replay: a match through radovid is kept in OPFS")
    ap.add_argument("--radovid", help="the radovid binary, for --replays-page (with --server)")
    ap.add_argument("--shots", action="store_true", help="--replays-page saves a screenshot of every step to temp/smoke-replays")
    ap.add_argument("--prepared-page", type=pathlib.Path, help="a build (web/out) that takes a prepared asset cache, with --cache-zip and --cache-folder")
    ap.add_argument("--cache-zip", type=pathlib.Path, help="a zip `dbota export-cache` made with this build's assets, for --prepared-page")
    ap.add_argument("--cache-folder", type=pathlib.Path, help="the cache folder that zip was made from, for --prepared-page")
    ap.add_argument("--only-wire", action="store_true", help="skip the import test even where an install is found")
    args = ap.parse_args()
    install = None if args.only_wire else find_install(args.install)
    if install:
        import_smoke(args, install)
    else:
        print("skipped: the import test needs a Dota 2 install (DBOTA_DOTA_PATH or --install)")
    if args.prepared_page:
        if not (args.cache_zip and args.cache_folder):
            sys.exit("--prepared-page needs --cache-zip and --cache-folder")
        check_prepared_page(args.prepared_page, args.cache_zip, args.cache_folder)
    if args.served_page:
        check_served_cache(args.served_page)
    if args.startup_page:
        check_startup_page(args.startup_page)
    if args.keys_page:
        check_browser_keys(args.keys_page)
    if args.replay_page:
        check_replay_page(args.replay_page)
    if args.fullscreen_page:
        check_fullscreen_page(args.fullscreen_page)
    if args.cursor_page:
        check_cursor_page(args.cursor_page)
    if args.boot_page:
        check_boot_page(args.boot_page)
    if args.pixels_page:
        check_canvas_pixels(args.pixels_page)
    if args.replays_page:
        if not (args.server and args.radovid):
            sys.exit("--replays-page needs --server and --radovid")
        check_replays_page(args.replays_page, args.server, args.radovid, args.shots)


if __name__ == "__main__":
    main()
