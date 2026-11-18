// Runs the importer (dbota_import, Rust compiled to wasm) off the page's thread: it reads the
// picked Dota files by slices (FileReaderSync) and writes the converted cache into the Origin
// Private File System with synchronous access handles. Nothing is sent anywhere.
//
// Messages in:  {op: "check" | "import", files: [[path, File]], modified}, and {op: "stop"}
// Messages out: {type: "checked", fresh} | {type: "progress", done, total, item, bytes}
//               | {type: "done", files, failures, deferred, stopped} | {type: "error", message}
//               | {type: "busy", seconds}: the server was busy and is asked again in that long
import { Install } from "./import/dbota_import.js";
import { dir, importer, readState, remove, room } from "./cache.js";

// The least storage an import needs: the converted cache is about 560 MB.
const NEEDED = 1200 * 2 ** 20;

// Cache files are only ever written through here: a path that is not plain is refused before
// it reaches the file system (the importer checks too).
const plain = (path) =>
  /^[A-Za-z0-9_+.-]+(\/[A-Za-z0-9_+.-]+)*$/.test(path) && !path.split("/").some((p) => p === "." || p === "..");

async function writeFile(path, bytes) {
  if (!plain(path)) throw new Error("unsafe path " + path);
  const parts = path.split("/");
  const name = parts.pop();
  const handle = await (await dir(parts, true)).getFileHandle(name, { create: true });
  const sync = await handle.createSyncAccessHandle();
  try {
    sync.truncate(0);
    sync.write(bytes, { at: 0 });
    sync.flush();
  } finally {
    sync.close();
  }
}

let stopping = false;
const post = (message) => self.postMessage(message);

// The importer's wasm (cache.js), fetched once more after a busy server's wait (busy.js), which
// the page is told of.
const loaded = () => importer((seconds) => post({ type: "busy", seconds }));

async function install(files) {
  await loaded();
  const made = new Install();
  for (const [path, file] of files) made.add(path, file);
  return made;
}

async function run(made, stamp) {
  if ((await room()) < NEEDED) throw new Error("the browser has too little storage left for the imported assets (about 600 MB)");
  await remove();
  const job = made.start();
  for (;;) {
    // A message ("stop") is handled while this awaits.
    post({ type: "progress", done: job.done(), total: job.total(), item: job.upcoming() ?? "", bytes: job.bytes() });
    if (stopping) break;
    const files = job.step();
    if (!files) break;
    // A file that cannot be written ends the import: the cache stays unfinished (no index, no
    // stamp), never an index that lists files that are not there.
    for (const [path, bytes] of files) await writeFile(path, bytes);
    await new Promise((r) => setTimeout(r, 0));
  }
  const report = job.finish();
  if (stopping) {
    await remove();
    return post({ type: "done", stopped: true, files: 0, failures: [] });
  }
  // An install that asked for more than an import reads finishes no cache: its files go.
  const cut = report.cut();
  if (cut) {
    await remove();
    throw new Error(cut);
  }
  // The index and the stamp last: a cache without them is not a finished one.
  await writeFile("index.txt", new TextEncoder().encode(report.index()));
  await writeFile("stamp.txt", new TextEncoder().encode(stamp));
  for (const warning of report.warnings()) console.warn(`Asset import: ${warning}`);
  post({ type: "done", stopped: false, files: report.files(), failures: report.failures() });
}

self.onmessage = async ({ data }) => {
  if (data.op === "stop") {
    stopping = true;
    return;
  }
  try {
    const made = await install(data.files);
    const stamp = made.stamp(data.modified);
    if (!stamp) return post({ type: "error", message: "pak01_dir.vpk is not in the chosen folder" });
    if (data.op === "check") {
      const state = await readState();
      return post({ type: "checked", fresh: !!state && state.stamp === stamp });
    }
    await run(made, stamp);
  } catch (e) {
    // Whatever was written is not a cache (it has no index or stamp): let the space go.
    await remove().catch(() => {});
    post({ type: "error", message: String((e && e.message) || e) });
  }
};
