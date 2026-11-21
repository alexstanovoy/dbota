"""`--sound-page`: any build: the audio context is resumed by the first gesture.
"""

import pathlib
import time
from playwright.sync_api import sync_playwright

from browser import opened, wait_console

FLAG = "--sound-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (130, ('--sound-page',), dict(type=pathlib.Path, help="any build: the audio context is resumed by the first gesture")),
)
ORDER = 10


def run(args):
    check_sound_page(args.sound_page)


def check_sound_page(directory):
    """The game's sound is not lost to the autoplay policy: a client started without a user gesture
    (a context the browser leaves suspended; Safari does so even after a gesture that ended before
    the context was made) is resumed by the player's first click or key, and the page says the
    state of its audio contexts (`dbota_audio_state()`) for a player who hears nothing."""
    spy = """
    window.__contexts = [];
    for (const name of ['AudioContext', 'webkitAudioContext']) {
      const Base = window[name];
      if (!Base) continue;
      window[name] = class extends Base { constructor(...a) { super(...a); window.__contexts.push(this); } };
    }
    """
    with sync_playwright() as p:
        server, base, browser, page, console, problems = opened(p, directory)
        page.add_init_script(spy)
        page.goto(base + "?cache=http&gpu=webgl2")
        wait_console(page, console, "dbota: running", 600, "the client's first frame")
        states = lambda: page.evaluate("window.__contexts.map(c => c.state)")
        assert states() == ["suspended"], f"no gesture yet: the context waits, got {states()}"
        page.mouse.click(640, 360)
        deadline = time.time() + 20
        while time.time() < deadline and states() != ["running"]:
            page.wait_for_timeout(250)
        assert states() == ["running"], f"the first click must start the sound, got {states()}"
        said = page.evaluate("globalThis.dbota_audio_state()")
        assert said == "running", f"the console one-liner says the state, got {said!r}"
        assert not problems, problems
        print("sound: a context left suspended is resumed by the first click")
        browser.close()
    server.shutdown()
