// The keys of the page and the keys of the game. The client lets the page's own events through
// (it does not cancel them, see `prevent_default_event_handling`), so this decides, for the
// canvas only: the game keeps its keys (F5 is Dota's quick buy, Tab and Space are bound, a
// right click moves a unit, the wheel zooms), and the browser keeps every Ctrl or Cmd chord
// (reload, close tab, switch tab, find) and F12.
//
// F11 is the game's fullscreen: the canvas goes fullscreen (the Fullscreen API, called inside the
// key's gesture), the way the display setting does it, so the setting and the key agree and the
// canvas, not a page-sized window, fills the screen. The pointer is locked first where the game
// keeps the cursor (pointer.js): the fullscreen uses the gesture up.
//
// Esc is a game key (cancel, close the menus), but a browser leaves fullscreen on it. Where the
// Keyboard Lock API exists (Chromium) the canvas locks Esc while it is fullscreen: a short press
// reaches the game and the browser asks the player to hold Esc to leave (F11 leaves too). Firefox
// and Safari have no such lock, and there Esc still leaves.
import { take } from "./pointer.js";

const canvas = document.getElementById("dbota");

export function ownedByBrowser(e) {
  return e.ctrlKey || e.metaKey || e.key === "F12";
}

window.addEventListener(
  "keydown",
  (e) => {
    if (e.key !== "F11" || e.ctrlKey || e.metaKey || e.altKey) return;
    e.preventDefault();
    if (e.repeat) return;
    if (document.fullscreenElement) {
      document.exitFullscreen();
      return;
    }
    take();
    canvas.requestFullscreen().catch(() => {});
  },
  true
);

for (const type of ["keydown", "keyup"]) {
  canvas.addEventListener(type, (e) => {
    if (!ownedByBrowser(e) && e.key !== "F11") e.preventDefault();
  });
}
canvas.addEventListener("contextmenu", (e) => e.preventDefault());
canvas.addEventListener("wheel", (e) => e.preventDefault(), { passive: false });

// The lock works only while the document is fullscreen; the browser lets it go on leaving, which is
// asked for as well so that the page never relies on it. Nothing is done on blur or a hidden tab:
// the browser applies the lock only while the tab is focused and shown, so Alt-Tab and tab switches
// take Esc back by themselves. On the menus, with no match, a short Esc then does nothing (the game
// has no use for it there) and only a held Esc or F11 leaves fullscreen.
let unsupported = false;
document.addEventListener("fullscreenchange", () => {
  const keyboard = navigator.keyboard;
  if (document.fullscreenElement !== canvas) {
    keyboard?.unlock?.();
    return;
  }
  if (typeof keyboard?.lock !== "function") {
    if (!unsupported) console.info("dbota: this browser cannot keep Esc in the game; Esc leaves fullscreen");
    unsupported = true;
    return;
  }
  keyboard.lock(["Escape"]).then(
    () => console.debug("dbota: Esc is locked to the game while fullscreen"),
    (e) => console.debug(`dbota: Esc lock refused: ${e?.name ?? "error"}`)
  );
});
