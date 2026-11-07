// The browser's asset cache: Dota's models, textures and sounds converted from the player's
// own install, kept in the Origin Private File System (private to this page, never uploaded).
// `readCache` serves it to the client; the import worker writes it (import_worker.js).

import "./busy.js";

export const ROOT = "dbota";

let rootHandle;
const dirs = new Map();

export async function supported() {
  return !!(navigator.storage && navigator.storage.getDirectory);
}

async function root() {
  rootHandle ??= await navigator.storage.getDirectory();
  return rootHandle;
}

// The directory handle of a cache folder path (`models/hero`), kept once found.
export async function dir(parts, create = false) {
  let key = "";
  let handle = await root();
  for (const part of [ROOT, ...parts]) {
    key += "/" + part;
    let next = dirs.get(key);
    if (!next) {
      next = await handle.getDirectoryHandle(part, { create });
      dirs.set(key, next);
    }
    handle = next;
  }
  return handle;
}

// A cache file's bytes, or null when it is not there.
export async function readCache(path, limit = Infinity) {
  const parts = path.split("/");
  const name = parts.pop();
  try {
    const file = await (await (await dir(parts)).getFileHandle(name)).getFile();
    if (file.size > limit) throw new Error(`${path} is ${file.size} bytes, at most ${limit}`);
    return new Uint8Array(await file.arrayBuffer());
  } catch (e) {
    if (e && (e.name === "NotFoundError" || e.name === "TypeMismatchError")) return null;
    throw e;
  }
}

const decode = (bytes) => (bytes ? new TextDecoder().decode(bytes) : null);

// The importer (dbota_import, wasm), loaded once for the page or the worker: both cache
// transports use its validated index, also used by the native client. Its module and wasm come
// through busy.js; the first caller's `onBusy` is told when a busy server makes them wait.
let indexModule;
export async function importer(onBusy) {
  const busy = globalThis.dbota_busy;
  indexModule ??= busy.import("./import/dbota_import.js", onBusy).then(async (module) => {
    const wasm = busy.wasm(new URL("./import/dbota_import_bg.wasm", import.meta.url), onBusy);
    // An importer already started (a test's) does not wait for it, and its failure is no one's.
    wasm.catch(() => {});
    await module.default({ module_or_path: wasm });
    return module;
  });
  return indexModule;
}

export async function parseIndex(bytes) {
  const { CacheIndex } = await importer();
  const checked = new CacheIndex(bytes);
  try {
    const files = new Map();
    const small = [];
    for (const [path, size, resident] of checked.files()) {
      files.set(path, size);
      if (resident) small.push(path);
    }
    return { files, small };
  } finally {
    checked.free();
  }
}

// The stamp and index are bounded before any whole-file read is attempted.
export async function readState(onBusy) {
  const [stampLimit, indexLimit] = (await importer(onBusy)).cache_metadata_limits();
  const stamp = decode(await readCache("stamp.txt", stampLimit));
  if (!stamp) return null;
  const index = await readCache("index.txt", indexLimit);
  if (!index) return null;
  return { stamp, ...(await parseIndex(index)) };
}

// A response's bytes, read as they arrive and given up past `limit` (a served file is no larger
// than the cache says only if its server is honest); `name` is what the errors call it.
export async function responseBytes(response, limit, name = "index.txt") {
  const length = response.headers.get("content-length");
  if (length !== null && Number(length) > limit) {
    await response.body?.cancel();
    throw new Error(`${name} is ${length} bytes, at most ${limit}`);
  }
  if (!response.body) throw new Error(`${name} has no response body`);
  const reader = response.body.getReader();
  const bytes = new Uint8Array(limit);
  let used = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) return bytes.subarray(0, used);
      if (value.byteLength > limit - used) throw new Error(`${name} exceeds ${limit} bytes`);
      bytes.set(value, used);
      used += value.byteLength;
    }
  } finally {
    await reader.cancel().catch(() => {});
    reader.releaseLock();
  }
}

// Each request for a served cache's index: past this it is given up.
const INDEX_MS = 15000;

// A served cache must carry a usable index too; no missing or malformed index grants files. A
// busy server's 503 is waited out and the index asked for once more (busy.js, which tells
// `onBusy`); whichever answer is taken is read within the same bound.
export async function publishServed(onBusy) {
  delete globalThis.dbota_cache;
  const busy = globalThis.dbota_busy;
  const [, limit] = (await importer(onBusy)).cache_metadata_limits();
  const signal = AbortSignal.timeout(2 * INDEX_MS + busy.MAX_S * 1000);
  const response = await busy.fetch("cache/index.txt", { signal }, onBusy);
  if (!response.ok) throw new Error(`cache/index.txt returned HTTP ${response.status}`);
  const index = await parseIndex(await responseBytes(response, limit));
  globalThis.dbota_cache = { mode: "http", files: index.files };
}

// What the client reads at start: the index, the small files in memory (it reads them
// synchronously), and the reader for the rest.
export async function publish(state) {
  const small = new Map();
  for (const path of state.small) {
    const bytes = await readCache(path);
    if (bytes) small.set(path, bytes);
  }
  // `reads` counts what the client asked for (the page's tests look at it), bounded: the last
  // paths, and which of the cache's files were read at all.
  const reads = { count: 0, last: [], seen: new Set() };
  const read = (path) => {
    reads.count++;
    reads.last.push(path);
    if (reads.last.length > 64) reads.last.shift();
    if (state.files.has(path)) reads.seen.add(path);
    return readCache(path);
  };
  globalThis.dbota_cache = { mode: "opfs", stamp: state.stamp, files: state.files, small, read, reads };
}

export async function remove() {
  dirs.clear();
  try {
    await (await root()).removeEntry(ROOT, { recursive: true });
  } catch (e) {
    if (!e || e.name !== "NotFoundError") throw e;
  }
}

// Room for the import: bytes the browser will let this page store, minus what it has.
export async function room() {
  const e = (await navigator.storage.estimate?.()) ?? {};
  return e.quota ? e.quota - (e.usage ?? 0) : Infinity;
}
