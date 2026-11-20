"""`--services-page`: a build without a replay: checks services before sign-in.
"""

import base64
import pathlib
import re
import subprocess
import sys
import time
from playwright.sync_api import sync_playwright

from browser import BROWSER_ARGS, HERE, radovid_config, serve, wait_console, without_steering_variables

FLAG = "--services-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (220, ('--services-page',), dict(type=pathlib.Path, help="a build without a replay: checks services before sign-in")),
    (230, ('--service-shots',), dict(type=pathlib.Path, help="write the service check's frames to this directory")),
)
ORDER = 90


def run(args):
    if not (args.server and args.radovid):
        sys.exit("--services-page needs --server and --radovid")
    check_service_page(args.services_page, args.server, args.radovid, args.service_shots)


def check_service_page(directory, server_bin, radovid_bin, shots):
    """The browser refuses a saved address with the wrong path on a working service before sign-in."""
    work = HERE.parent.parent / "temp/smoke-services"
    work.mkdir(parents=True, exist_ok=True)
    subprocess.run([radovid_bin, "dev-cert", str(work)], check=True, capture_output=True)
    # Reserve no fixed control port. The unused game and relay ports are never bound: no game starts.
    config = work / "service.toml"
    config.write_text(radovid_config(
        work, directory.resolve(), server_bin, 0, 65535, work / "cert.pem", work / "key.pem", 65535
    ))
    with (work / "service.log").open("w") as log, serve(directory) as page_server:
        service = subprocess.Popen([radovid_bin, "serve", "--config", str(config)],
                                   stderr=log, env=without_steering_variables())
        try:
            endpoint = read_service_address(work / "service.log", service)
            run_service_page(page_server, endpoint, shots)
        finally:
            service.terminate()
            service.wait(timeout=20)
            page_server.shutdown()


def read_service_address(log, service):
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline and service.poll() is None:
        if match := re.search(r"listening on (127\.0\.0\.1:\d+)", log.read_text()):
            return f"wss://{match[1]}"
        time.sleep(0.1)
    raise AssertionError(f"service did not start: {log.read_text()}")


def run_service_page(page_server, endpoint, shots):
    url = f"http://127.0.0.1:{page_server.server_port}/?cache=http"
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(args=[*BROWSER_ARGS, "--ignore-certificate-errors"])
        page = browser.new_page(viewport={"width": 1920, "height": 1080})
        console = []
        page.on("console", lambda message: console.append(message.text))
        page.route_web_socket(re.compile(r"^wss://bota\.lubush\.space/"), lambda connection: connection.close())
        page.goto(url)
        wait_console(page, console, r"service wss://bota\.lubush\.space did not pass its check:", 90,
                     "the new browser client to check the public service")
        capture_service_card(page, failed=True, shots=shots, filename="default-browser")
        page.close()
        for path, result in [("", "passed its check"), ("/wrong", "did not pass its check: CannotConnect")]:
            address = endpoint + path
            context = browser.new_context(viewport={"width": 1920, "height": 1080})
            context.add_init_script(f"localStorage.setItem('dbota.menu', {json.dumps(f'service = {address}')})")
            page = context.new_page()
            console = []
            page.on("console", lambda message: console.append(message.text))
            page.goto(url)
            wait_console(page, console, rf"service {re.escape(address)} {result}", 90, result)
            capture_service_card(page, failed=bool(path), shots=shots)
            context.close()
        browser.close()


def capture_service_card(page, failed, shots, filename=None):
    # The failed check paints a red headline. A passed check paints a sign-in button that
    # reaches the right of the card. The narrow CHECK AGAIN button does not reach there.
    clip = ({"x": 738, "y": 345, "width": 290, "height": 58} if failed else
            {"x": 1000, "y": 425, "width": 140, "height": 137})
    pixels_with_status = """async ([encoded, failed]) => {
        const image = new Image();
        image.src = 'data:image/png;base64,' + encoded;
        await image.decode();
        const canvas = new OffscreenCanvas(image.width, image.height);
        const context = canvas.getContext('2d');
        context.drawImage(image, 0, 0);
        const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
        let count = 0;
        for (let i = 0; i < pixels.length; i += 4) {
            const r = pixels[i], g = pixels[i + 1], b = pixels[i + 2];
            if (failed ? r > 170 && r > g + 55 && r > b + 40
                       : g > 90 && g > r + 25 && g > b + 24) count++;
        }
        return count;
    }"""
    deadline = time.monotonic() + 30
    while True:
        crop = page.screenshot(clip=clip)
        count = page.evaluate(pixels_with_status, [base64.b64encode(crop).decode("ascii"), failed])
        if count >= (40 if failed else 1000):
            break
        assert time.monotonic() < deadline, f"service status pixels: {count}, failure expected: {failed}"
    if shots:
        shots.mkdir(parents=True, exist_ok=True)
        name = filename or ("wrong-path-browser" if failed else "sign-in-browser")
        page.screenshot(path=shots / f"{name}.png")
