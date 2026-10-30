// The browser's replays: the player's own matches, kept by the client in this browser's private
// storage, apart from the copy of the Dota assets (so deleting that copy keeps the replays).
// The Origin Private File System where it can write from a page, else IndexedDB; neither (a
// private window of some browsers) and the client keeps none. The client decides what is kept
// and for how long (src/replays); this file only stores, lists, reads, removes, hands a replay
// to the player as a download, and takes one in from a file the player picks.
// Published as `globalThis.dbota_replays`, which the client reads (src/replays/browser.rs).

export const FOLDER = "dbota-replays";
// The longest replay read from a picked file, as the client's own limit.
const MAX_BYTES = 64 * 2 ** 20;
const INDEX = "index.txt";

// A store over the Origin Private File System: one folder of files.
async function opfs() {
  if (!(navigator.storage && navigator.storage.getDirectory)) return null;
  if (typeof FileSystemFileHandle === "undefined" || !FileSystemFileHandle.prototype.createWritable) return null;
  const dir = await (await navigator.storage.getDirectory()).getDirectoryHandle(FOLDER, { create: true });
  return {
    kind: "opfs",
    async list() {
      const files = [];
      for await (const [name, handle] of dir.entries()) {
        if (handle.kind !== "file") continue;
        const file = await handle.getFile();
        files.push({ name, size: file.size, modified: Math.floor(file.lastModified / 1000) });
      }
      return files;
    },
    async read(name) {
      try {
        return new Uint8Array(await (await (await dir.getFileHandle(name)).getFile()).arrayBuffer());
      } catch (e) {
        if (e && (e.name === "NotFoundError" || e.name === "TypeMismatchError")) return null;
        throw e;
      }
    },
    async write(name, bytes) {
      const writable = await (await dir.getFileHandle(name, { create: true })).createWritable();
      try {
        await writable.write(bytes);
      } catch (e) {
        await writable.abort().catch(() => {});
        // A write that did not fit leaves no half of a replay.
        await dir.removeEntry(name).catch(() => {});
        throw e;
      }
      await writable.close();
    },
    async remove(name) {
      try {
        await dir.removeEntry(name);
      } catch (e) {
        if (!e || e.name !== "NotFoundError") throw e;
      }
    },
  };
}

// The same over IndexedDB, for a browser that cannot write OPFS files from a page.
async function idb() {
  if (!globalThis.indexedDB) return null;
  const db = await new Promise((resolve, reject) => {
    const open = indexedDB.open(FOLDER, 1);
    open.onupgradeneeded = () => open.result.createObjectStore("files", { keyPath: "name" });
    open.onsuccess = () => resolve(open.result);
    open.onerror = () => reject(open.error);
  });
  const run = (mode, make) =>
    new Promise((resolve, reject) => {
      const tx = db.transaction("files", mode);
      const request = make(tx.objectStore("files"));
      tx.oncomplete = () => resolve(request && request.result);
      tx.onerror = tx.onabort = () => reject(tx.error);
    });
  return {
    kind: "idb",
    async list() {
      const rows = await run("readonly", (s) => s.getAll());
      return rows.map((r) => ({ name: r.name, size: r.bytes.byteLength, modified: r.modified }));
    },
    async read(name) {
      const row = await run("readonly", (s) => s.get(name));
      return row ? new Uint8Array(row.bytes) : null;
    },
    write: (name, bytes) =>
      run("readwrite", (s) => s.put({ name, bytes: bytes.slice().buffer, modified: Math.floor(Date.now() / 1000) })),
    remove: (name) => run("readwrite", (s) => s.delete(name)),
  };
}

async function open() {
  for (const make of [opfs, idb]) {
    try {
      const store = await make();
      if (store) return store;
    } catch (e) {
      console.warn("dbota: replay storage unavailable:", e);
    }
  }
  return null;
}

const text = (bytes) => new TextDecoder().decode(bytes);

// Hands bytes to the player as a file download.
function save(name, bytes) {
  const url = URL.createObjectURL(new Blob([bytes], { type: "application/octet-stream" }));
  const link = Object.assign(document.createElement("a"), { href: url, download: name });
  document.body.append(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 60000);
}

// Asks the player for a replay file; its name and bytes, or null when none was chosen.
function pick() {
  return new Promise((resolve, reject) => {
    const input = Object.assign(document.createElement("input"), { type: "file", accept: ".brp" });
    input.style.display = "none";
    const done = (value) => {
      input.remove();
      resolve(value);
    };
    input.addEventListener("cancel", () => done(null));
    input.addEventListener("change", async () => {
      const file = input.files && input.files[0];
      if (!file) return done(null);
      if (file.size > MAX_BYTES) {
        input.remove();
        return reject(new Error("it is longer than a replay may be"));
      }
      try {
        done({ name: file.name, bytes: new Uint8Array(await file.arrayBuffer()) });
      } catch (e) {
        input.remove();
        reject(e);
      }
    });
    document.body.append(input);
    input.click();
  });
}

// The store, if there is one, as the client uses it. Every call waits for the ones before it,
// so the client's order (a replay, then the index that names it) is the order they happen in.
export async function publish() {
  const store = await open();
  let last = Promise.resolve();
  const queued = (work) => {
    const result = last.then(work);
    last = result.catch(() => {});
    return result;
  };
  globalThis.dbota_replays = {
    kind: store ? store.kind : "none",
    load: () =>
      queued(async () => {
        if (!store) throw new Error("this browser keeps nothing");
        const files = await store.list();
        const index = files.some((f) => f.name === INDEX) ? text(await store.read(INDEX)) : null;
        return { index, files: files.filter((f) => f.name !== INDEX) };
      }),
    put: (name, bytes) => queued(() => store.write(name, bytes)),
    get: (name) => queued(() => store.read(name)),
    remove: (names) => queued(async () => void (await Promise.all(names.map((n) => store.remove(n))))),
    index: (body) => queued(() => store.write(INDEX, new TextEncoder().encode(body))),
    download: (name) =>
      queued(async () => {
        const bytes = await store.read(name);
        if (!bytes) throw new Error("that replay is not kept any more");
        save(name, bytes);
      }),
    pick,
  };
}
