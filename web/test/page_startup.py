"""`--startup-page` (any build): the page loads the WebGPU client, or the WebGL2 one with
`?gpu=webgl2`, and each draws; a renderer that cannot be made (the page then offers the WebGL2 client, which
runs) and a client out of memory each end with a message on the page.
"""

import pathlib
from playwright.sync_api import sync_playwright

from browser import assert_launch_labels_hidden, opened, wait_console

FLAG = "--startup-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (100, ('--startup-page',), dict(type=pathlib.Path, help="any build: the client picked, failures shown")),
)
ORDER = 50


def run(args):
    check_startup_page(args.startup_page)


NO_GRAPHICS = """
  delete Navigator.prototype.gpu;
  const get = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function (type, options) {
    return type === "webgl2" ? null : get.call(this, type, options);
  };"""
# A WebGPU adapter but no WebGPU canvas: the client's renderer cannot be made.
NO_WEBGPU_CANVAS = """
  const get = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function (type, options) {
    return type === "webgpu" ? null : get.call(this, type, options);
  };"""


def check_startup_page(directory):
    """The page picks the client the browser can draw with, and a start that fails says so on the
    page: no graphics (the page's screen of what the browser lacks), a renderer that cannot be made
    (offering the WebGL2 client), no memory."""
    def run(query, init=None, args=()):
        with sync_playwright() as p:
            server, base, browser, page, console, problems = opened(p, directory, args)
            if init:
                page.add_init_script(init)
            page.goto(base + query)
            yield page, console, problems
            browser.close()
        server.shutdown()

    def failed(page, title):
        page.wait_for_selector("#failed:not([hidden])", timeout=600000)
        got = page.inner_text("#failTitle")
        assert title in got, got

    for query, api in (("?cache=http", "webgpu"), ("?cache=http&gpu=webgl2", "webgl2")):
        for page, console, problems in run(query):
            wait_console(page, console, "dbota: drawing with " + api, 60, f"the {api} client")
            wait_console(page, console, "dbota: running", 600, f"the {api} client's first frame")
            assert_launch_labels_hidden(page)
            assert not problems, problems
            print(f"{api}: the client runs")
    for page, console, problems in run("?cache=http", NO_GRAPHICS):
        page.wait_for_selector("#unsupported", state="visible", timeout=600000)
        assert "WebGPU or WebGL 2" in page.inner_text("#lacks"), page.inner_text("#lacks")
        assert not any("drawing with" in c for c in console), "no client is loaded without graphics"
        print("no graphics: said on the page")
    for page, console, problems in run("?cache=http", NO_WEBGPU_CANVAS):
        failed(page, "could not start its graphics")
        assert "wgpu surface" in page.text_content("#failDetail"), "the panic is in the details"
        page.click("#otherApi")
        wait_console(page, console, "dbota: drawing with webgl2", 60, "the WebGL2 client offered instead")
        wait_console(page, console, "dbota: running", 600, "the WebGL2 client's first frame")
        assert "gpu=webgl2" in page.url, page.url
        print("renderer not made: said on the page, the WebGL2 client runs instead")
    # The client's memory capped at 96 MiB: the menu needs more.
    for page, console, problems in run("?cache=http", args=["--js-flags=--wasm-max-mem-pages=1536"]):
        failed(page, "ran out of memory")
        print("out of memory: said on the page")
