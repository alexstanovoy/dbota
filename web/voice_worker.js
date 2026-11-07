// Reads the voice pack of the hero the client speaks for off the page's thread, from the
// page's private storage or from next to the page (read as it arrives, given up past the
// pack's bound), checks it whole with the importer's own reader (dbota_import's
// voice_pack_records, the native client's code) and hands the pack to the page with where each
// record is in it, its buffer moved, not copied (voice.js).
//
// Message in:  {hero, path, url, mode}: the pack's cache path, where a served cache has it, and
//              the cache's mode ("opfs" or "http")
// Message out: {hero, buffer, records: [[name, start, end]]} | {hero, error}
import { importer, readCache, responseBytes } from "./cache.js";

// A served pack, no more than `limit` bytes of it read.
async function served(url, limit) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return responseBytes(response, limit, url);
}

self.onmessage = async ({ data: { hero, path, url, mode } }) => {
  try {
    const module = await importer();
    const limit = module.voice_pack_limit();
    const read = mode === "opfs" ? await readCache(path, limit) : await served(url, limit);
    if (!read) throw new Error(`${path} is not in the cache`);
    const records = module.voice_pack_records(hero, read);
    // The pack's own buffer, cut to it: what the page is handed.
    const buffer = read.byteOffset === 0 && read.byteLength === read.buffer.byteLength ? read.buffer : read.slice().buffer;
    self.postMessage({ hero, buffer, records }, [buffer]);
  } catch (e) {
    self.postMessage({ hero, error: String((e && e.message) || e) });
  }
};
