"""The voice worker reads a served hero's voice pack within the pack's own bound: a pack served
larger than that (or without end) is given up at the bound, not read whole into the worker's memory.

Build dbota_import_web and run wasm-bindgen into DBOTA_INDEX_PAGE/import, and copy web/cache.js,
web/busy.js and web/voice_worker.js beside it. No game or install is needed.
"""
import functools
import http.server
import json
import os
import pathlib
import struct
import threading
import unittest

from playwright.sync_api import sync_playwright

# What the endless pack serves at most, in chunks: far past any bound the worker may keep.
ENDLESS_BYTES = 256 << 20
CHUNK = 1 << 20


class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path != "/endless.bin":
            return super().do_GET()
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.end_headers()
        chunk = b"\0" * CHUNK
        try:
            for _ in range(ENDLESS_BYTES // CHUNK):
                self.wfile.write(chunk)
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
        cls.browser = cls.playwright.chromium.launch(args=["--no-sandbox", "--mute-audio"])
        cls.page = cls.browser.new_page()
        cls.page.goto(f"http://127.0.0.1:{cls.server.server_port}/cache.js")

    @classmethod
    def tearDownClass(cls):
        cls.browser.close()
        cls.playwright.stop()
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=5)

    def test_browser_voice_records_return_the_checked_records_byte_ranges(self):
        rules = json.dumps({"hero": "pudge", "class": "npc_dota_hero_pudge", "criteria": [],
                            "rules": [], "events": [], "pruned": {}}).encode()
        records = [(b"rules.json", rules), (b"events.txt", b"")]
        pack = bytearray(b"DBVOICE\x01" + struct.pack("<I", len(records)))
        expected = []
        for name, data in records:
            pack.extend(struct.pack("<H", len(name)) + name + struct.pack("<I", len(data)))
            start = len(pack)
            pack.extend(data)
            expected.append([name.decode(), start, len(pack)])
        result = self.page.evaluate("""async (bytes) => {
          const module = await import('./import/dbota_import.js');
          await module.default();
          return module.voice_pack_records('pudge', new Uint8Array(bytes));
        }""", list(pack))
        self.assertEqual(result, expected)

    def test_browser_voice_records_reject_a_pack_with_the_native_readers_error(self):
        error = self.page.evaluate("""async () => {
          const module = await import('./import/dbota_import.js');
          await module.default();
          try {
            module.voice_pack_records('pudge', new Uint8Array([0, 1, 2, 3]));
          } catch (error) {
            return error.message;
          }
          return null;
        }""")
        self.assertEqual(error, "not a voice pack of this build (made by another dbota build of "
                         "the same import revision?): import the assets again")

    def test_a_served_pack_past_its_bound_is_refused_by_the_worker(self):
        # What the worker ends with, not what the server managed to push into socket buffers before
        # the abort arrived: that count depends on the machine's load, the bound does not.
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
        self.assertEqual(result["error"], f"endless.bin exceeds {result['limit']} bytes", result)

    def test_a_pack_past_its_bound_is_read_no_further_than_the_chunk_that_crosses_it(self):
        # The worker reads a served pack with `responseBytes`. A stream that serves a chunk only when
        # asked for one counts exactly what the reader took, with no network in between.
        result = self.page.evaluate("""async (chunk) => {
          const module = await import('./import/dbota_import.js');
          await module.default();
          const limit = module.voice_pack_limit();
          const { responseBytes } = await import('./cache.js');
          let taken = 0;
          let cancelled = false;
          const body = new ReadableStream(
            {
              pull(controller) { taken += chunk; controller.enqueue(new Uint8Array(chunk)); },
              cancel() { cancelled = true; },
            },
            { highWaterMark: 0 },
          );
          let error = null;
          try {
            await responseBytes(new Response(body), limit, 'endless.bin');
          } catch (e) {
            error = e.message;
          }
          return {limit, taken, cancelled, error};
        }""", CHUNK)
        self.assertEqual(result["error"], f"endless.bin exceeds {result['limit']} bytes", result)
        self.assertEqual(result["limit"] % CHUNK, 0, result)
        self.assertEqual(result["taken"], result["limit"] + CHUNK, result)
        self.assertTrue(result["cancelled"], result)


if __name__ == "__main__":
    unittest.main()
