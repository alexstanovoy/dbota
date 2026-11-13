"""The served replay store keeps Watch's bytes without a file-picker/download API."""
import functools
import http.server
import pathlib
import threading
import unittest

from playwright.sync_api import sync_playwright


class Handler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


class ReplayActionsTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        directory = pathlib.Path(__file__).resolve().parents[1]
        cls.server = http.server.ThreadingHTTPServer(
            ("127.0.0.1", 0), functools.partial(Handler, directory=str(directory)))
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.playwright = sync_playwright().start()
        cls.browser = cls.playwright.chromium.launch(args=["--no-sandbox", "--disable-gpu"])
        cls.page = cls.browser.new_page()
        cls.page.goto(f"http://127.0.0.1:{cls.server.server_port}/replays.js")

    @classmethod
    def tearDownClass(cls):
        cls.browser.close()
        cls.playwright.stop()
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=5)

    def test_the_served_store_has_no_file_import_or_download_action(self):
        actions = self.page.evaluate("""async () => {
          const module = await import('./replays.js');
          await module.publish();
          return Object.keys(globalThis.dbota_replays).sort();
        }""")
        self.assertEqual(actions, ["get", "index", "kind", "load", "put", "remove"])

    def test_kept_replay_bytes_survive_reopening_the_store(self):
        result = self.page.evaluate("""async () => {
          const module = await import('./replays.js');
          await module.publish();
          await dbota_replays.put('kept.brp', new Uint8Array([40,181,47,253,7]));
          await module.publish();
          const bytes = Array.from(await dbota_replays.get('kept.brp'));
          await dbota_replays.remove(['kept.brp']);
          return {bytes, removed: await dbota_replays.get('kept.brp')};
        }""")
        self.assertEqual(result, {"bytes": [40, 181, 47, 253, 7], "removed": None})
