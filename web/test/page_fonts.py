"""`--fonts-page` (any build): the page serves the interface fonts out of its own folder (a link out
of it is not followed, as a service's static directory does not) and the client takes them.
"""

import pathlib
from playwright.sync_api import sync_playwright

from browser import opened, wait_console

FLAG = "--fonts-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (90, ('--fonts-page',), dict(type=pathlib.Path, help="any build: the page holds its interface fonts")),
)
ORDER = 60


def run(args):
    check_fonts_page(args.fonts_page)


FONTS = ("bota-sans-bold.ttf", "bota-sans-regular.ttf", "bota-sans-cjk.otf")


def check_fonts_page(directory):
    """The page holds the fonts every word of the interface is drawn in, and the client takes them:
    a page whose fonts do not arrive draws buttons, tabs and labels with no text at all."""
    with sync_playwright() as p:
        server, base, browser, page, console, problems = opened(p, directory)
        page.goto(base + "?cache=http")
        for name in FONTS:
            got = page.evaluate(
                "async (url) => { const r = await fetch(url); return [r.status, (await r.arrayBuffer()).byteLength]; }",
                f"assets/fonts/{name}",
            )
            assert got[0] == 200 and got[1] > 10000, f"the page serves no {name}: {got}"
        wait_console(page, console, "interface fonts: .* stands behind the Latin ones", 600, "the client to take its fonts")
        assert not problems, problems
        print("fonts: the page serves the interface fonts and the client has them")
        browser.close()
    server.shutdown()
