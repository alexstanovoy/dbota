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
export async function readCache(path) {
  const parts = path.split("/");
  const name = parts.pop();
  try {
    const file = await (await (await dir(parts)).getFileHandle(name)).getFile();
    return new Uint8Array(await file.arrayBuffer());
  } catch (e) {
    if (e && (e.name === "NotFoundError" || e.name === "TypeMismatchError")) return null;
    throw e;
  }
}

const decode = (bytes) => (bytes ? new TextDecoder().decode(bytes) : null);

// The stamp (what the cache was made from) and the index of a finished cache, or null.
export async function readState() {
  const [stamp, index] = [decode(await readCache("stamp.txt")), decode(await readCache("index.txt"))];
  if (!stamp || !index) return null;
  const files = new Map();
  const small = [];
  for (const line of index.split("\n")) {
    const [kind, size, path, flag] = line.split(" ");
    if (kind !== "file") continue;
    files.set(path, Number(size));
    if (flag === "small") small.push(path);
  }
  return { stamp, files, small };
}

// What the client reads at start: the index, the small files in memory (it reads them
// synchronously), and the reader for the rest.
export async function publish(state) {
  const small = new Map();
  for (const path of state.small) {
    const bytes = await readCache(path);
    if (bytes) small.set(path, bytes);
  }
  // `reads` counts what the client asked for (the page's tests look at it), bounded.
  const reads = { count: 0, last: [] };
  const read = (path) => {
    reads.count++;
    reads.last.push(path);
    if (reads.last.length > 64) reads.last.shift();
    return readCache(path);
  };
  globalThis.dbota_cache = { mode: "opfs", files: state.files, small, read, reads };
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
