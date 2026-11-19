"""`--boot-page` (any build): the page never spins for ever: a part that does not load, code the
browser cannot run, a page that never arrives, a game download that stalls or is refused each end on a
screen that says the stage, while a slow download that keeps moving is waited for and the game starts
(Chromium and Firefox).
"""

import pathlib
import sys
import threading
import time
from playwright.sync_api import sync_playwright

from browser import BROWSER_ARGS, Quiet, assert_in, launch_engine, serve, wait_console

FLAG = "--boot-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (180, ('--boot-page',), dict(type=pathlib.Path, help="any build: every way the start can go wrong ends on a screen")),
)
ORDER = 160


def run(args):
    check_boot_page(args.boot_page)


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
