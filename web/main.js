// The page: asks for the player's Dota 2 folder (or a prepared asset cache), has the worker
// build the private asset cache from it (or check and copy the prepared one), then starts the
// client (dbota.js, the game compiled to wasm).
import * as cache from "./cache.js";
import * as grant from "./grant.js";
import * as replays from "./replays.js";
import "./keys.js";
import * as pointer from "./pointer.js";

const $ = (id) => document.getElementById(id);
const show = (...ids) => {
  $("booting").hidden = true;
  for (const id of ["grant", "ready", "progress", "failed", "stale"]) $(id).hidden = !ids.includes(id);
  $("gate").hidden = false;
};
const say = (text) => ($("status").textContent = text);
const mib = (n) => (n / 2 ** 20).toFixed(0) + " MB";

// Every wait on the browser has a limit, so the page never waits for ever: a stage that does not
// answer in `ms` ends as an error that names it, and the caller shows what the player can do.
const STORAGE_MS = 20000;
const STALL_MS = 30000;
function within(stage, promise, ms = STORAGE_MS) {
  let timer;
  const late = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${stage} did not answer in ${ms / 1000} seconds`)), ms);
  });
  return Promise.race([promise, late]).finally(() => clearTimeout(timer));
}

// ?cache=http plays what a local server holds under /cache (a developer's own export); ?assets=none
// skips the question and plays with stand-ins; ?gpu=webgl2 takes the WebGL2 client even where
// WebGPU works.
const query = new URLSearchParams(location.search);

// Which client the browser can draw with. Bevy fixes its render paths when it is compiled, so
// the page carries one client per graphics API: WebGPU where the browser gives an adapter, else
// WebGL2. `tried` says why neither, for the message.
async function graphics() {
  const tried = [];
  if (query.get("gpu") === "webgl2") {
    tried.push("WebGPU not asked for (?gpu=webgl2)");
  } else if (!navigator.gpu) {
    tried.push("WebGPU is off or missing");
  } else {
    try {
      if (await within("WebGPU", navigator.gpu.requestAdapter(), 10000)) return { api: "webgpu" };
      tried.push("WebGPU has no graphics adapter");
    } catch (e) {
      tried.push("WebGPU failed: " + ((e && e.message) || e));
    }
  }
  const canvas = document.createElement("canvas");
  let why = "";
  canvas.addEventListener("webglcontextcreationerror", (e) => (why = e.statusMessage || ""));
  const gl = canvas.getContext("webgl2");
  if (gl) {
    gl.getExtension("WEBGL_lose_context")?.loseContext();
    return { api: "webgl2" };
  }
  tried.push("WebGL 2 is unavailable" + (why ? ` (the browser says: ${why})` : ""));
  return { api: null, tried };
}

// The end of a run that went wrong: the page says what happened and what to do, the console
// has the details. Only the first is shown: what follows a panic is its echo (boot.js).
const fail = (title, text, detail) => globalThis.dbota_boot.fail(title, text, detail);

const NO_GPU =
  "In Firefox or LibreWolf, open about:config, set dom.webgpu.enabled to true and reload this page " +
  "(about:support says under Graphics why WebGL is off). In Chrome or Edge, turn on \"Use graphics " +
  "acceleration when available\" in the settings (System).";

function noGraphics(detail) {
  fail(
    "This browser does not let the page use the graphics card",
    "dbota draws with WebGPU or WebGL 2, and the browser gives the page neither. " + NO_GPU,
    detail
  );
}

// The renderer could not be made with the API the browser offered: its graphics, not a bug. A
// WebGPU that fails this way may still leave WebGL2 working: the page offers to load that client.
function graphicsFailed(report) {
  const webgpu = drawing === "webgpu";
  $("otherApi").hidden = !webgpu;
  fail(
    "The browser could not start its graphics for the game",
    `It offers ${webgpu ? "WebGPU" : "WebGL 2"}, but making the game's renderer with it failed, often because ` +
      "of a graphics driver the browser does not fully support. " +
      (webgpu ? "Draw with WebGL 2 instead (below), or update" : "Update") +
      " the graphics driver or try another browser.",
    report
  );
}

// What the client tells the page (src/webpage.rs).
let drawing = null;
let running = false;
let refusal = null;
globalThis.dbota_page = {
  panicked(report) {
    if (!running && /bevy_render.*renderer|wgpu surface|Unable to find a GPU|request_?device/i.test(report)) {
      return graphicsFailed(report);
    }
    fail(
      "The game stopped because of an error in it",
      "This is a bug in dbota, not something you did. Reloading the page usually gets you back; the details " +
        "below (also in the browser's console) help to fix it.",
      report
    );
  },
  refused(requested, heap) {
    refusal = { requested, heap };
    console.warn(`dbota: the browser refused ${mib(requested)} more memory with ${mib(heap)} in use`);
  },
  running() {
    running = true;
    $("starting").hidden = true;
    console.info("dbota: running");
  },
  keepCursor(keep) {
    pointer.keep(keep);
  },
};

// The client stopping without a panic: the browser gave it no more memory, or something else
// ended it (the console has it).
function stopped(error) {
  if (!(error instanceof WebAssembly.RuntimeError)) return;
  const detail = String(error && error.stack ? error.stack : error);
  if (refusal) {
    fail(
      "The game ran out of memory",
      `The browser gave it no more memory (it asked for ${mib(refusal.requested)} with ${mib(refusal.heap)} in use; ` +
        "a page's WebAssembly memory ends at 4 GB, often sooner). Close other tabs and reload; playing with " +
        "stand-ins needs much less.",
      detail
    );
  } else {
    fail("The game stopped", "Reloading the page usually gets you back; the browser's console has the details.", detail);
  }
}
addEventListener("error", (e) => stopped(e.error));
addEventListener("unhandledrejection", (e) => stopped(e.reason));

// The wasm of the client as a response the browser compiles while it arrives, counting the bytes:
// the player sees the download move, and a download that gets nothing for STALL_MS ends as an error.
function downloaded(url, onBytes) {
  const abort = new AbortController();
  let seen = Date.now();
  let stalled = false;
  const watch = setInterval(() => {
    if (Date.now() - seen > STALL_MS) {
      stalled = true;
      abort.abort();
    }
  }, 1000);
  const response = fetch(url, { signal: abort.signal })
    .then((res) => {
      if (!res.ok) throw new Error(`${url} answered ${res.status}`);
      const total = res.headers.get("content-encoding") ? 0 : Number(res.headers.get("content-length")) || 0;
      let got = 0;
      const count = new TransformStream({
        transform(chunk, out) {
          seen = Date.now();
          got += chunk.length;
          onBytes(got, total);
          out.enqueue(chunk);
        },
        flush: () => clearInterval(watch),
      });
      return new Response(res.body.pipeThrough(count), { headers: { "content-type": "application/wasm" } });
    })
    .catch((e) => {
      clearInterval(watch);
      throw stalled ? new Error(`${url} stopped arriving (nothing for ${STALL_MS / 1000} seconds)`) : e;
    });
  return response;
}

const sayStarting = (text) => ($("starting").textContent = text);

async function startGame() {
  $("gate").hidden = true;
  $("starting").hidden = false;
  sayStarting("Starting the game...");
  const slow = setTimeout(() => {
    sayStarting("Still starting... If nothing changes, the browser's console (F12) says why.");
  }, 60000);
  // The client keeps the player's replays in this browser (replays.js); a storage that is
  // missing or does not answer only means it keeps none.
  await within("The replay storage", replays.publish()).catch((e) => console.warn("dbota: no replay storage:", e));
  sayStarting("Checking the graphics...");
  const gpu = await graphics();
  if (!gpu.api) {
    clearTimeout(slow);
    return noGraphics(gpu.tried.join("; "));
  }
  drawing = gpu.api;
  console.info("dbota: drawing with " + gpu.api);
  const wasm = gpu.api === "webgpu" ? "dbota_bg.wasm" : "dbota_webgl2_bg.wasm";
  try {
    const { default: init } = await within("The game's script", import(gpu.api === "webgpu" ? "./dbota.js" : "./dbota_webgl2.js"), STALL_MS * 2);
    const module_or_path = downloaded("./" + wasm, (got, total) =>
      sayStarting(`Downloading the game: ${mib(got)}${total ? " of " + mib(total) : ""}`)
    );
    await init({ module_or_path });
    sayStarting("Starting the game...");
  } catch (e) {
    // Bevy leaves its event loop by throwing; anything else is a real failure.
    if (!String(e).includes("control flow")) {
      clearTimeout(slow);
      if (e instanceof WebAssembly.RuntimeError) return stopped(e);
      fail(
        "The game could not be downloaded or started",
        "The game's files did not arrive or the browser refused them. Check the connection and reload the page; if it keeps " +
          "happening, try another browser. The details below (also in the console) say which file.",
        String((e && e.stack) || e)
      );
    }
  }
}

// Runs the import worker on `message`; resolves with its last message ("done", "checked" or
// "error"), handing the progress ones to `onMessage`.
function runWorker(message, onMessage) {
  const worker = new Worker("./import_worker.js", { type: "module" });
  return new Promise((resolve) => {
    worker.onmessage = ({ data }) => {
      onMessage?.(data);
      if (data.type !== "progress") {
        worker.terminate();
        resolve(data);
      }
    };
    worker.onerror = (e) => {
      worker.terminate();
      resolve({ type: "error", message: e.message || "the import worker failed" });
    };
    $("cancel").onclick = () => worker.postMessage({ op: "stop" });
    worker.postMessage(message);
  });
}

const modifiedOf = (files) => files.get("pak01_dir.vpk")?.lastModified ?? 0;

// The progress screen and what follows a worker's copy: `stage` heads the count, `failed`
// starts the line saying why it did not work out.
async function copyWith(message, stage, failed) {
  navigator.storage.persist?.().catch(() => {});
  say("");
  show("progress");
  const result = await runWorker(message, (m) => {
    if (m.type !== "progress") return;
    $("bar").value = m.total ? m.done / m.total : 0;
    $("stage").textContent = `${stage}: ${m.done} of ${m.total}`;
    $("detail").textContent = m.bytes ? `${m.item} (${mib(m.bytes)} written)` : m.item;
  });
  if (result.type === "error") {
    say(failed + result.message);
    show("grant");
    return null;
  }
  if (result.stopped) {
    say("Import cancelled.");
    show("grant");
    return null;
  }
  return result;
}

async function importFrom(files) {
  if (!files.has("pak01_dir.vpk")) {
    say("That folder has no pak01_dir.vpk. Pick the dota 2 beta folder (game/dota inside it holds the packages).");
    return show("grant");
  }
  const result = await copyWith({ op: "import", files: [...files], modified: modifiedOf(files) }, "Making your private copy", "The import failed: ");
  if (!result) return;
  const list = $("failures");
  list.hidden = !result.failures.length;
  list.replaceChildren(...result.failures.slice(0, 30).map((f) => Object.assign(document.createElement("li"), { textContent: f })));
  await offerReady(
    `Imported ${result.files} files. ${result.failures.length ? result.failures.length + " could not be converted and use stand-ins." : ""}`
  );
}

// A prepared asset cache (`{zip}` or `{files}`, see grant.pickPrepared): the worker checks it
// whole (this build's stamp, the index, the paths and sizes) before it copies anything, and
// refuses with the reason (which build the cache is for) when it is not usable.
async function loadPrepared(prepared) {
  if (!prepared) return;
  const result = await copyWith({ op: "load", ...prepared }, "Copying the prepared cache", "");
  if (!result) return;
  await offerReady(`Loaded ${result.files} files of the prepared asset cache.`);
}

// The cache is there: publish it to the client, then ask or start.
async function offerReady(text) {
  const state = await cache.readState();
  if (!state) return show("grant");
  await within("The browser's private storage", cache.publish(state));
  $("readyText").textContent = text || `Your imported Dota assets are ready (${state.files.size} files).`;
  show("ready");
}

async function pick() {
  say("");
  try {
    const files = grant.canPick() ? await grant.pickFolder() : await grant.pickWithInput($("folder"));
    if (files.size) return importFrom(files);
    say("No Dota 2 packages found there. Pick the dota 2 beta folder.");
  } catch (e) {
    if (!e || e.name !== "AbortError") say("Could not read the folder: " + (e && e.message ? e.message : e));
  }
  show("grant");
}

// This build's asset fingerprint (the cache version, the importer's revision and the manifest's
// hash): a copy's stamp starts with the fingerprint of the dbota that made it, as on the desktop.
async function importerVersion() {
  const { default: init, cache_version } = await import("./import/dbota_import.js");
  await init();
  return cache_version();
}

// A copy another dbota made is not played: the page holds at the question until it is made
// again, in one click where the browser still holds the folder (asked again if need be, which
// the click allows), else from the folder the player picks.
function holdStale(stamp, version) {
  console.info(`dbota: the imported copy is stamped ${stamp}, this build needs assets ${version}: import again`);
  $("staleWhy").textContent = `They were imported by another dbota and may lack what this one draws (it needs assets ${version}).`;
  $("reimport").onclick = async () => {
    const handle = await grant.remembered({ ask: true }).catch(() => null);
    const files = handle ? await grant.filesOf(handle).catch(() => new Map()) : new Map();
    return files.has("pak01_dir.vpk") ? importFrom(files) : pick();
  };
  show("stale");
}

// A remembered folder the browser still lets us read: is the copy as new as the install?
async function checkFresh() {
  const handle = await grant.remembered().catch(() => null);
  if (!handle) return;
  const files = await grant.filesOf(handle).catch(() => new Map());
  if (!files.has("pak01_dir.vpk")) return;
  const result = await runWorker({ op: "check", files: [...files], modified: modifiedOf(files) });
  if (result.type === "checked" && !result.fresh) {
    $("update").hidden = false;
    $("update").onclick = () => importFrom(files);
    say("Dota 2 has changed since the copy was made.");
  }
}

async function main() {
  globalThis.dbota_boot.started();
  $("standins").onclick = startGame;
  $("standinsStale").onclick = startGame;
  $("play").onclick = startGame;
  $("reload").onclick = () => location.reload();
  $("otherApi").onclick = () => {
    const url = new URL(location.href);
    url.searchParams.set("gpu", "webgl2");
    location.assign(url);
  };
  $("pick").onclick = pick;
  $("again").onclick = pick;
  for (const [kind, ids] of [["zip", ["loadZip", "loadZipStale"]], ["folder", ["loadFolder", "loadFolderStale"]]]) {
    for (const id of ids) {
      $(id).onclick = async () => {
        try {
          loadPrepared(await grant.pickPrepared(kind, $("packZip"), $("packFolder")));
        } catch (e) {
          say("Could not read it: " + (e && e.message ? e.message : e));
        }
      };
    }
  }
  $("remove").onclick = async () => {
    await cache.remove();
    await grant.forget().catch(() => {});
    delete globalThis.dbota_cache;
    say("The imported copy is deleted.");
    show("grant");
  };
  if (query.get("assets") === "none") return startGame();
  if (query.get("cache") === "http") {
    await within("The served cache", cache.publishServed());
    return startGame();
  }
  if (!(await cache.supported())) {
    say("This browser has no private storage for assets: playing with stand-ins is possible.");
    return show("grant");
  }
  let state;
  let version;
  try {
    state = await within("The browser's private storage", cache.readState().catch(() => null));
    version = await within("The tool that reads your Dota files", importerVersion(), STALL_MS * 2);
  } catch (e) {
    console.warn("dbota:", e);
    say(`${e.message}. Importing your Dota files is not possible now; playing with stand-ins is.`);
    return show("grant");
  }
  $("version").textContent = `assets ${version}`;
  if (state && !state.stamp.startsWith(version + "/")) return holdStale(state.stamp, version);
  if (state) {
    await offerReady();
    checkFresh();
  } else {
    show("grant");
  }
}

main().catch((e) => {
  say("Something went wrong: " + e);
  show("grant");
});
