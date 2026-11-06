"""The actual browser cache transport rejects bad indexes before publishing any assets.

Build the release importer with its web feature and wasm-bindgen into
DBOTA_INDEX_PAGE/import, and copy web/cache.js beside it. No game or install is needed.
"""
import functools
import http.server
import os
import pathlib
import threading
import unittest

from playwright.sync_api import sync_playwright

REPOSITORY = pathlib.Path(__file__).resolve().parents[2]


class CacheIndexTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        directory = pathlib.Path(os.environ["DBOTA_INDEX_PAGE"])
        handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(directory))
        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.playwright = sync_playwright().start()
        cls.browser = cls.playwright.chromium.launch(args=["--no-sandbox"])
        cls.page = cls.browser.new_page()
        cls.page.goto(f"http://127.0.0.1:{cls.server.server_port}/")
        import re
        source = (REPOSITORY / "crates/dbota_import/src/import.rs").read_text()
        revision = re.search(r"pub const IMPORT_REV: u32 = (\d+)", source).group(1)
        cls.header = f"dbota-cache 1 rev {revision}\n"

    @classmethod
    def tearDownClass(cls):
        cls.browser.close()
        cls.playwright.stop()
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=5)

    def setUp(self):
        self.page.reload()
        self.page.evaluate("async () => { const module = await import('./import/dbota_import.js'); await module.default(); }")

    def test_direct_export_rejects_oversized_input_before_copying_into_wasm(self):
        result = self.page.evaluate("""async () => {
          const module = await import('./import/dbota_import.js');
          const wasm = await module.default();
          const bytes = new Uint8Array(64 * 1024 * 1024);
          const before = wasm.memory.buffer.byteLength;
          try { new module.CacheIndex(bytes); return {accepted: true}; }
          catch (e) {
            return {accepted: false, error: String(e), growth: wasm.memory.buffer.byteLength - before};
          }
        }""")
        self.assertFalse(result["accepted"], result)
        self.assertIn("at most 2097152", result["error"])
        self.assertLess(result["growth"], 2 * 1024 * 1024, result)

    def test_served_indexes_reject_corruption_without_publishing_assets(self):
        valid = self.header + "file 1 icons/items/blink.png\n"
        cases = [
            ("wrong header", "wrong\nfile 1 icons/items/blink.png\n", "index.txt starts"),
            ("traversal", self.header + "file 1 icons/items/../../outside.png\n", "file it must not"),
            ("duplicate", valid + "file 1 icons/items/blink.png\n", "again"),
            ("flag", self.header + "file 1 icons/items/blink.png small\n", "marks"),
            ("negative size", self.header + "file -1 icons/items/blink.png\n", "cannot read"),
            ("oversized", valid + " " * (2 * 1024 * 1024), "2097152"),
        ]
        for scenario, text, error in cases:
            with self.subTest(scenario=scenario):
                result = self.page.evaluate("""async (text) => {
                  const cache = await import('./cache.js');
                  delete globalThis.dbota_cache;
                  globalThis.fetch = async () => new Response(text);
                  try { await cache.publishServed(); return {published: !!globalThis.dbota_cache}; }
                  catch (e) { return {published: !!globalThis.dbota_cache, error: String(e)}; }
                }""", text)
                self.assertFalse(result["published"], scenario)
                self.assertIn(error, result.get("error", ""), result)

    def test_streamed_index_is_cancelled_at_the_byte_limit(self):
        result = self.page.evaluate("""async () => {
          const cache = await import('./cache.js');
          let sent = 0, cancelled = false;
          globalThis.fetch = async () => new Response(new ReadableStream({
            pull(controller) { if (sent === 40) { controller.close(); return; } sent++; controller.enqueue(new Uint8Array(65536)); },
            cancel() { cancelled = true; }
          }, {highWaterMark: 0}));
          try { await cache.publishServed(); return {published: true}; }
          catch (e) { return {published: !!globalThis.dbota_cache, error: String(e), sent, cancelled}; }
        }""")
        self.assertFalse(result["published"], result)
        self.assertTrue(result["cancelled"], result)
        self.assertLessEqual(result["sent"], 33)
        self.assertIn("exceeds", result["error"])

    def test_private_metadata_limit_is_checked_before_reading_the_file(self):
        result = self.page.evaluate("""async () => {
          const cache = await import('./cache.js');
          let reads = 0;
          Object.defineProperty(navigator.storage, 'getDirectory', {configurable: true, value: async () => ({
            getDirectoryHandle: async () => ({getFileHandle: async (name) => ({
              getFile: async () => name === 'stamp.txt' ? new File(['stamp'], name) : ({
                size: 2 ** 32, arrayBuffer: async () => { reads++; return new ArrayBuffer(0); }
              })
            })})
          })});
          try { await cache.readState(); return {reads}; }
          catch (e) { return {reads, error: String(e)}; }
        }""")
        self.assertEqual(result["reads"], 0, result)
        self.assertIn("at most", result.get("error", ""), result)

    def test_valid_served_and_private_indexes_publish_identical_files(self):
        text = self.header + "file 1 icons/items/blink.png\nfile 2 text/labels.json small\n"
        result = self.page.evaluate("""async (text) => {
          const cache = await import('./cache.js');
          globalThis.fetch = async () => new Response(text);
          await cache.publishServed();
          const served = [...globalThis.dbota_cache.files];
          Object.defineProperty(navigator.storage, 'getDirectory', {configurable: true, value: async () => ({
            getDirectoryHandle: async () => ({getFileHandle: async (name) => ({
              getFile: async () => new File([name === 'stamp.txt' ? 'stamp' : text], name)
            })})
          })});
          const state = await cache.readState();
          return {served, private: [...state.files], small: state.small};
        }""", text)
        self.assertEqual(result["served"], [["icons/items/blink.png", 1], ["text/labels.json", 2]])
        self.assertEqual(result["private"], result["served"])
        self.assertEqual(result["small"], ["text/labels.json"])


if __name__ == "__main__":
    unittest.main()
