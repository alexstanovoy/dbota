// Getting at the player's Dota 2 install: the files the importer reads, as `File` objects
// that stay on this machine (nothing is read until it is sliced). The importer needs the
// package files of `game/dota`: `pak01_dir.vpk`, the numbered parts and `maps/*.vpk`.
// Also getting at a prepared asset cache someone else made (a .zip, or its folder).

const DB = "dbota-grant";
const KEY = "dota";

export const canPick = () => typeof showDirectoryPicker === "function";

const isPack = (name) => name.endsWith(".vpk");

// `game/dota` inside a picked folder: the folder itself, or `dota`, or `game/dota` below it.
async function findDota(handle) {
  for (const steps of [[], ["dota"], ["game", "dota"]]) {
    try {
      let dir = handle;
      for (const step of steps) dir = await dir.getDirectoryHandle(step);
      await dir.getFileHandle("pak01_dir.vpk");
      return dir;
    } catch (e) {
      if (!e || (e.name !== "NotFoundError" && e.name !== "TypeMismatchError")) throw e;
    }
  }
  return null;
}

// The package files below a folder handle: Map of path (relative to game/dota) to File.
export async function filesOf(handle) {
  const dota = await findDota(handle);
  if (!dota) return new Map();
  const files = new Map();
  for await (const [name, entry] of dota.entries()) {
    if (entry.kind === "file" && isPack(name)) files.set(name, await entry.getFile());
    if (entry.kind === "directory" && name === "maps") {
      for await (const [inner, file] of entry.entries()) {
        if (file.kind === "file" && isPack(inner)) files.set("maps/" + inner, await file.getFile());
      }
    }
  }
  return files;
}

// A folder picked with the browser's directory picker (Chromium), remembered for later.
export async function pickFolder() {
  const handle = await showDirectoryPicker({ id: "dota", mode: "read" });
  const files = await filesOf(handle);
  if (files.size) await remember(handle).catch(() => {});
  return files;
}

// A folder picked through `<input webkitdirectory>` (every browser): its FileList holds every
// file below the folder, so only the package files are kept, relative to the folder that has
// `pak01_dir.vpk` closest to the top.
export function filesFromList(list) {
  let base = null;
  for (const file of list) {
    const path = file.webkitRelativePath || file.name;
    if (path.split("/").pop() !== "pak01_dir.vpk") continue;
    const dir = path.slice(0, path.length - "pak01_dir.vpk".length);
    if (base === null || dir.length < base.length) base = dir;
  }
  const files = new Map();
  if (base === null) return files;
  for (const file of list) {
    const path = file.webkitRelativePath || file.name;
    if (!path.startsWith(base) || !isPack(path)) continue;
    const rel = path.slice(base.length);
    if (!rel.includes("/") || (rel.startsWith("maps/") && rel.split("/").length === 2)) files.set(rel, file);
  }
  return files;
}

// Opens the fallback picker; resolves with the files, or an empty map when it is dismissed.
export function pickWithInput(input) {
  return new Promise((resolve) => {
    input.value = "";
    input.onchange = () => resolve(filesFromList(input.files));
    input.oncancel = () => resolve(new Map());
    input.click();
  });
}

// A prepared asset cache: the .zip file or the folder picked through an `<input type=file>`.
// Resolves with `{zip: File}` or `{files: [[path, File], ...]}` (paths as the picker names
// them, the picked folder first), or null when the picker is dismissed or holds nothing.
export function pickPrepared(input) {
  return new Promise((resolve) => {
    input.value = "";
    input.onchange = () => {
      const list = [...input.files];
      if (!list.length) return resolve(null);
      resolve(input.webkitdirectory ? { files: list.map((f) => [f.webkitRelativePath || f.name, f]) } : { zip: list[0] });
    };
    input.oncancel = () => resolve(null);
    input.click();
  });
}

const open = () =>
  new Promise((resolve, reject) => {
    const request = indexedDB.open(DB, 1);
    request.onupgradeneeded = () => request.result.createObjectStore("handles");
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });

const store = async (mode, run) => {
  const db = await open();
  return new Promise((resolve, reject) => {
    const request = run(db.transaction("handles", mode).objectStore("handles"));
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
};

export const remember = (handle) => store("readwrite", (s) => s.put(handle, KEY));
export const forget = () => store("readwrite", (s) => s.delete(KEY));

// The remembered folder, when the browser still lets this page read it without asking.
export async function remembered({ ask = false } = {}) {
  if (!canPick()) return null;
  const handle = await store("readonly", (s) => s.get(KEY)).catch(() => null);
  if (!handle) return null;
  let state = await handle.queryPermission({ mode: "read" });
  if (state !== "granted" && ask) state = await handle.requestPermission({ mode: "read" });
  return state === "granted" ? handle : null;
}
