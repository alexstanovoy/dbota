#!/usr/bin/env python3
"""The browser's first run, end to end in the real client (headless Chromium, Playwright): everything a
player meets before the menu is the client's, drawn in its canvas, and these checks drive it the way the
native first-run tests do: through the client's own script steps (`--script`, handed to the page by an init
script as `globalThis.dbota_args`, once per load of the page) and its log lines in the console, while the
browser's dialogs are answered here (a folder upload, or a stubbed directory picker).

    PYTHONPATH=temp/pwlib python3 web/test/first_run.py --page temp/page [--replay-page temp/replay \\
        --install "<dota 2 beta>" --reference-cache "$DBOTA_ASSETS"]

* `--page` (any build): the journey on a tiny install (a package directory with nothing in it: every asset
  fails to convert, which the screen tells): a fresh profile meets the welcome screen, a folder without Dota is
  refused with what to pick, the install is imported and the client loads again at sign-in; the settings'
  RE-IMPORT GAME FILES brings the screen back and BACK TO DBOTA leaves it. Then: a copy another build made, one
  whose index is gone, one whose stamp is too long to be one, and one the browser evicted each start at the
  screen again; a write the storage refuses ends the import with no copy left; the browser's directory picker
  is used where it exists; a browser without private storage or graphics is told on the page's own screen; a
  storage that does not answer is said on the first-run screen; a served copy (`?cache=http`) whose index is
  missing, wrong or endless is refused on the failure screen. Nothing is fetched from elsewhere than the page.
* `--replay-page` with `--install` (a build with a replay built in, a Dota 2 install): the real import through
  the canvas: CANCEL mid-import leaves no copy, a full import while the frame keeps drawing, every icon the
  same as a desktop import's (`--reference-cache`), the map then drawn from the copy; a stale copy is
  imported again. Prints the import's time, the browser's peak memory and the longest task of the page's
  thread while it ran.
"""
import argparse
import hashlib
import json
import os
import pathlib
import re
import shutil
import sys
import tempfile
import time

from playwright.sync_api import sync_playwright

from smoke import BROWSER_ARGS, Quiet, serve

HERE = pathlib.Path(__file__).resolve().parent
# A VPK directory file (version 1) with no file in it: the smallest install the importer opens.
EMPTY_PACKAGE = bytes([0x34, 0x12, 0xAA, 0x55, 1, 0, 0, 0, 1, 0, 0, 0, 0])
# Where Steam keeps Dota 2 on Linux, for a run that names no install (DBOTA_DOTA_PATH or --install).
STEAM = [
    "~/.var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/common/dota 2 beta",
    "~/.steam/steam/steamapps/common/dota 2 beta",
    "~/.local/share/Steam/steamapps/common/dota 2 beta",
]
# Seconds any one wait may take: a hang guard only.
PATIENCE = 600


def farm(install, root):
    """`root/dota 2 beta/game/dota`: the install's packages and interface typefaces as hard links (Chromium's
    folder upload skips symlinks; nothing is copied, so `root` is on the install's drive), plus a decoy file
    the client must ignore."""
    dota = root / "dota 2 beta/game/dota"
    source = install / "game/dota"
    for pattern in ("*.vpk", "maps/*.vpk", "panorama/fonts/*.otf"):
        for found in source.glob(pattern):
            link = dota / found.relative_to(source)
            link.parent.mkdir(parents=True, exist_ok=True)
            if not link.exists():
                os.link(found, link)
    (dota / "gameinfo.gi").write_text("not a package")
    return root / "dota 2 beta"


def find_install(given):
    for candidate in [given, os.environ.get("DBOTA_DOTA_PATH"), *STEAM]:
        if candidate and (pathlib.Path(candidate).expanduser() / "game/dota/pak01_dir.vpk").is_file():
            return pathlib.Path(candidate).expanduser()
    return None


def fixtures(root):
    """A tiny install (`dota 2 beta/game/dota/pak01_dir.vpk`, nothing in it) and a folder without Dota."""
    dota = root / "dota 2 beta/game/dota"
    dota.mkdir(parents=True)
    (dota / "pak01_dir.vpk").write_bytes(EMPTY_PACKAGE)
    (dota / "gameinfo.gi").write_text("not a package")
    nothing = root / "nothing"
    nothing.mkdir()
    (nothing / "readme.txt").write_text("no Dota here")
    return root / "dota 2 beta", nothing


def scripts_init(scripts, extra=()):
    """An init script handing the client `scripts[n]` on the page's n-th load in this tab (none after the
    last), with `extra` arguments on every load."""
    return f"""(() => {{
      const scripts = {json.dumps(scripts)};
      const n = Number(sessionStorage.getItem("dbota-load") || "0");
      sessionStorage.setItem("dbota-load", String(n + 1));
      globalThis.dbota_args = [...{json.dumps(list(extra))}, ...(scripts[n] ? ["--script", scripts[n]] : [])];
    }})();"""


class Run:
    """A fresh browser profile on a page served from `directory`: its console, page errors and requests."""

    def __init__(self, p, directory, handler=Quiet, viewport=(1280, 720)):
        self.server = serve(directory, handler)
        self.base = f"http://127.0.0.1:{self.server.server_address[1]}/"
        self.browser = p.chromium.launch(args=BROWSER_ARGS)
        self.context = self.browser.new_context(viewport={"width": viewport[0], "height": viewport[1]})
        self.page = self.context.new_page()
        self.console, self.problems, self.requests = [], [], []
        # The folders the client's folder upload field is given, in turn (`answer`).
        self.answers = []
        self.answered = 0
        self.page.on("console", lambda m: self.console.append(m.text))
        self.page.on("pageerror", lambda e: self.problems.append(f"page error: {e}"))
        self.page.on("request", lambda r: self.requests.append((r.method, r.url, r.post_data)))

    def close(self):
        self.browser.close()
        self.server.shutdown()
        self.server.server_close()

    def answer(self):
        """While the first-run screen waits for its folder dialog, the next folder is handed to the folder
        upload field the client opened. A browser shows that dialog only within a moment of the player's own
        gesture, which the client's script steps are not, so the field gets its files as Playwright gives
        them to any: its change event is what the client listens to."""
        states = [c for c in self.console if "first run: " in c and " | " in c]
        if self.answers and states and "first run: choosing " in states[-1] and len(states) > self.answered:
            self.answered = len(states)
            self.page.set_input_files("input[type=file]", str(self.answers.pop(0)))

    def said(self, pattern, what, timeout=PATIENCE, after=0):
        """Waits for a console line matching `pattern` among those from the `after`-th on."""
        seen = re.compile(pattern)
        deadline = time.time() + timeout
        while time.time() < deadline:
            self.answer()
            failed = [c for c in self.console if "script: timed out" in c or "does not hold" in c or "panicked" in c]
            assert not failed, failed
            for line in self.console[after:]:
                if seen.search(line):
                    return line
            self.page.wait_for_timeout(200)
        told = [c for c in self.console if "memory: " not in c]
        print("console:", *[c[:300] for c in told[-40:]], sep="\n  ", file=sys.stderr)
        sys.exit(f"timed out waiting for {what}")

    def step(self, step, after=0):
        """Waits for the script to have played `step`."""
        return self.said("^script: " + re.escape(step) + "$", f"the script step {step}", after=after)

    def choose(self, *folders):
        """Answers the folder uploads the client opens next with `folders`, in turn."""
        self.answers.extend(folders)

    def storage(self):
        """What the page's private storage holds of the copies: `dbota/` listed, and current.txt."""
        return self.page.evaluate("""async () => {
          const root = await navigator.storage.getDirectory();
          let dbota;
          try { dbota = await root.getDirectoryHandle('dbota'); } catch (e) { return null; }
          const names = [];
          for await (const name of dbota.keys()) names.push(name);
          let current = null;
          try { current = await (await (await dbota.getFileHandle('current.txt')).getFile()).text(); } catch (e) {}
          return { names: names.sort(), current };
        }""")

    def copy_file(self, path, text=None, remove=False):
        """Reads, writes (`text`) or removes a file of the current copy."""
        return self.page.evaluate("""async ([path, text, remove]) => {
          const dbota = await (await navigator.storage.getDirectory()).getDirectoryHandle('dbota');
          const current = (await (await (await dbota.getFileHandle('current.txt')).getFile()).text()).trim();
          let dir = await dbota.getDirectoryHandle(current);
          const parts = path.split('/');
          const name = parts.pop();
          for (const part of parts) dir = await dir.getDirectoryHandle(part);
          if (remove) return dir.removeEntry(name);
          if (text !== null) {
            const w = await (await dir.getFileHandle(name, { create: true })).createWritable();
            await w.write(text); await w.close(); return null;
          }
          return [...new Uint8Array(await (await (await dir.getFileHandle(name)).getFile()).arrayBuffer())];
        }""", [path, text, remove])

    def own_origin_only(self):
        bad = [(m, u) for m, u, _ in self.requests if not u.startswith((self.base, "blob:", "data:"))]
        posts = [(m, u) for m, u, d in self.requests if m != "GET" or d]
        assert not bad, f"requests outside the page's own origin: {bad}"
        assert not posts, f"the page sent data: {posts}"


def journey(p, page_dir, install, nothing):
    """Fresh profile -> welcome -> a folder without Dota refused -> the install imported -> sign-in -> the
    menu (without an account); the settings' RE-IMPORT GAME FILES -> the screen -> BACK TO DBOTA -> sign-in."""
    scripts = [
        "await:text:Welcome to dbota;await:text:free on Steam;await:text:never leave your computer;"
        "await:text:CHOOSE YOUR DOTA 2 FOLDER;await:nobutton:quit dbota;await:nobutton:import from dota 2;"
        "click:choose the dota 2 folder;await:text:holds no Dota 2;click:choose the dota 2 folder;"
        "await:button:continue;await:text:could not be converted;click:continue",
        "await:button:sign in;await:button:create account;click:play without an account;click:settings;"
        "click:options;click:re-import game files",
        "await:text:Re-import the game files;await:button:back to dbota;click:back to dbota",
        "await:button:create account",
    ]
    run = Run(p, page_dir)
    try:
        run.page.add_init_script("delete window.showDirectoryPicker;" + scripts_init(scripts))
        run.choose(nothing, install)
        run.page.goto(run.base)
        run.said(r"first run: ready \| CHOOSE YOUR DOTA 2 FOLDER", "the welcome screen")
        assert run.page.is_hidden("#gate"), "the page's own screen stays hidden: the client shows the first run"
        run.step("await:text:holds no Dota 2")
        run.step("click:continue")
        run.said(r"first run: loading the page again(?! to)", "the page loaded again on the copy")
        at = len(run.console)
        run.step("await:button:create account", after=at)
        state = run.storage()
        assert state and len(state["names"]) == 2 and state["current"].strip() in state["names"], state
        stamp = bytes(run.copy_file("stamp.txt")).decode()
        assets = run.said(r"assets rev\S+", "the client's asset fingerprint")
        assert stamp.startswith(re.search(r"assets (\S+)", assets).group(1) + "/"), (stamp, assets)
        print(f"first run: welcome, a folder without Dota refused, the import, sign-in; storage {state}")
        run.said(r"first run: loading the page again to re-import", "the re-import asked", after=at)
        at = len(run.console)
        run.step("click:back to dbota", after=at)
        run.step("await:button:create account", after=at)
        assert run.storage() == state, "BACK TO DBOTA keeps the copy as it was"
        print("first run: RE-IMPORT GAME FILES brings the screen back, BACK TO DBOTA keeps the copy")
        run.own_origin_only()
        assert not run.problems, run.problems
        return run
    except BaseException:
        run.close()
        raise


def again_at_screen(run, change, screen, why):
    """After `change` to the copy, a load of the page starts at the first-run `screen`, the log saying why."""
    change()
    at = len(run.console)
    run.page.reload()
    run.said(r"asset copy: " + why, "why the copy is not read", after=at)
    run.said(r"first run: ready \| " + screen, f"the {screen} screen", after=at)


def cleanup_while_another_tab_publishes(run):
    """Publish while startup is between reading the current pointer and taking its cleanup lock."""
    original = run.storage()
    run.page.evaluate("sessionStorage.setItem('race-cleanup', '1')")
    run.page.add_init_script("""(() => {
      if (!sessionStorage.getItem('race-cleanup')) return;
      const locks = navigator.locks;
      const request = locks.request.bind(locks);
      locks.request = async (name, options, run) => {
        if (name !== 'dbota-import' || !sessionStorage.getItem('race-cleanup')) {
          return request(name, options, run);
        }
        sessionStorage.removeItem('race-cleanup');
        await request(name, async () => {
          const root = await (await navigator.storage.getDirectory()).getDirectoryHandle('dbota');
          await root.getDirectoryHandle('copy-42', {create: true});
          const pointer = await (await root.getFileHandle('current.txt')).createWritable();
          await pointer.write('copy-42\\n');
          await pointer.close();
        });
        const result = await request(name, options, run);
        console.info('cache fixture: startup cleanup completed');
        return result;
      };
    })();""")
    run.page.reload()
    run.said(r"^cache fixture: startup cleanup completed$", "the startup cleanup")
    storage = run.storage()
    assert storage["current"] == "copy-42\n" and "copy-42" in storage["names"], storage
    run.page.evaluate("""async (previous) => {
      const root = await (await navigator.storage.getDirectory()).getDirectoryHandle('dbota');
      const pointer = await (await root.getFileHandle('current.txt')).createWritable();
      await pointer.write(previous); await pointer.close();
      await root.removeEntry('copy-42');
    }""", original["current"])
    print("startup cleanup left the copy published by another tab")


def copies(run, page_dir, install):
    """A copy of another build, one without its index, one whose stamp is too long to be one, and one the
    browser evicted each start at the screen; the import then makes a copy the client starts on."""
    other = "rev1.1/0000000000000000/1/2"
    stamp = bytes(run.copy_file("stamp.txt")).decode()
    again_at_screen(run, lambda: run.copy_file("stamp.txt", other), "CHOOSE YOUR DOTA 2 FOLDER", r"the copy is stamped")
    run.said(r"out of date for this version|first run: ready", "the stale copy's screen")
    print("a copy another build made: the screen asks for the folder again")
    again_at_screen(run, lambda: run.copy_file("stamp.txt", "x" * 5000), "CHOOSE YOUR DOTA 2 FOLDER",
                    r"stamp\.txt is 5000 bytes, at most 4096")
    print("a stamp longer than a stamp may be is not read: the screen")
    run.copy_file("stamp.txt", stamp)
    again_at_screen(run, lambda: run.copy_file("index.txt", remove=True), "CHOOSE YOUR DOTA 2 FOLDER", r"the copy has no index")
    print("a copy without its index (an import cut off): the screen")
    evict = "async () => (await navigator.storage.getDirectory()).removeEntry('dbota', { recursive: true })"
    again_at_screen(run, lambda: run.page.evaluate(evict), "CHOOSE YOUR DOTA 2 FOLDER", r"none in this browser's private storage")
    print("a copy the browser evicted: the screen")


def failed_write(p, page_dir, install):
    """A write the storage refuses ends the import: the screen says so, and nothing of the copy is left."""
    run = Run(p, page_dir)

    def failing(route):
        body = route.fetch().text()
        route.fulfill(body="FileSystemSyncAccessHandle.prototype.write = () => { throw new Error('injected: the "
                           "write failed'); };\n" + body, content_type="text/javascript")

    try:
        run.page.route("**/import_worker.js", failing)
        run.page.add_init_script("delete window.showDirectoryPicker;" + scripts_init(
            ["click:choose the dota 2 folder;await:text:The import failed: ;await:text:injected"]))
        run.choose(install)
        run.page.goto(run.base)
        run.step("await:text:injected")
        assert run.storage() == {"names": [], "current": None}, run.storage()
        print("a write the storage refuses: the import fails, no copy is left")
    finally:
        run.close()


def failed_replacement(p, page_dir, install):
    """Fail the publication of a replacement and keep the previous copy, even when it is stale."""
    run = Run(p, page_dir)
    try:
        run.page.add_init_script("delete window.showDirectoryPicker;" + scripts_init([
            "click:choose the dota 2 folder;await:button:continue;click:continue",
            "await:button:sign in",
            "click:choose the dota 2 folder;await:text:The import failed: ;await:text:injected publication failure",
        ]))
        run.choose(install, install)
        run.page.goto(run.base)
        run.step("await:button:sign in")
        run.copy_file("stamp.txt", "rev1.1/0000000000000000/1/2")
        previous = run.storage()

        def failing(route):
            body = route.fetch().text()
            route.fulfill(body="""for (const method of ['createWritable', 'createSyncAccessHandle']) {
              const open = FileSystemFileHandle.prototype[method];
              FileSystemFileHandle.prototype[method] = function (...args) {
                return this.name === 'current.txt' ? Promise.reject(new Error('injected publication failure')) : open.apply(this, args);
              };
            }\n""" + body, content_type="text/javascript")

        run.page.route("**/import_worker.js", failing)
        run.page.reload()
        run.step("await:text:injected publication failure")
        assert run.storage() == previous, "a failed publication lost the previous copy"
        print("a failed publication kept the previous copy")
    finally:
        run.close()


PICKER = """(() => {
  // The browser's directory picker, answered with a folder of the page's own private storage: the
  // tiny install, or a folder without Dota.
  globalThis.showDirectoryPicker = async () => {
    const root = await navigator.storage.getDirectory();
    const make = async (dir, path) => { for (const part of path) dir = await dir.getDirectoryHandle(part, { create: true }); return dir; };
    const picked = await make(root, ['picked', globalThis.dbota_pick]);
    if (globalThis.dbota_pick === 'library') {
      const dota = await make(picked, ['steamapps', 'common', 'dota 2 beta', 'game', 'dota']);
      const w = await (await dota.getFileHandle('pak01_dir.vpk', { create: true })).createWritable();
      await w.write(new Uint8Array(%s)); await w.close();
    }
    return picked;
  };
})();""" % json.dumps(list(EMPTY_PACKAGE))


def directory_picker(p, page_dir):
    """Where the browser has a directory picker, the client uses it: a folder without Dota is refused, a Steam
    library holding the install is walked down to it and imported."""
    for pick, last in (("empty", "await:text:holds no Dota 2"), ("library", "await:button:continue")):
        run = Run(p, page_dir)
        try:
            run.page.add_init_script(PICKER + f"globalThis.dbota_pick = '{pick}';" + scripts_init(
                [f"click:choose the dota 2 folder;{last}"]))
            run.page.goto(run.base)
            run.step(last)
        finally:
            run.close()
    print("the directory picker: a folder without Dota refused, a Steam library walked down to the install")


def unsupported(p, page_dir):
    """A browser without what the client needs is told on the page's own screen, which says what is missing
    and that a desktop dbota exists; no download link while no release names one."""
    cases = [
        ("no private storage", "Object.defineProperty(navigator, 'storage', {value: undefined});", "Private storage"),
        ("no import lock", "Object.defineProperty(navigator, 'locks', {value: undefined});", "Web Locks"),
        ("no folder dialog", "delete window.showDirectoryPicker; delete HTMLInputElement.prototype.webkitdirectory;",
         "choose a folder"),
        ("no graphics", """delete Navigator.prototype.gpu;
           const get = HTMLCanvasElement.prototype.getContext;
           HTMLCanvasElement.prototype.getContext = function (type, o) { return type === 'webgl2' ? null : get.call(this, type, o); };""",
         "WebGPU or WebGL 2"),
    ]
    for name, init, lacks in cases:
        run = Run(p, page_dir)
        try:
            run.page.add_init_script(init)
            run.page.goto(run.base)
            run.page.wait_for_selector("#unsupported", state="visible", timeout=PATIENCE * 1000)
            said = run.page.inner_text("#unsupported")
            assert lacks in said and "Windows, Linux and macOS" in said, said
            assert run.page.is_hidden("#native"), "no download links while no release names them"
            assert not any("drawing with" in c for c in run.console), "no client is loaded"
            print(f"unsupported, {name}: said on the page")
        finally:
            run.close()


def storage_hangs(p, page_dir):
    """A private storage that never answers: the first-run screen says so after its limit."""
    run = Run(p, page_dir)
    try:
        run.page.add_init_script("navigator.storage.getDirectory = () => new Promise(() => {});")
        run.page.goto(run.base)
        run.said(r"first run: ready \| CHOOSE YOUR DOTA 2 FOLDER \| note: .*did not answer in 20 seconds", "the note")
        print("a storage that does not answer: the first-run screen says so")
    finally:
        run.close()


def served_refusals(p, page_dir):
    """A served copy (`?cache=http`) is read only through its validated index, read no further than an index
    may be long: a missing, wrong or endless one ends on the failure screen, saying why."""
    source = (HERE.parent.parent / "crates/dbota_import/src/import.rs").read_text()
    header = "dbota-cache 1 rev " + re.search(r"pub const IMPORT_REV: u32 = (\d+)", source).group(1) + "\n"

    class Served(Quiet):
        body = None
        endless = False

        def do_GET(self):
            if self.path != "/cache/index.txt":
                return super().do_GET()
            if Served.endless:
                self.send_response(200)
                self.end_headers()
                try:
                    for _ in range(64):
                        self.wfile.write(b"x" * (1 << 20))
                except OSError:
                    pass
                return
            if Served.body is None:
                return self.send_error(404)
            body = Served.body.encode()
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    cases = [
        ("missing", None, False, "HTTP 404"),
        ("wrong header", "wrong\nfile 1 icons/items/blink.png\n", False, "index.txt starts"),
        ("a path out of the copy", header + "file 1 icons/items/../../outside.png\n", False, "must not"),
        ("listed twice", header + "file 1 icons/items/blink.png\nfile 1 icons/items/blink.png\n", False, "again"),
        ("endless", None, True, "exceeds"),
    ]
    for name, body, endless, why in cases:
        Served.body, Served.endless = body, endless
        run = Run(p, page_dir, Served)
        try:
            run.page.goto(run.base + "?cache=http")
            run.page.wait_for_selector("#failed", state="visible", timeout=PATIENCE * 1000)
            assert "served copy of the assets cannot be read" in run.page.inner_text("#failTitle")
            detail = run.page.text_content("#failDetail")
            assert why in detail, (name, detail)
            print(f"a served index, {name}: refused ({detail[:80]})")
        finally:
            run.close()


def browser_memory():
    """The memory of the processes this one started (the browser's), in MB: their proportional set sizes,
    which count a page shared by several processes once."""
    parents = {}
    for proc in pathlib.Path("/proc").iterdir():
        if proc.name.isdigit():
            try:
                parents[int(proc.name)] = int(re.search(r"PPid:\s+(\d+)", (proc / "status").read_text()).group(1))
            except (OSError, AttributeError):
                continue
    mine = {os.getpid()}
    for _ in range(16):
        mine |= {pid for pid, parent in parents.items() if parent in mine}
    total = 0
    for pid in mine - {os.getpid()}:
        try:
            total += int(re.search(r"Pss:\s+(\d+)", pathlib.Path(f"/proc/{pid}/smaps_rollup").read_text()).group(1)) // 1024
        except (OSError, AttributeError):
            continue
    return total


# The longest task of the page's thread while an import runs (from when the test marks its start,
# `dbota_import_began`, a time of the page's clock), kept over the reload that follows it in the tab's
# session storage (`dbota-long`).
LONG_TASKS = """(() => {
  try {
    new PerformanceObserver((list) => {
      const began = globalThis.dbota_import_began;
      if (began === undefined) return;
      const during = list.getEntries().filter((e) => e.startTime >= began).map((e) => e.duration);
      const longest = Math.max(Number(sessionStorage.getItem("dbota-long") || "0"), ...during);
      sessionStorage.setItem("dbota-long", String(longest));
    }).observe({ type: 'longtask', buffered: true });
  } catch (e) {}
})();"""


def real_import(p, page_dir, install, reference):
    """The real import through the canvas, its CANCEL, the frames while it runs, the icons it makes, the
    map drawn from the copy, and a stale copy imported again."""
    # Beside the repository, on the install's drive (hard links), never in a RAM-backed temporary folder.
    scratch = HERE.parent.parent / "temp" / f"first-run-install-{os.getpid()}"
    folder = farm(install, scratch)
    scripts = [
        "click:choose the dota 2 folder;await:text: of ;frames:10;click:cancel;await:text:Import cancelled",
        "click:choose the dota 2 folder;await:text:MAKING YOUR PRIVATE COPY;await:text: of ;frames:120",
        "",
        "await:text:out of date for this version;click:choose the dota 2 folder;await:text:MAKING YOUR PRIVATE COPY",
    ]
    run = Run(p, page_dir)
    try:
        run.page.add_init_script("delete window.showDirectoryPicker;" + LONG_TASKS + scripts_init(scripts))
        run.choose(folder, folder, folder)
        run.page.goto(run.base)
        run.step("await:text:Import cancelled")
        assert run.storage() == {"names": [], "current": None}, f"a cancelled import leaves nothing: {run.storage()}"
        print("real import: CANCEL mid-import leaves no copy")
        at = len(run.console)
        run.page.reload()
        run.said(r"first run: working", "the import", after=at)
        run.page.evaluate("globalThis.dbota_import_began = performance.now()")
        started = time.time()
        before = browser_memory()
        peak = before
        while not any(re.search(r"first run: (restarting|partial)", c) for c in run.console[at:]):
            peak = max(peak, browser_memory())
            run.page.wait_for_timeout(500)
            assert time.time() - started < 1800, "the import ended"
        took = time.time() - started
        drawn = [i for i, c in enumerate(run.console) if c == "script: frames:120"]
        ended = [i for i, c in enumerate(run.console) if re.search(r"first run: (restarting|partial)", c)]
        assert drawn and drawn[0] < ended[0], "120 frames were drawn while the import ran"
        run.said(r"asset copy: copy-\d+ in this browser's private storage", "the copy read", after=ended[0])
        run.said(r"map prepared: \d+ merged meshes", "the map drawn from the copy", after=ended[0])
        longest = float(run.page.evaluate("sessionStorage.getItem('dbota-long') || '0'"))
        print(f"real import: {took:.0f} s, the frame kept drawing; longest task of the page's thread while it ran "
              f"{longest:.0f} ms; the browser's memory {before} MB as it began, at most {peak} MB")
        bad = [c for c in run.console[ended[0]:] if "cannot be read" in c or "panicked" in c]
        assert not bad, bad[:5]
        if reference:
            root = pathlib.Path(reference)
            paths = sorted(str(f.relative_to(root)) for f in (root / "icons").rglob("*.png"))
            wrong = [p for p in paths if hashlib.sha256(bytes(run.copy_file(p))).hexdigest()
                     != hashlib.sha256((root / p).read_bytes()).hexdigest()]
            assert paths and not wrong, f"{len(wrong)} of {len(paths)} icons differ from the desktop's, e.g. {wrong[:5]}"
            print(f"real import: all {len(paths)} icons the same as the desktop import's")
        run.copy_file("stamp.txt", "rev1.1/0000000000000000/1/2")
        at = len(run.console)
        run.page.reload()
        run.step("await:text:MAKING YOUR PRIVATE COPY", after=at)
        run.said(r"map prepared: \d+ merged meshes", "the map drawn from the copy made again", after=at)
        print("real import: a stale copy is imported again")
        run.own_origin_only()
    finally:
        run.close()
        shutil.rmtree(scratch, ignore_errors=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--page", type=pathlib.Path, help="any build: the first run on a tiny install, and its refusals")
    ap.add_argument("--replay-page", type=pathlib.Path, help="a build with a replay: the real import (needs --install)")
    ap.add_argument("--install", help="the dota 2 beta folder, for --replay-page (else DBOTA_DOTA_PATH or Steam's)")
    ap.add_argument("--reference-cache", help="a desktop import of this build, whose icons the browser's must equal")
    args = ap.parse_args()
    with sync_playwright() as p:
        if args.page:
            scratch = pathlib.Path(tempfile.mkdtemp(prefix=f"dbota-first-run-{os.getpid()}-"))
            try:
                install, nothing = fixtures(scratch)
                run = journey(p, args.page, install, nothing)
                try:
                    cleanup_while_another_tab_publishes(run)
                    copies(run, args.page, install)
                finally:
                    run.close()
                failed_write(p, args.page, install)
                failed_replacement(p, args.page, install)
                directory_picker(p, args.page)
                unsupported(p, args.page)
                storage_hangs(p, args.page)
                served_refusals(p, args.page)
            finally:
                shutil.rmtree(scratch, ignore_errors=True)
        if args.replay_page:
            install = find_install(args.install)
            if not install:
                sys.exit("--replay-page needs a Dota 2 install: --install, DBOTA_DOTA_PATH or Steam's usual places")
            real_import(p, args.replay_page, install, args.reference_cache or os.environ.get("DBOTA_ASSETS"))
    print("first run checks passed")


if __name__ == "__main__":
    main()
