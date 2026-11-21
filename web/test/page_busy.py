"""`--busy-page` (any build): a server answering 503 with Retry-After (busy) is told on the page and
tried once more after the wait: the page's own scripts (Chromium and Firefox), the game's download and the
import worker's module; a download refused twice ends on the failure screen.
"""

import pathlib
import re
import sys
import time
from playwright.sync_api import sync_playwright

from browser import HERE, Quiet, assert_in, launch_engine, serve, wait_console

FLAG = "--busy-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (190, ('--busy-page',), dict(type=pathlib.Path, help="any build: a busy server's 503 is waited for and tried once more")),
)
ORDER = 150


def run(args):
    check_busy_page(args.busy_page)


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
