// The browser's asset cache: Dota's models, textures and sounds converted from the player's
// own install, kept in the Origin Private File System (private to this page, never uploaded).
// `readCache` serves it to the client; the import worker writes it (import_worker.js).

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

// Both cache transports use the importer's validated index, also used by the native client.
let indexModule;
async function indexApi() {
  indexModule ??= import("./import/dbota_import.js").then(async (module) => {
    await module.default();
    return module;
  });
  return indexModule;
}

export async function parseIndex(bytes) {
  const { CacheIndex } = await indexApi();
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
export async function readState() {
  const [stampLimit, indexLimit] = (await indexApi()).cache_metadata_limits();
  const stamp = decode(await readCache("stamp.txt", stampLimit));
  if (!stamp) return null;
  const index = await readCache("index.txt", indexLimit);
  if (!index) return null;
  return { stamp, ...(await parseIndex(index)) };
}

async function responseBytes(response, limit) {
  const length = response.headers.get("content-length");
  if (length !== null && Number(length) > limit) {
    await response.body?.cancel();
    throw new Error(`index.txt is ${length} bytes, at most ${limit}`);
  }
  if (!response.body) throw new Error("index.txt has no response body");
  const reader = response.body.getReader();
  const bytes = new Uint8Array(limit);
  let used = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) return bytes.subarray(0, used);
      if (value.byteLength > limit - used) throw new Error(`index.txt exceeds ${limit} bytes`);
      bytes.set(value, used);
      used += value.byteLength;
    }
  } finally {
    await reader.cancel().catch(() => {});
    reader.releaseLock();
  }
}

// A served cache must carry a usable index too; no missing or malformed index grants files.
export async function publishServed() {
  delete globalThis.dbota_cache;
  const [, limit] = (await indexApi()).cache_metadata_limits();
  const response = await fetch("cache/index.txt", { signal: AbortSignal.timeout(15000) });
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
