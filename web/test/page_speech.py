"""`--speech-page`: a replay build with --speech-check --face-trace.
"""

import base64
import pathlib
import re
from playwright.sync_api import sync_playwright

from browser import HERE, opened, wait_console

FLAG = "--speech-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (120, ('--speech-page',), dict(type=pathlib.Path, help="a replay build with --speech-check --face-trace")),
)
ORDER = 30


def run(args):
    check_speech_page(args.speech_page)


def check_speech_page(directory):
    """A queued voice leaves the drawn mouth still until the audio context can run."""
    with sync_playwright() as p:
        server, base, browser, page, console, problems = opened(
            p, directory, args=("--autoplay-policy=document-user-activation-required",),
        )
        try:
            # Runtime evaluations and screenshots must not grant implicit activation.
            session = page.context.new_cdp_session(page)

            def audio_state():
                return session.send("Runtime.evaluate", {
                    "expression": "dbota_audio_state()", "returnByValue": True, "userGesture": False,
                })["result"].get("value")

            page.goto(base + "?cache=http&gpu=webgl2")
            wait_console(page, console, r"speech-check: queued", 180, "a resident voice to queue")
            rectangle = wait_console(
                page, console, r"speech-check: crop ([0-9.]+) ([0-9.]+) ([0-9.]+) ([0-9.]+)",
                60, "the visible portrait bounds",
            )
            x, y, w, h = map(float, rectangle.groups())
            clip = {"x": x + 0.20 * w, "y": y + 0.42 * h, "width": 0.60 * w, "height": 0.42 * h}
            state = audio_state()
            assert state == "suspended", f"before the first gesture: expected suspended audio, got {state}"
            def snapshot():
                return session.send("Page.captureScreenshot", {
                    "format": "png", "clip": {**clip, "scale": 1},
                })["data"]
            frames = lambda: [match[0] for line in console if (
                match := re.search(r"face: 0 \S+(?: -?\d+\.\d+)+", line)
            )]
            def more_frames(count):
                for _ in range(120):
                    if len(frames()) >= count:
                        return
                    page.wait_for_timeout(50)
                raise AssertionError(f"expected {count} drawn frames, got {len(frames())}")

            # Give the newly created portrait texture time to reach the browser's canvas.
            # Check the submitted mouth weights from the moment the voice was queued.
            rest_count = len(frames())
            more_frames(rest_count + 3)
            still = snapshot()
            more_frames(rest_count + 6)
            before_gesture = snapshot()
            resting = frames()[rest_count:]
            assert len(set(resting)) == 1 and resting[0].startswith("face: 0 - "), (
                "the drawn mouth moved before the first gesture", resting[-4:]
            )
            assert audio_state() == "suspended"
            assert not any("speech-check: presented" in line for line in console)
            if before_gesture != still:
                import base64
                scratch = HERE.parent.parent / "temp"
                (scratch / "speech-before-first.png").write_bytes(base64.b64decode(still))
                (scratch / "speech-before-second.png").write_bytes(base64.b64decode(before_gesture))
                raise AssertionError("the mouth pixels changed before the first gesture")

            page.mouse.click(1240, 300)
            wait_console(page, console, r"speech-check: presented [0-9.]+", 30, "the first presented audio")
            images = {still}
            for _ in range(8):
                images.add(snapshot())
                page.wait_for_timeout(25)
            positions = [float(m[1]) for line in console
                         if (m := re.search(r"speech-check: presented ([0-9.]+)", line))]
            assert audio_state() == "running"
            assert len(positions) > 1 and positions[-1] - positions[0] > 0.10, positions
            assert len(images) >= 3, "the rendered mouth region did not change during playback"
            assert len(set(frames())) > 2, "the drawn morph weights did not change"
            assert not problems, problems
            print(f"browser audio: zero pre-gesture mouth changes, {len(images)} distinct mouth crops; "
                  f"presentation advanced {positions[-1] - positions[0]:.3f} s")
        finally:
            browser.close()
            server.shutdown()
