"""`--keys-page` (any page, even `web/` unbuilt): over the canvas the browser keeps its Ctrl and
Cmd chords and F12, the game its keys (F5, Tab, Space, letters, right click, wheel); in fullscreen
Chromium's canvas locks Esc (and lets it go on leaving), Firefox's page says once that it cannot.
"""

import pathlib
from playwright.sync_api import sync_playwright

from browser import launch_engine, opened, serve, wait_console

FLAG = "--keys-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (140, ('--keys-page',), dict(type=pathlib.Path, help="any page, even web/ unbuilt: whose keys are whose")),
)
ORDER = 70


def run(args):
    check_browser_keys(args.keys_page)
    check_escape_in_fullscreen(args.keys_page)


ESC_LOCK_SPY = """(() => {
  // Records what the page asks of the Keyboard Lock API and passes it on to the real one.
  globalThis.keyboardLock = [];
  const keyboard = navigator.keyboard;
  if (!keyboard) return;
  for (const name of ['lock', 'unlock']) {
    const real = keyboard[name].bind(keyboard);
    keyboard[name] = (...args) => { keyboardLock.push([name, ...args]); return real(...args); };
  }
  globalThis.escapes = [];
  addEventListener('keydown', (e) => { if (e.key === 'Escape') escapes.push(document.fullscreenElement !== null); }, true);
})()"""


def check_escape_in_fullscreen(directory):
    """Esc is a game key, which a browser takes for leaving fullscreen. Where the Keyboard Lock API
    exists (Chromium) the canvas locks Esc when it goes fullscreen and lets it go when it leaves
    (web/keys.js); where it does not (Firefox) the page says so once and Esc is left to the
    browser. Needs no client, so any page serves. A press of Esc injected by the test never goes
    through the browser's own handling of the key (which leaves fullscreen, or with the lock asks
    for it to be held), so what the player's short press does cannot be seen here: only what the
    page asks for, and that the page sees the key."""
    with sync_playwright() as p:
        server = serve(directory)
        base = f"http://127.0.0.1:{server.server_address[1]}/"
        for engine in ("chromium", "firefox"):
            browser = launch_engine(p, engine)
            page = browser.new_context(viewport={"width": 1280, "height": 720}).new_page()
            console = []
            page.on("console", lambda m: console.append(m.text))
            page.add_init_script(ESC_LOCK_SPY)
            # The load event comes after the page's modules (main.js, which imports keys.js) have run,
            # so F11 below meets its handler; goto's own timeout bounds the wait.
            page.goto(base + "?cache=http", wait_until="load")
            page.wait_for_function("() => document.getElementById('dbota') !== null")
            fullscreen = lambda: page.evaluate("document.fullscreenElement === document.getElementById('dbota')")
            page.keyboard.press("F11")
            page.wait_for_function("() => document.fullscreenElement !== null", timeout=60000)
            if engine == "chromium":
                wait_console(page, console, "Esc is locked to the game while fullscreen", 60, "the browser to grant the Esc lock")
                calls = page.evaluate("keyboardLock")
                assert calls == [["lock", ["Escape"]]], f"the page locks Esc, and only Esc: {calls}"
                page.keyboard.press("Escape")
                assert page.evaluate("escapes") == [True], "the page sees a short Esc in fullscreen"
                # Only a precondition: an injected Esc never reaches the browser's own handling, so this
                # holds with or without the lock. The assertions of this check are the lock's grant above
                # and its release below.
                assert fullscreen(), "the canvas is still fullscreen after the injected Esc"
                page.keyboard.press("F11")
                page.wait_for_function("() => document.fullscreenElement === null", timeout=60000)
                page.wait_for_function("() => keyboardLock.length === 2", timeout=60000)
                calls = page.evaluate("keyboardLock")
                assert calls[1][0] == "unlock", f"the page lets Esc go on leaving fullscreen: {calls}"
                print("escape: Chromium locks Esc in fullscreen and lets it go on leaving")
            else:
                wait_console(page, console, "this browser cannot keep Esc in the game", 60, "the page to say Esc cannot be kept")
                page.keyboard.press("F11")
                page.wait_for_function("() => document.fullscreenElement === null", timeout=60000)
                said = [c for c in console if "cannot keep Esc" in c]
                assert len(said) == 1, f"the page says it once: {said}"
                print("escape: Firefox has no Esc lock, the page says so once")
            browser.close()
    server.shutdown()


KEYS_LOGIC = """() => {
  const canvas = document.getElementById('dbota');
  const kept = (type, init) => {
    const e = type === 'wheel'
      ? new WheelEvent(type, { ...init, cancelable: true, bubbles: true })
      : type === 'contextmenu'
        ? new MouseEvent(type, { ...init, cancelable: true, bubbles: true })
        : new KeyboardEvent(type, { ...init, cancelable: true, bubbles: true });
    canvas.dispatchEvent(e);
    return !e.defaultPrevented;
  };
  return {
    browser: [
      ['Ctrl+R', kept('keydown', { key: 'r', code: 'KeyR', ctrlKey: true })],
      ['Ctrl+T', kept('keydown', { key: 't', code: 'KeyT', ctrlKey: true })],
      ['Ctrl+W', kept('keydown', { key: 'w', code: 'KeyW', ctrlKey: true })],
      ['Ctrl+Tab', kept('keydown', { key: 'Tab', code: 'Tab', ctrlKey: true })],
      ['Cmd+F', kept('keydown', { key: 'f', code: 'KeyF', metaKey: true })],
      ['F12', kept('keydown', { key: 'F12', code: 'F12' })],
    ],
    game: [
      ['F5', kept('keydown', { key: 'F5', code: 'F5' })],
      ['Tab', kept('keydown', { key: 'Tab', code: 'Tab' })],
      ['Space', kept('keydown', { key: ' ', code: 'Space' })],
      ['Q', kept('keydown', { key: 'q', code: 'KeyQ' })],
      ['Q up', kept('keyup', { key: 'q', code: 'KeyQ' })],
      ['right click', kept('contextmenu', { button: 2 })],
      ['wheel', kept('wheel', { deltaY: 100 })],
    ],
  };
}"""


def check_browser_keys(directory):
    """The page's keys (web/keys.js): over the canvas the browser keeps every Ctrl or Cmd chord
    and F12 (reload, tabs, find, tools), and the game keeps its own (F5 is Dota's quick buy, Tab
    and Space are bound, a right click moves, the wheel zooms). Events are made on the canvas
    and whether the page let them through to the browser is read; needs no client, so any page
    (even web/ as it is in the repository) serves."""
    with sync_playwright() as p:
        server, base, browser, page, console, problems = opened(p, directory)
        page.goto(base + "?cache=http")
        page.wait_for_function("document.getElementById('dbota') !== null")
        page.wait_for_timeout(1000)
        got = page.evaluate(KEYS_LOGIC)
        taken = [name for name, kept in got["browser"] if not kept]
        assert not taken, f"the browser's keys taken by the page: {taken}"
        let_through = [name for name, kept in got["game"] if kept]
        assert not let_through, f"the game's keys let through to the browser: {let_through}"
        print("browser keys: the browser keeps its chords and F12, the game its keys")
        browser.close()
    server.shutdown()
