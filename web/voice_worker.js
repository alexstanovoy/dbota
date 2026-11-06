// Reads the voice pack of the hero the client speaks for off the page's thread, from the
// page's private storage or from next to the page, checks it whole with the importer's own
// reader (dbota_import's voice_pack_records, the native client's code) and hands each record
// back to the page by itself, its buffer moved, not copied (voice.js).
//
// Message in:  {hero, path, url, mode}: the pack's cache path, where a served cache has it, and
//              the cache's mode ("opfs" or "http")
// Message out: {hero, records: [[name, ArrayBuffer]]} | {hero, error}
import { importer, readCache } from "./cache.js";

// A served pack, read whole but no more than `limit` bytes.
async function served(url, limit) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (bytes.byteLength > limit) throw new Error(`${url} is ${bytes.byteLength} bytes, at most ${limit}`);
  return bytes;
}

self.onmessage = async ({ data: { hero, path, url, mode } }) => {
  try {
    const module = await importer();
    const limit = module.voice_pack_limit();
    const bytes = mode === "opfs" ? await readCache(path, limit) : await served(url, limit);
    if (!bytes) throw new Error(`${path} is not in the cache`);
    const records = module.voice_pack_records(hero, bytes).map(([name, start, end]) => [name, bytes.slice(start, end).buffer]);
    self.postMessage({ hero, records }, records.map(([, buffer]) => buffer));
  } catch (e) {
    self.postMessage({ hero, error: String((e && e.message) || e) });
  }
};
