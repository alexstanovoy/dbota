"""`--fullscreen-page`: a replay build made with `DBOTA_WEB_ARGS="--replay built-in --pov 0 --settings video"`;
the display mode and F11 put the canvas fullscreen and leaving it turns the setting back.
"""

import pathlib
from playwright.sync_api import TimeoutError as PlaywrightTimeout, sync_playwright

from browser import HERE, after_frames, click_drawn, opened, wait_console

FLAG = "--fullscreen-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (70, ('--fullscreen-page',), dict(type=pathlib.Path, help="a replay build opened on the Video settings")),
)
ORDER = 120


def run(args):
    check_fullscreen_page(args.fullscreen_page, args.shots)


def check_fullscreen_page(directory, shots=False):
    """Windowed fullscreen in Settings puts the canvas fullscreen (inside the selection gesture), the
    page leaving it (Esc) turns the setting back, and F11 is the same fullscreen. Headless Chromium
    keeps its viewport, so the canvas filling a bigger screen is not seen here."""
    with sync_playwright() as p:
        server, base, browser, page, console, problems = opened(p, directory)
        page.goto(base + "?cache=http")
        wait_console(page, console, r"match known: tick \d+", 600, "the client to know the match")
        page.locator("#dbota").wait_for(state="visible", timeout=60000)

        def shot(name):
            if shots:
                page.screenshot(path=str(HERE.parent.parent / f"temp/smoke-{name}.png"), timeout=600000)

        def fullscreen_becomes(entered, what):
            # The state itself, polled: how long a loaded machine takes to draw and handle a click
            # decides nothing, only the 60 s hang guard.
            try:
                page.wait_for_function(
                    "entered => !!document.fullscreenElement === entered", arg=entered, polling=100, timeout=60000,
                )
            except PlaywrightTimeout as error:
                raise AssertionError(what) from error

        # The settings search stays in the sidebar while the video section moves with font and
        # layout changes. Searching leaves one visible control, so the first Tab focuses it.
        click_drawn(page, (302, 75))
        page.keyboard.type("Display mode")
        after_frames(page)
        for key in ("Enter", "Tab", "Enter", "ArrowDown", "Enter"):
            page.keyboard.press(key)
            after_frames(page)
        fullscreen_becomes(True, "picking windowed fullscreen puts the canvas fullscreen")
        shot("fullscreen-picked")
        page.evaluate("document.exitFullscreen()")
        fullscreen_becomes(False, "the page leaves fullscreen")
        after_frames(page)
        shot("fullscreen-left")  # the setting shows Windowed again
        page.keyboard.press("F11")
        fullscreen_becomes(True, "F11 is the canvas's fullscreen")
        page.keyboard.press("F11")
        fullscreen_becomes(False, "F11 leaves fullscreen")
        assert not problems, problems
        print("fullscreen: the setting and F11 enter it, leaving it works")
        browser.close()
    server.shutdown()
