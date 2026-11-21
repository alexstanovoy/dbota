"""`--pixels-page` (a replay build): at a device scale of 1.5, and after the page is resized, the
canvas's drawing buffer and the client's surface are the canvas's device-pixel size.
"""

import pathlib
import re
from playwright.sync_api import sync_playwright

from browser import BROWSER_ARGS, serve, wait_console

FLAG = "--pixels-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (200, ('--pixels-page',), dict(type=pathlib.Path, help="a replay build: the canvas drawn at its device pixels")),
)
ORDER = 160


def run(args):
    check_canvas_pixels(args.pixels_page)


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
