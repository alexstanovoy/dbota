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
(SwiftShader does).
"""
import argparse
import functools
import http.server
import os
import pathlib
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
    sys.exit("Dota 2 not found: pass --install")


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


def serve():
    handler = functools.partial(Quiet, directory=str(OUT))
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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--install")
    ap.add_argument("--no-game", action="store_true")
    ap.add_argument("--keep", action="store_true", help="leave the imported copy in place")
    ap.add_argument("--settle", type=int, default=5, help="seconds the client draws before the screenshot")
    args = ap.parse_args()
    install = find_install(args.install)
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

        if not args.no_game:
            page.click("#play")
            try:
                page.wait_for_function(
                    "() => globalThis.dbota_cache && globalThis.dbota_cache.reads && globalThis.dbota_cache.reads.count > 20",
                    timeout=180000,
                )
                # The map: its glTF, its geometry and its images.
                page.wait_for_function(
                    """() => { const s = globalThis.dbota_cache.reads.seen;
                       return s.has('map/mid/mid.gltf') && s.has('map/mid/mid.bin')
                         && [...s].some((p) => p.startsWith('map/mid/') && p.endsWith('.png')); }""",
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
            assert any(p.endswith((".png", ".mp3", ".wav")) for p in reads["last"]), reads
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


if __name__ == "__main__":
    main()
