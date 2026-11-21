"""`--ui-art-page`: a match replay with Shadow Fiend's HUD and an active item tooltip.
"""

import base64
import pathlib
from playwright.sync_api import sync_playwright

from browser import HERE, opened, wait_loading_cover

FLAG = "--ui-art-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (170, ('--ui-art-page',), dict(type=pathlib.Path, help="a match replay with Shadow Fiend's HUD and an active item tooltip")),
)
ORDER = 20


def run(args):
    check_ui_art_page(args.ui_art_page)


# Interior samples at the smoke viewport's default HUD scale and unpositioned cursor.
# The two left samples are from the D and F ability slots. The strip sample avoids its label.
HUD_ART_REGIONS = (
    ("Presence of the Dark Lord", (650, 635, 22, 25)),
    ("Feast of Souls", (682, 635, 23, 25)),
    ("Magic Stick active strip", (130, 175, 60, 11)),
)


def check_ui_art_page(directory):
    """Both hero icons and the open item tooltip colour the rendered match canvas."""
    with sync_playwright() as p:
        server, base, browser, page, console, problems = opened(p, directory)
        try:
            page.goto(base + "?cache=http&gpu=webgl2")
            wait_loading_cover(page, console)
            assert any("dbota: drawing with webgl2" in line for line in console), console
            screenshot = page.locator("#dbota").screenshot(
                path=str(HERE.parent.parent / "temp/smoke-ui-art.png"), timeout=60000,
            )
            samples = page.evaluate("""async ({png, regions}) => {
                const frame = new Image();
                frame.src = 'data:image/png;base64,' + png;
                await frame.decode();
                const canvas = document.createElement('canvas');
                canvas.width = frame.width;
                canvas.height = frame.height;
                const ctx = canvas.getContext('2d');
                ctx.drawImage(frame, 0, 0);
                return regions.map(([x, y, width, height], region) => {
                    const rgba = ctx.getImageData(x, y, width, height).data;
                    let coloured = 0;
                    for (let at = 0; at < rgba.length; at += 4) {
                        const [r, g, b, a] = rgba.subarray(at, at + 4);
                        const red = r >= 25 && r >= Math.max(g, b) + 12;
                        const blue = b >= 58 && b >= r + 24 && b >= g + 20;
                        if (a === 255 && (region === regions.length - 1 ? blue : red)) coloured++;
                    }
                    return [coloured, width * height];
                });
            }""", {"png": base64.b64encode(screenshot).decode(), "regions": [rect for _, rect in HUD_ART_REGIONS]})
            for (name, _), (coloured, total) in zip(HUD_ART_REGIONS, samples, strict=True):
                assert coloured * 25 >= total, f"{name} art colour in {coloured}/{total} pixels"
            assert not problems, problems
            print(f"match HUD: coloured pixels in both icons and the active tooltip strip {samples}")
        finally:
            browser.close()
            server.shutdown()
