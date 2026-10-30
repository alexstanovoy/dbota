// The pointer while the game keeps the cursor (src/web_cursor.rs: where the desktop confines it to
// the window, the page locks the pointer). A browser locks only inside a gesture, so the page asks:
// when the client says it keeps the cursor (the frame after the click or key that got it there), on
// a press on the canvas, and on F11 before the fullscreen uses the gesture up (keys.js).
//
// A locked pointer reports motion, not a place: the page moves the game's cursor by that motion,
// held inside the canvas, and hands the client each move and press at that place as the pointer's
// own, so the client reads one cursor however the pointer is held.
const canvas = document.getElementById("dbota");

// Chromium reports a locked pointer's motion in device pixels, other engines in CSS pixels
// (w3c/pointerlock#42); the client's windowing reads it the same way.
const deviceMotion = !!navigator.userAgentData?.brands?.some((b) => b.brand === "Chromium");

const isLocked = () => document.pointerLockElement === canvas;
// The cursor in CSS pixels of the canvas while the pointer is locked; where the pointer was last
// seen free, where the lock starts (a locked pointer's own place says nothing).
let at = null;
let free = null;
// The moves and presses this page hands on, which it must not steer again.
const handed = new WeakSet();

let wanted = false;

// Why a lock was refused, for the console only (a missing gesture, the browser's pause after an
// Esc, the document not focused).
const refused = (e) => console.debug(`pointer lock refused: ${e?.name ?? "error"}`);

// Locks the pointer if the game keeps the cursor. A refusal (no gesture, the browser's pause after
// an Esc) is the browser's answer: the next press asks again.
export function take() {
  if (wanted && !isLocked()) canvas.requestPointerLock()?.catch?.(refused);
}

// Whether the game keeps the cursor, from the client.
export function keep(want) {
  wanted = want;
  if (want) take();
  else if (isLocked()) document.exitPointerLock();
}

canvas.addEventListener("pointerdown", (e) => {
  if (!handed.has(e)) take();
});
document.addEventListener("pointerlockchange", () => {
  if (!isLocked()) at = null;
});
canvas.addEventListener("pointermove", (e) => {
  if (!handed.has(e) && !isLocked()) free = [e.offsetX, e.offsetY];
});

const clamp = (v, lo, hi) => Math.min(Math.max(v, lo), hi);

function steer(e) {
  if (handed.has(e) || !isLocked()) return;
  e.stopImmediatePropagation();
  const r = canvas.getBoundingClientRect();
  // The last device pixel inside the canvas.
  const right = Math.max(r.width - 1 / devicePixelRatio, 0);
  const bottom = Math.max(r.height - 1 / devicePixelRatio, 0);
  const k = deviceMotion ? devicePixelRatio : 1;
  const [x, y] = at ?? free ?? [r.width / 2, r.height / 2];
  // Only a move is motion. A press or a release happens where the cursor is, whatever movement the
  // browser attaches to it (Chromium reports the pointer's travel since its last event on them
  // too, and a lock that re-centres the pointer makes that travel large).
  const moved = e.type === "pointermove";
  const [dx, dy] = moved ? [e.movementX, e.movementY] : [0, 0];
  at = [clamp(x + dx / k, 0, right), clamp(y + dy / k, 0, bottom)];
  const init = {
    bubbles: true,
    cancelable: true,
    composed: true,
    clientX: r.left + at[0],
    clientY: r.top + at[1],
    movementX: dx,
    movementY: dy,
    button: e.button,
    buttons: e.buttons,
    pointerId: e.pointerId,
    pointerType: e.pointerType,
    isPrimary: e.isPrimary,
    pressure: e.pressure,
    ctrlKey: e.ctrlKey,
    shiftKey: e.shiftKey,
    altKey: e.altKey,
    metaKey: e.metaKey,
  };
  // The client reads a move's coalesced events, which a page's own event has only when given.
  if (e.type === "pointermove") init.coalescedEvents = [new PointerEvent(e.type, init)];
  const own = new PointerEvent(e.type, init);
  handed.add(own);
  canvas.dispatchEvent(own);
}

for (const type of ["pointermove", "pointerdown", "pointerup"]) {
  window.addEventListener(type, steer, true);
}
