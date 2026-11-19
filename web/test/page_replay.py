"""`--replay-page`: the page plays a Zstandard-packed replay built into it (read through to the
seek tick, as the packed file is decoded record by record) and the client says it knows the
match and heard the chat lines, with no read error.
"""

import pathlib
import re
from playwright.sync_api import sync_playwright

from browser import HERE, opened, wait_console

FLAG = "--replay-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (10, ('--replay-page',), dict(type=pathlib.Path, help="a build with a packed replay built in")),
)
ORDER = 90


def run(args):
    check_replay_page(args.replay_page)


def check_replay_page(directory):
    """A packed (Zstandard) replay built into the page is read through and played."""
    with sync_playwright() as p:
        server, base, browser, page, console, problems = opened(p, directory)
        page.goto(base + "?cache=http")
        wait_console(page, console, r"match known: tick \d+", 600, "the client to know the match")
        # The lines arrive as the replay is read through to its seek tick.
        heard = wait_console(page, console, r"chat: ([5-9]|\d\d+) lines heard", 300, "the chat lines of the replay")
        page.wait_for_timeout(3000)
        page.screenshot(path=str(HERE.parent.parent / "temp/smoke-replay.png"), timeout=600000)
        assert any("stands behind the Latin ones" in c for c in console), "the CJK fallback is registered"
        bad = [c for c in console if "cannot be read" in c or "panicked" in c or "RuntimeError" in c]
        assert not bad, bad
        assert not problems, problems
        lines = max(int(m.group(1)) for c in console if (m := re.search(r"chat: (\d+) lines heard", c)))
        assert lines >= 5, f"the packed replay's chat lines: {heard.group(0)}"
        print(f"packed replay: match known, {lines} chat lines heard, no read error")
        browser.close()
    server.shutdown()
