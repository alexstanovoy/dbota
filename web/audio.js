// The game's sound in a browser. The client's audio stream makes its AudioContext when the client
// starts, and a browser may leave a context made without a user gesture suspended (Safari does so
// even when a gesture came a moment before); nothing in the client would ever resume it, and the
// game would play in silence. This keeps every context the page makes and resumes the suspended
// ones on the player's first click, key or touch.
const contexts = [];
for (const name of ["AudioContext", "webkitAudioContext"]) {
  const Base = globalThis[name];
  if (!Base) continue;
  globalThis[name] = class extends Base {
    constructor(...args) {
      super(...args);
      contexts.push(this);
    }
  };
}

function resume() {
  for (const context of contexts) {
    if (context.state === "suspended" || context.state === "interrupted") {
      context.resume().catch((e) => console.warn("dbota: the sound could not be resumed:", e));
    }
  }
}
for (const event of ["pointerdown", "keydown", "touchend"]) {
  addEventListener(event, resume, { capture: true, passive: true });
}

// For a player who hears nothing: type `dbota_audio_state()` in the console.
globalThis.dbota_audio_state = () => (contexts.length ? contexts.map((c) => c.state).join(",") : "no audio context");
