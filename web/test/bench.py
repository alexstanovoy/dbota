#!/usr/bin/env python3
"""Frame times of the browser clients: plays a page built with `--bench` in its arguments in Chromium
on the real GPU, frame rate uncapped, and prints the client's `bench:` line (see
src/framestats.rs) for each graphics API and run.

    DBOTA_WEB_REPLAY=$PWD/match.brp \\
        DBOTA_WEB_ARGS="--replay built-in --seek 12000 --follow 0 --distance 6500 --bench 20" \\
        OUT=temp/bench-page web/build.sh
    PYTHONPATH=temp/pwlib tools/headless.sh python3 web/test/bench.py temp/bench-page [--runs 3]

The browser is a window in tools/headless.sh's private compositor: a headless Chromium paces its frames
at 60 Hz whatever its flags say, which hides every frame under 16.7 ms.

The page plays the native asset cache of this build (`?cache=http`, DBOTA_ASSETS as smoke.py takes it).
In a browser the client runs on one thread: the line's `main_*` and `render_*` fields are the two
halves of each frame's work on it, and `cpu_pct`/`rss_mb` are not known there.
"""
import argparse
import os
import pathlib
import re
import sys
import time

from playwright.sync_api import sync_playwright

from smoke import serve

# The real GPU, not SwiftShader, and nothing pacing the frames: no vsync, no frame rate limit.
ARGS = [
    "--enable-unsafe-webgpu", "--enable-features=Vulkan", "--use-angle=vulkan", "--ignore-gpu-blocklist",
    "--disable-gpu-vsync", "--disable-frame-rate-limit",
    "--disable-features=LocalNetworkAccessChecks,BlockInsecurePrivateNetworkRequests",
]
# What ends a run: the client died. Its other ERROR lines are said on stderr with the result.
FAILED = re.compile(r"panicked|RuntimeError")
# What makes a run no measure: a pipeline the renderer could not make draws nothing of what it
# was for, and a frame that draws less is faster for it.
UNDRAWN = re.compile(r"[Ss]hader compilation failed|[Pp]ipeline .*(failed|error)|Validation Error")


def platform():
    """The window system the browser opens on: the compositor's (Chromium's own guess, its "auto"
    hint, reads the desktop session's type, which a private compositor does not set)."""
    if os.environ.get("WAYLAND_DISPLAY"):
        return ["--ozone-platform=wayland"]
    if os.environ.get("DISPLAY"):
        return ["--ozone-platform=x11"]
    sys.exit("no window system: run inside tools/headless.sh")


def run(p, base, api, size, timeout):
    """One page load until its `bench:` line; returns the line."""
    browser = p.chromium.launch(args=ARGS + platform(), headless=False)
    try:
        width, height = size
        page = browser.new_context(viewport={"width": width, "height": height}).new_page()
        console = []
        page.on("console", lambda m: console.append(m.text))
        page.on("pageerror", lambda e: console.append(f"page error: {e}"))
        page.goto(base + "?cache=http" + ("&gpu=webgl2" if api == "webgl2" else ""))
        deadline = time.time() + timeout
        while time.time() < deadline:
            for line in console:
                if m := re.search(r"bench: .*", line):
                    errors = {c[:300] for c in console if "ERROR" in c}
                    for error in errors:
                        print(f"{api}: logged: {error}", file=sys.stderr)
                    if undrawn := [e for e in errors if UNDRAWN.search(e)]:
                        sys.exit(f"{api}: no measure, the renderer drew less: {undrawn[0]}")
                    return m.group(0)
                if FAILED.search(line) or line.startswith("page error"):
                    sys.exit(f"{api}: {line[:400]}")
            page.wait_for_timeout(500)
        print(*console[-20:], sep="\n  ", file=sys.stderr)
        sys.exit(f"{api}: no bench line in {timeout} s")
    finally:
        browser.close()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("page", type=pathlib.Path, help="a page built with --bench in DBOTA_WEB_ARGS")
    ap.add_argument("--apis", default="webgpu webgl2", help="the clients to time (default: both)")
    ap.add_argument("--runs", type=int, default=1, help="runs of each client")
    ap.add_argument("--size", default="1920x1080", help="the page's viewport, WxH")
    ap.add_argument("--timeout", type=int, default=600, help="seconds a run may take")
    args = ap.parse_args()
    size = tuple(int(n) for n in args.size.split("x"))
    server = serve(args.page)
    base = f"http://127.0.0.1:{server.server_address[1]}/"
    with sync_playwright() as p:
        for _ in range(args.runs):
            for api in args.apis.split():
                print(api, run(p, base, api, size, args.timeout), flush=True)
    server.shutdown()


if __name__ == "__main__":
    main()
