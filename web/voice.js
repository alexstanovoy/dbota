// The page's side of the hero's voice: the client asks for a hero's pack
// (`globalThis.dbota_voice.request`), a worker reads it (voice_worker.js), and what it hands
// back waits here until the client takes it (`take`, src/webvoice.rs). One pack at a time: a
// newer request ends the older. A worker that has not answered after LIMIT_MS is ended and its
// pack reported as not read.
const LIMIT_MS = 120_000;

let current = null;
const answered = new Map();

function end() {
  if (!current) return;
  clearTimeout(current.timer);
  current.worker.terminate();
  current = null;
}

function request(hero, path, url) {
  end();
  answered.clear();
  const mode = globalThis.dbota_cache ? globalThis.dbota_cache.mode : null;
  const worker = new Worker(new URL("./voice_worker.js", import.meta.url), { type: "module" });
  const answer = (data) => {
    if (!current || current.worker !== worker) return;
    end();
    answered.set(hero, data);
  };
  const timer = setTimeout(() => answer({ hero, error: `the voice worker gave no answer in ${LIMIT_MS / 1000} s` }), LIMIT_MS);
  current = { worker, timer };
  worker.onmessage = ({ data }) => answer(data);
  worker.onerror = (e) => answer({ hero, error: e.message || "the voice worker failed" });
  worker.postMessage({ hero, path, url, mode });
}

function take(hero) {
  const data = answered.get(hero);
  if (!data) return null;
  answered.delete(hero);
  return data;
}

globalThis.dbota_voice = { request, take };
