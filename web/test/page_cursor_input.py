"""`--cursor-input-page` (a replay build, both clients as web/build.sh makes them): the same with a real mouse,
under `DBOTA_HEADLESS_XWAYLAND=1 tools/headless.sh`: a Chromium window on its X server at a device scale of
1.5, moved and clicked through the X server's own input (XTEST), the way a mouse's events reach the browser.
The game's cursor goes as far as the mouse moves, and a click lands where the cursor is drawn: the dashboard
button opens the settings and the client reports the cursor at the place the page holds.
"""

import ctypes
import ctypes.util
import os
import pathlib
import re
import sys
import time
from playwright.sync_api import TimeoutError as PlaywrightTimeout, sync_playwright

from browser import BROWSER_ARGS, DASHBOARD, serve, wait_console, wait_loading_cover

FLAG = "--cursor-input-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (160, ('--cursor-input-page',), dict(type=pathlib.Path, help="a replay build: the cursor with a real mouse (X server)")),
)
ORDER = 130


def run(args):
    check_cursor_input(args.cursor_input_page)


class XMouse:
    """The mouse of the X server in DISPLAY, moved and pressed through XTEST: the events reach the browser as a
    real mouse's do (through the server and the window system), which no page-made event or DevTools input does
    while the pointer is locked."""

    def __init__(self):
        self.x11 = ctypes.CDLL(ctypes.util.find_library("X11") or "libX11.so.6")
        self.xtst = ctypes.CDLL(ctypes.util.find_library("Xtst") or "libXtst.so.6")
        self.x11.XOpenDisplay.restype = ctypes.c_void_p
        self.x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
        self.x11.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
        self.x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
        self.xtst.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
        self.xtst.XTestFakeRelativeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
        self.xtst.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
        self.display = self.x11.XOpenDisplay(os.environ["DISPLAY"].encode())
        assert self.display, f"no X server at DISPLAY={os.environ['DISPLAY']}"

    def to(self, x, y):
        self.xtst.XTestFakeMotionEvent(self.display, -1, x, y, 0)
        self.x11.XSync(self.display, 0)

    def by(self, dx, dy):
        self.xtst.XTestFakeRelativeMotionEvent(self.display, dx, dy, 0)
        self.x11.XSync(self.display, 0)

    def click(self, button=1):
        for down in (1, 0):
            self.xtst.XTestFakeButtonEvent(self.display, button, down, 0)
            self.x11.XSync(self.display, 0)
            time.sleep(0.1)

    def close(self):
        self.x11.XCloseDisplay(self.display)


# Every place the page hands the client while the pointer is locked: [type, clientX, clientY].
HANDED_ALL = """window.handed = [];
for (const type of ['pointermove', 'pointerdown', 'pointerup'])
    addEventListener(type, (e) => { if (!e.isTrusted) handed.push([type, e.clientX, e.clientY]); }, true);"""


def check_cursor_input(directory):
    """A real mouse in a match, at a device scale of 1.5: the game's cursor moves as far as the mouse does, and a
    click lands where the cursor is drawn (the dashboard button opens the settings, and the client reports the
    cursor at the page's place, not moved by the press)."""
    if not os.environ.get("DISPLAY"):
        sys.exit("--cursor-input-page needs an X server: run it under DBOTA_HEADLESS_XWAYLAND=1 tools/headless.sh")
    scale = 1.5
    mouse = XMouse()
    with sync_playwright() as p:
        server = serve(directory)
        base = f"http://127.0.0.1:{server.server_address[1]}/"
        # A window of the X server (not Wayland, where nothing could move the mouse), at the scale of a desktop
        # set to 150 %: the browser's own device scale, not an emulated one.
        browser = p.chromium.launch(
            headless=False,
            args=[*BROWSER_ARGS, "--ozone-platform=x11", f"--force-device-scale-factor={scale}",
                  "--window-position=0,0", "--window-size=1000,560"],
            env={k: v for k, v in os.environ.items() if k != "WAYLAND_DISPLAY"},
        )
        page = browser.new_context(no_viewport=True).new_page()
        console, problems = [], []
        page.on("console", lambda m: console.append(m.text))
        page.on("pageerror", lambda e: problems.append(f"page error: {e}"))
        page.add_init_script(HANDED_ALL)
        page.add_init_script("""addEventListener('pointermove', e => {
          if (e.isTrusted) window.pointerOnCanvas = e.target.id === 'dbota';
        }, true)""")
        page.goto(base + "?cache=http")
        wait_console(page, console, r"match known: tick \d+", 600, "the client to know the match")
        wait_loading_cover(page, console)
        assert page.evaluate("devicePixelRatio") == scale, page.evaluate("devicePixelRatio")
        last = lambda: page.evaluate("handed.length ? handed[handed.length - 1] : null")

        # A click in the window takes the pointer.
        mouse.to(400, 300)
        page.wait_for_function("() => window.pointerOnCanvas", polling=100, timeout=60000)
        mouse.click()
        page.wait_for_function(
            "() => document.pointerLockElement === document.getElementById('dbota')", polling=100, timeout=60000,
        )
        wait_console(page, console, "cursor locked to the canvas", 60, "the client to see the lock")
        print("cursor input: a real click locks the pointer")

        # The game's cursor goes as far as the mouse: 30 device pixels right and 15 down are 20 and 10 of the
        # page's (CSS) pixels at a scale of 1.5.
        # A single device pixel can round away at this scale. Wait for a genuine handed move
        # before taking the starting position; pointer lock alone does not acknowledge input.
        def move_mouse(x, y):
            before = page.evaluate("handed.length")
            mouse.by(x, y)
            page.wait_for_function(
                "before => handed.slice(before).some(([type]) => type === 'pointermove')",
                arg=before, polling=100, timeout=60000,
            )

        move_mouse(3, 3)
        start = last()
        for _ in range(3):
            move_mouse(10, 5)
        # Xwayland and the browser can coalesce several moves; await their delivery.
        page.wait_for_function("""([x, y]) => {
          const at = handed.at(-1);
          return at && at[1] - x >= 19 && at[2] - y >= 9;
        }""", arg=start[1:], polling=100, timeout=60000)
        end = last()
        moved = [end[1] - start[1], end[2] - start[2]]
        failures = []
        if not (abs(moved[0] - 20) <= 1 and abs(moved[1] - 10) <= 1):
            failures.append(f"the mouse's 30x15 device pixels move the cursor 20x10 CSS pixels, not {moved}")

        # Into the top-left corner, then onto the dashboard button (the interface scales with the page's
        # height, at least to 0.6 of a 720 pixel high one's), a whole number of device pixels away.
        middle = DASHBOARD[0] * max(page.evaluate("innerHeight") / 720, 0.6)
        steps = round(middle * scale)
        target = [steps / scale] * 2
        for _ in range(60):
            if last()[1:] == [0, 0]:
                break
            move_mouse(-60, -60)
        assert last()[1:] == [0, 0], f"the cursor stops at the top-left corner: {last()}"
        before_move = page.evaluate("handed.length")
        mouse.by(steps, steps)
        # The move reaches the page through Xwayland and the browser, when it does: as late as the machine is busy
        # (a 500 ms sleep read the corner on a loaded run, and the press that followed was already at the right place).
        # So the state is awaited, not a span: the wait ends the moment the cursor is within a pixel of where it
        # must end, and its 60 s only decides that it never came.
        try:
            page.wait_for_function("""([before, x, y]) => {
              const at = handed.at(-1);
              return handed.length > before && at[0] === 'pointermove'
                && Math.abs(at[1] - x) <= 1 && Math.abs(at[2] - y) <= 1;
            }""", arg=[before_move, *target], polling=100, timeout=60000)
        except PlaywrightTimeout as error:
            raise AssertionError(
                f"the cursor never reached {target} CSS pixels within 60 s of the mouse's {steps}x{steps} device "
                f"pixels from the corner; the last place handed to the client is {last()}") from error
        at = last()[1:]
        # A real click there: the press and the release are handed at the cursor, the client's cursor is that
        # place (it reports it when the settings the button opens let the pointer go), and the button opens.
        before = page.evaluate("handed.length")
        mouse.click()
        released, deadline = None, time.time() + 120
        while not released and time.time() < deadline:
            page.wait_for_timeout(500)
            released = next((m for c in console if (m := re.search(r"cursor released at (\d+),(\d+)", c))), None)
        presses = [h for h in page.evaluate(f"handed.slice({before})") if h[0] != "pointermove"]
        if not (presses and all(h[1:] == at for h in presses)):
            failures.append(f"the press and the release are at the cursor {at}: {presses}")
        got = released and [int(released.group(1)), int(released.group(2))]
        if not released:
            failures.append(f"a click with the cursor drawn on the dashboard button ({at}) opens the settings: it did not")
        elif got != [round(v) for v in at]:
            failures.append(f"the client's cursor at the click is where the cursor is drawn, {at}, not {got}")
        failures += problems
        assert not failures, "\n".join(failures)
        print(f"cursor input: the cursor moves as far as the mouse ({moved}); a real click lands where it is drawn ({got})")
        browser.close()
        server.shutdown()
    mouse.close()
