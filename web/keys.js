// The keys of the page and the keys of the game. The client lets the page's own events through
// (it does not cancel them, see `prevent_default_event_handling`), so this decides, for the
// canvas only: the game keeps its keys (F5 is Dota's quick buy, Tab and Space are bound, a
// right click moves a unit, the wheel zooms), and the browser keeps every Ctrl or Cmd chord
// (reload, close tab, switch tab, find) and F12.
//
// F11 is the game's fullscreen: the canvas goes fullscreen (the Fullscreen API, called inside the
// key's gesture), the way the display setting does it, so the setting and the key agree and the
// canvas, not a page-sized window, fills the screen.
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
    if (document.fullscreenElement) document.exitFullscreen();
    else canvas.requestFullscreen().catch(() => {});
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
