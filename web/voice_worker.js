// Reads the voice pack of the hero the client speaks for off the page's thread, from the copy of
// the assets in the page's private storage or from next to the page (read as it arrives, given up
// past the pack's bound), checks it whole with the importer's own reader (dbota_import's
// voice_pack_records, the native client's code) and hands the pack to the page with where each
// record is in it, its buffer moved, not copied (voice.js).
//
// Message in:  {hero, path, url, folder}: the pack's path in the copy, where a served copy has it,
//              and the copy's folder in private storage (["dbota", "copy-…"]; null: served)
// Message out: {hero, buffer, records: [[name, start, end]]} | {hero, error}
import init, { voice_pack_limit, voice_pack_records } from "./import/dbota_import.js";
import { responseBytes } from "./bounded.js";

let loaded;
const importer = () => (loaded ??= init({ module_or_path: new URL("./import/dbota_import_bg.wasm", import.meta.url) }));

// A served pack, no more than `limit` bytes of it read.
async function served(url, limit) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return responseBytes(response, limit, url);
}

// A pack in private storage, its size checked before it is read; null when it is not there.
async function stored(folder, path, limit) {
  const parts = [...folder, ...path.split("/")];
  const name = parts.pop();
  try {
    let dir = await navigator.storage.getDirectory();
    for (const part of parts) dir = await dir.getDirectoryHandle(part);
    const file = await (await dir.getFileHandle(name)).getFile();
    if (file.size > limit) throw new Error(`${path} is ${file.size} bytes, at most ${limit}`);
    return new Uint8Array(await file.arrayBuffer());
  } catch (e) {
    if (e && (e.name === "NotFoundError" || e.name === "TypeMismatchError")) return null;
    throw e;
  }
}

self.onmessage = async ({ data: { hero, path, url, folder } }) => {
  try {
    await importer();
    const limit = voice_pack_limit();
    const read = folder ? await stored(folder, path, limit) : await served(url, limit);
    if (!read) throw new Error(`${path} is not in the copy of the assets`);
    const records = voice_pack_records(hero, read);
    // The pack's own buffer, cut to it: what the page is handed.
    const buffer = read.byteOffset === 0 && read.byteLength === read.buffer.byteLength ? read.buffer : read.slice().buffer;
    self.postMessage({ hero, buffer, records }, [buffer]);
  } catch (e) {
    self.postMessage({ hero, error: String((e && e.message) || e) });
  }
};
