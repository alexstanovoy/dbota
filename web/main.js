// The loader: what has to happen before the game (dbota.js, the client compiled to wasm) can
// start, and what is left to say once it has stopped. Everything a player meets in between, the
// first run included (what dbota is, the import of their Dota 2 files, sign-in), is the client's.
// What stays here, and why it cannot be the client's:
// - which features the browser lacks (the client cannot even start without them), said on the
//   page's own screen with what to do instead;
// - which client to load: Bevy fixes its graphics API when it is compiled, so there is one client
//   for WebGPU and one for WebGL 2, and the choice is made before either is downloaded;
// - the download, with its progress, its limits and a busy server's wait (busy.js);
// - the end of a client that stopped (a panic, no more memory): it can no longer draw, the page
//   says what happened.
import "./busy.js";
import * as replays from "./replays.js";
import { NATIVE_CLIENT } from "./native.js";
import "./keys.js";
import "./voice.js";
import * as pointer from "./pointer.js";

const $ = (id) => document.getElementById(id);
const busy = globalThis.dbota_busy;
// A busy server (busy.js): said where the page shows what it is doing now.
const sayBusy = (wait) => {
  for (const id of ["booting", "starting"]) if (!$(id).hidden) return ($(id).textContent = busy.line(wait));
};
// The longest a busy server may make a stage wait, on top of the stage's own limit.
const BUSY_MS = busy.MAX_S * 1000;
const mib = (n) => (n / 2 ** 20).toFixed(0) + " MB";

// Every wait on the browser has a limit, so the page never waits for ever: a stage that does not
// answer in `ms` ends as an error that names it, and the caller shows what the player can do.
const STALL_MS = 30000;
function within(stage, promise, ms) {
  let timer;
  const late = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${stage} did not answer in ${ms / 1000} seconds`)), ms);
  });
  return Promise.race([promise, late]).finally(() => clearTimeout(timer));
}

// ?gpu=webgl2 takes the WebGL2 client even where WebGPU works; ?cache=http plays on a copy of the
// assets served beside the page (a developer's own, see README.md), which the client reads itself.
const query = new URLSearchParams(location.search);
const served = query.get("cache") === "http";

// The native client's downloads, once a release names where they are (web/native.js).
function showNative() {
  if (!NATIVE_CLIENT) return;
  for (const link of $("native").querySelectorAll("a[data-os]")) link.href = NATIVE_CLIENT[link.dataset.os];
  $("native").hidden = false;
}

// The end of a run that went wrong: the page says what happened and what to do, the console has
// the details. Only the first is shown: what follows a panic is its echo (boot.js).
const fail = (title, text, detail) => {
  globalThis.dbota_boot.fail(title, text, detail);
  showNative();
};

// What the browser lacks of what the client needs before it can start: a copy of the player's
// Dota 2 files lives in the page's private storage (the Origin Private File System), is made from
// a folder the player picks, by a worker. A served copy needs none of that.
function lacking() {
  if (served) return [];
  const lacks = [];
  if (!(navigator.storage && navigator.storage.getDirectory)) {
    lacks.push("Private storage for its copy of your Dota 2 files (the Origin Private File System).");
  }
  if (typeof showDirectoryPicker !== "function" && !("webkitdirectory" in HTMLInputElement.prototype)) {
    lacks.push("A way to choose a folder of your computer (a directory picker or a folder upload).");
  }
  if (typeof Worker !== "function") lacks.push("Web workers, which make the copy while the game keeps drawing.");
  return lacks;
}

function unsupported(lacks) {
  console.warn("dbota: this browser lacks " + lacks.join(" "));
  $("lacks").replaceChildren(...lacks.map((text) => Object.assign(document.createElement("li"), { textContent: text })));
  $("unsupported").hidden = false;
  $("gate").hidden = false;
  showNative();
}

// Which client the browser can draw with: WebGPU where the browser gives an adapter, else WebGL2.
// `tried` says why neither, for the message.
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
  failed(title, text, detail) {
    fail(title, text || "Reloading the page usually gets you back; the details below say why it stopped.", detail);
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
        "a page's WebAssembly memory ends at 4 GB, often sooner). Close other tabs and reload.",
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
function downloaded(url, onBytes, onBusy) {
  const abort = new AbortController();
  let seen = Date.now();
  let stalled = false;
  const watch = setInterval(() => {
    if (Date.now() - seen > STALL_MS) {
      stalled = true;
      abort.abort();
    }
  }, 1000);
  const waited = (wait) => {
    // The wait the server asked for is no stall.
    seen = Date.now() + wait * 1000;
    onBusy(wait);
  };
  return busy
    .fetch(url, { signal: abort.signal }, waited)
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
}

const sayStarting = (text) => ($("starting").textContent = text);

async function start() {
  globalThis.dbota_boot.started();
  $("booting").hidden = true;
  const lacks = lacking();
  const gpu = await graphics();
  if (!gpu.api) lacks.push("Graphics the page may use: WebGPU or WebGL 2. " + NO_GPU);
  if (lacks.length) return unsupported(lacks);
  $("starting").hidden = false;
  sayStarting("Starting the game...");
  const slow = setTimeout(() => {
    sayStarting("Still starting... If nothing changes, the browser's console (F12) says why.");
  }, 60000);
  // The client keeps the player's replays in this browser (replays.js); a storage that is
  // missing or does not answer only means it keeps none.
  await within("The replay storage", replays.publish(), 20000).catch((e) => console.warn("dbota: no replay storage:", e));
  drawing = gpu.api;
  console.info("dbota: drawing with " + gpu.api);
  const wasm = gpu.api === "webgpu" ? "dbota_bg.wasm" : "dbota_webgl2_bg.wasm";
  try {
    const script = gpu.api === "webgpu" ? "./dbota.js" : "./dbota_webgl2.js";
    const { default: init } = await within("The game's script", busy.import(script, sayBusy), STALL_MS * 2 + BUSY_MS);
    const module_or_path = downloaded(
      "./" + wasm,
      (got, total) => sayStarting(`Downloading the game: ${mib(got)}${total ? " of " + mib(total) : ""}`),
      sayBusy
    );
    await init({ module_or_path });
    sayStarting("Starting the game...");
  } catch (e) {
    // winit may leave its event loop by throwing: no failure.
    if (String(e).includes("control flow")) return;
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

$("reload").onclick = () => location.reload();
$("otherApi").onclick = () => {
  const url = new URL(location.href);
  url.searchParams.set("gpu", "webgl2");
  location.assign(url);
};
start().catch((e) => fail("The page could not start the game", "Reload the page; the details below say why.", String(e)));
