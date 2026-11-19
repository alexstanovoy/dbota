"""`--avatar-picker-page`: any build: opens the browser avatar chooser.
"""

import pathlib
from playwright.sync_api import sync_playwright

from browser import BROWSER_ARGS, serve

FLAG = "--avatar-picker-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (240, ('--avatar-picker-page',), dict(type=pathlib.Path, help="any build: opens the browser avatar chooser")),
)
ORDER = 80


def run(args):
    check_avatar_picker_page(args.avatar_picker_page)


def check_avatar_picker_page(directory):
    """A real browser chooser returns the selected bytes and refuses a file past its limit."""
    server = serve(directory)
    try:
        with sync_playwright() as playwright:
            browser = playwright.chromium.launch(args=BROWSER_ARGS)
            page = browser.new_page()
            page.goto(f"http://127.0.0.1:{server.server_port}/")
            page.wait_for_function("() => typeof globalThis.dbota_page?.pickAvatar === 'function'")
            page.evaluate("""() => {
                const button = document.createElement("button");
                button.id = "avatar-test";
                button.textContent = "Choose image";
                Object.assign(button.style, {position: "fixed", top: "8px", left: "8px", zIndex: "1000"});
                button.onclick = () => {
                    globalThis.pickedImage = dbota_page.pickAvatar(1 << 20).then(
                        bytes => Array.from(bytes),
                        error => String(error),
                    );
                };
                document.body.prepend(button);
            }""")
            for contents, expected in [
                (b"image", list(b"image")),
                (bytes((1 << 20) + 1), "Choose a PNG or JPEG up to 1 MB."),
            ]:
                with page.expect_file_chooser() as chosen:
                    page.click("#avatar-test")
                chosen.value.set_files({"name": "avatar.png", "mimeType": "image/png", "buffer": contents})
                assert page.evaluate("() => globalThis.pickedImage") == expected
                assert page.locator('input[type="file"][accept="image/png,image/jpeg"]').count() == 0
            browser.close()
    finally:
        server.shutdown()
        server.server_close()
