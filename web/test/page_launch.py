"""`--launch-page` (any build): the WebGL2 client starts and draws its first frames with no panic; a panic fails
at once with its message.
"""

import pathlib
import time
from playwright.sync_api import sync_playwright

from browser import opened

FLAG = "--launch-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (110, ('--launch-page',), dict(type=pathlib.Path, help="any build: the client starts and draws without a panic")),
)
ORDER = 40


def run(args):
    check_launch_page(args.launch_page)


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
