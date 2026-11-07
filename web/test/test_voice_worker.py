"""The voice worker reads a served hero's voice pack within the pack's own bound: a pack served
larger than that (or without end) is given up at the bound, not read whole into the worker's memory.

Build the release importer with its web feature and wasm-bindgen into DBOTA_INDEX_PAGE/import, and
copy web/cache.js, web/busy.js and web/voice_worker.js beside it. No game or install is needed.
"""
import functools
import http.server
import os
import pathlib
import threading
import unittest

from playwright.sync_api import sync_playwright

# What the endless pack serves at most, in chunks: far past any bound the worker may keep.
ENDLESS_BYTES = 256 << 20
CHUNK = 1 << 20


class Handler(http.server.SimpleHTTPRequestHandler):
    sent = 0

    def do_GET(self):
        if self.path != "/endless.bin":
            return super().do_GET()
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.end_headers()
        chunk = b"\0" * CHUNK
        try:
            while Handler.sent < ENDLESS_BYTES:
                self.wfile.write(chunk)
                Handler.sent += CHUNK
        except (BrokenPipeError, ConnectionResetError):
            pass

    def log_message(self, *args):
        pass


class VoiceWorkerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        directory = pathlib.Path(os.environ["DBOTA_INDEX_PAGE"])
        handler = functools.partial(Handler, directory=str(directory))
        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.playwright = sync_playwright().start()
        cls.browser = cls.playwright.chromium.launch(args=["--no-sandbox"])
        cls.page = cls.browser.new_page()
        cls.page.goto(f"http://127.0.0.1:{cls.server.server_port}/cache.js")

    @classmethod
    def tearDownClass(cls):
        cls.browser.close()
        cls.playwright.stop()
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=5)

    def test_a_served_pack_past_its_bound_is_given_up_at_the_bound(self):
        Handler.sent = 0
        result = self.page.evaluate("""async () => {
          const module = await import('./import/dbota_import.js');
          await module.default();
          const limit = module.voice_pack_limit();
          const answer = await new Promise((resolve) => {
            const worker = new Worker('./voice_worker.js', {type: 'module'});
            worker.onmessage = ({data}) => { worker.terminate(); resolve(data); };
            worker.onerror = (e) => resolve({error: 'the worker failed: ' + e.message});
            worker.postMessage({hero: 'pudge', path: 'sounds/voice/pudge.bin', url: 'endless.bin', mode: 'http'});
          });
          return {limit, error: answer.error || null, records: !!answer.records};
        }""")
        self.assertFalse(result["records"], result)
        self.assertIn(f"exceeds {result['limit']} bytes", result["error"] or "", result)
        # Given up at the bound: what the server could still push into the connection's buffers
        # besides, not the whole endless pack.
        self.assertLess(Handler.sent, result["limit"] + (32 << 20), result)


if __name__ == "__main__":
    unittest.main()
