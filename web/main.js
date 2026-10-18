// The page: asks for the player's Dota 2 folder (or not), has the worker build the private
// asset cache from it, then starts the client (dbota.js, the game compiled to wasm).
import * as cache from "./cache.js";
import * as grant from "./grant.js";

const $ = (id) => document.getElementById(id);
const show = (...ids) => {
  for (const id of ["grant", "ready", "progress"]) $(id).hidden = !ids.includes(id);
  $("gate").hidden = false;
};
const say = (text) => ($("status").textContent = text);
const mib = (n) => (n / 2 ** 20).toFixed(0) + " MB";

// ?cache=http plays what a local server holds under /cache (a developer's own export); ?assets=none
// skips the question and plays with stand-ins.
const query = new URLSearchParams(location.search);

async function startGame() {
  $("gate").hidden = true;
  const { default: init } = await import("./dbota.js");
  try {
    await init();
  } catch (e) {
    // Bevy leaves its event loop by throwing; anything else is a real failure.
    if (!String(e).includes("control flow")) {
      $("gate").hidden = false;
      say("The game could not start: " + e);
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
    `Imported ${result.files} files. ${result.failures.length ? result.failures.length + " could not be converted and use stand-ins. " : ""}` +
      (result.deferred ? "Animated units and the map use stand-ins for now." : "")
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
