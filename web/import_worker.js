// The import worker's start: a worker begins from a script, so this one loads the importer (Rust,
// crates/dbota_import_web) and hands the worker to it, which does the rest. A server too busy to
// send the importer at once is waited out (busy.js) and the client is told of the wait.
import "./busy.js";
import init, { run } from "./import/dbota_import.js";

const told = (seconds) => self.postMessage({ type: "busy", seconds });
await init({ module_or_path: globalThis.dbota_busy.wasm(new URL("./import/dbota_import_bg.wasm", import.meta.url), told) });
run();
