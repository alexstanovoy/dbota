"""`--cursor-page` (a replay build, e.g. the `--replay-page` one): in a match a press on the canvas
locks the pointer, the game's cursor moves by the mouse's motion and stays inside the canvas, a
press there is the game's at that place (the dashboard button opens the settings, which let the
pointer go), and closing them or F11 after an Esc takes it again.
"""

import pathlib
import time
from playwright.sync_api import sync_playwright

from browser import DASHBOARD, HANDED, OWN_EVENT, opened, wait_console, wait_loading_cover

FLAG = "--cursor-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (150, ('--cursor-page',), dict(type=pathlib.Path, help="a replay build: the cursor kept in a match")),
)
ORDER = 140


def run(args):
    check_cursor_page(args.cursor_page)


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
        wait_loading_cover(page, console)
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
        far = 10 * max(size[0], size[1])
        mouse("pointermove", far, far)
        last = [size[0] - 1 / size[2], size[1] - 1 / size[2]]
        got = page.evaluate("handed[handed.length - 1]")
        assert got == last, f"the cursor stays inside the canvas: {got}, not {last}"
        mouse("pointermove", -far, -far)
        assert page.evaluate("handed[handed.length - 1]") == [0, 0], "the cursor stops at the top-left corner"
        print("cursor: locked by a press, held inside the canvas")
        # The dashboard button, the top-left corner's first: in a replay page it opens the settings.
        mouse("pointermove", *DASHBOARD)
        # Only a move is the cursor's motion: a press and a release with a movement on them stay where the
        # cursor is.
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
