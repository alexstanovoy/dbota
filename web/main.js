// The page: asks for the player's Dota 2 folder (or not), has the worker build the private
// asset cache from it, then starts the client (dbota.js, the game compiled to wasm).
import * as cache from "./cache.js";
import * as grant from "./grant.js";

const $ = (id) => document.getElementById(id);
const show = (...ids) => {
  for (const id of ["grant", "ready", "progress", "failed"]) $(id).hidden = !ids.includes(id);
  $("gate").hidden = false;
};
const say = (text) => ($("status").textContent = text);
const mib = (n) => (n / 2 ** 20).toFixed(0) + " MB";

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
      if (await navigator.gpu.requestAdapter()) return { api: "webgpu" };
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
// has the details. Only the first is shown: what follows a panic is its echo.
let ended = false;
function fail(title, text, detail) {
  console.error(`dbota: ${title}. ${text}${detail ? "\n" + detail : ""}`);
  if (ended) return;
  ended = true;
  $("starting").hidden = true;
  $("failTitle").textContent = title;
  $("failText").textContent = text;
  $("failMore").hidden = !detail;
  $("failDetail").textContent = detail || "";
  show("failed");
}

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

async function startGame() {
  $("gate").hidden = true;
  $("starting").hidden = false;
  const slow = setTimeout(() => {
    $("starting").textContent = "Still starting... If nothing changes, the browser's console (F12) says why.";
  }, 60000);
  const gpu = await graphics();
  if (!gpu.api) {
    clearTimeout(slow);
    return noGraphics(gpu.tried.join("; "));
  }
  drawing = gpu.api;
  console.info("dbota: drawing with " + gpu.api);
  try {
    const { default: init } = await import(gpu.api === "webgpu" ? "./dbota.js" : "./dbota_webgl2.js");
    await init();
  } catch (e) {
    // Bevy leaves its event loop by throwing; anything else is a real failure.
    if (!String(e).includes("control flow")) {
      clearTimeout(slow);
      if (e instanceof WebAssembly.RuntimeError) return stopped(e);
      fail("The game could not start", "Reloading the page may help; the browser's console has the details.", String((e && e.stack) || e));
    }
  }
}

function runWorker(op, files, modified, onMessage) {
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
    worker.postMessage({ op, files: [...files], modified });
  });
}

const modifiedOf = (files) => files.get("pak01_dir.vpk")?.lastModified ?? 0;

async function importFrom(files) {
  if (!files.has("pak01_dir.vpk")) {
    say("That folder has no pak01_dir.vpk. Pick the dota 2 beta folder (game/dota inside it holds the packages).");
    return show("grant");
  }
  navigator.storage.persist?.().catch(() => {});
  say("");
  show("progress");
  const result = await runWorker("import", files, modifiedOf(files), (m) => {
    if (m.type !== "progress") return;
    $("bar").value = m.total ? m.done / m.total : 0;
    $("stage").textContent = `Making your private copy: ${m.done} of ${m.total}`;
    $("detail").textContent = `${m.item} (${mib(m.bytes)} written)`;
  });
  if (result.type === "error") {
    say("The import failed: " + result.message);
    return show("grant");
  }
  if (result.stopped) {
    say("Import cancelled.");
    return show("grant");
  }
  const list = $("failures");
  list.hidden = !result.failures.length;
  list.replaceChildren(...result.failures.slice(0, 30).map((f) => Object.assign(document.createElement("li"), { textContent: f })));
  await offerReady(
    `Imported ${result.files} files. ${result.failures.length ? result.failures.length + " could not be converted and use stand-ins." : ""}`
  );
}

// The cache is there: publish it to the client, then ask or start.
async function offerReady(text) {
  const state = await cache.readState();
  if (!state) return show("grant");
  await cache.publish(state);
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

// A remembered folder the browser still lets us read: is the copy as new as the install?
async function checkFresh() {
  const handle = await grant.remembered().catch(() => null);
  if (!handle) return;
  const files = await grant.filesOf(handle).catch(() => new Map());
  if (!files.has("pak01_dir.vpk")) return;
  const result = await runWorker("check", files, modifiedOf(files));
  if (result.type === "checked" && !result.fresh) {
    $("update").hidden = false;
    $("update").onclick = () => importFrom(files);
    say("Dota 2 has changed since the copy was made.");
  }
}

async function main() {
  $("standins").onclick = startGame;
  $("play").onclick = startGame;
  $("reload").onclick = () => location.reload();
  $("otherApi").onclick = () => {
    const url = new URL(location.href);
    url.searchParams.set("gpu", "webgl2");
    location.assign(url);
  };
  $("pick").onclick = pick;
  $("again").onclick = pick;
  $("remove").onclick = async () => {
    await cache.remove();
    await grant.forget().catch(() => {});
    delete globalThis.dbota_cache;
    say("The imported copy is deleted.");
    show("grant");
  };
  if (query.get("assets") === "none") return startGame();
  if (query.get("cache") === "http") {
    globalThis.dbota_cache = { mode: "http" };
    return startGame();
  }
  if (!(await cache.supported())) {
    say("This browser has no private storage for assets: playing with stand-ins is possible.");
    return show("grant");
  }
  const state = await cache.readState().catch(() => null);
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
