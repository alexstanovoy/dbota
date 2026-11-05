# dbota in a browser

The page asks for the player's own Dota 2 folder, builds a private copy of the assets inside the browser, then starts
the client (compiled to wasm, drawn through WebGPU or WebGL2). **Valve's files never leave the player's machine**: the
folder is read inside the tab with ranged reads, the copy lives in the browser's Origin Private File System, the page
has no code that sends it anywhere, and its Content-Security-Policy allows connections only to its own origin and the
game servers. Without a copy the page offers only the Dota 2 folder or a prepared cache, and the client doesn't start.

## Parts

| File | What it does |
| --- | --- |
| `index.html`, `page.css` | the gate (folder question, progress, ready) over the client's canvas; the CSP |
| `boot.js` | a classic script: "Loading" at once, and the one failure screen for a browser too old, a part that won't load or a start that stalls; every later wait has a limit in `main.js` and ends on a screen naming the stage |
| `main.js` | the flow: stored copy, folder grant, import progress, update check, delete, start the client that suits the browser |
| `grant.js` | the prepared-cache picker and the folder grant (`showDirectoryPicker`, remembered in IndexedDB; else `<input webkitdirectory>`); keeps only the `*.vpk` files it needs, whatever folder level was picked |
| `import_worker.js` | a Web Worker running `dbota_import` (wasm), writing with synchronous OPFS handles; also checks and copies a prepared cache |
| `cache.js` | the OPFS cache: layout, reads, index and stamp, published to the client as `globalThis.dbota_cache` |
| `replays.js` | the player's replays in OPFS (else IndexedDB), published as `globalThis.dbota_replays` |
| `report.js` | `?report`: console lines to the page's server, for local debugging |
| `keys.js`, `pointer.js` | which keys are the game's; the pointer lock and the game's own cursor |
| `test/smoke.py` | the Playwright smoke tests (below) |
| `build.sh` | builds `web/out`: both clients, the importer, the page |

The Rust side: `crates/dbota_import` (`import.rs` the importer, `web.rs` the wasm module over picked `File`s,
`pack.rs` the prepared-cache checks) and in the client `src/webcache.rs`, `src/cachefs.rs` and `src/manifest.rs`.

## Graphics

Bevy fixes its render paths at compile time, so the page carries two clients, `--features webgpu` (`dbota.js`) and
`--features webgl2` (`dbota_webgl2.js`), and loads WebGPU where the browser offers an adapter, else WebGL2
(`?gpu=webgl2` forces it). A browser with neither gets a message saying how to turn one on. On WebGL2 the client does
with one shadow cascade, CPU batching and no bindless textures, and decodes block-compressed textures where the
browser lacks the extensions.

The canvas draws at the screen's own pixels. Chromium on Linux draws through Xwayland unless told otherwise, and on a
Wayland desktop with fractional scaling the compositor then softens the whole window: set
`chrome://flags/#ozone-platform-hint` to Wayland. A start that fails never leaves a still canvas: panics and refused
allocations reach the page, which says what happened and what to do.

## Stamps

The copy lives in `dbota/` in OPFS with `index.txt` (written last: without it the copy is unfinished) and `stamp.txt`:
the asset fingerprint of the build that made it, then the size and modification time of `pak01_dir.vpk`, which Dota's
updater rewrites with every content change. A copy another build made is never played: the page offers a re-import
or a prepared cache of this version. When the remembered folder is readable without a prompt, it is compared in the
background and "Update from the Dota 2 folder" appears if the install changed.

## Prepared caches

A friend without Dota 2 plays from a cache someone with the game made (`dbota export-cache cache.zip`, ~400 MB; hand
it only to people you may). The page takes it as a `.zip` or a folder. Everything in it is untrusted and checked in the
worker with the same code as `dbota import-cache` before private storage is touched: the stamp must match this build,
`index.txt` must list exactly the files present with their sizes, every path must be plain and within the cache's
folders, extensions and size limits, and a zip may have no encryption, links, zip64 or overlapping members, each
inflated to no more than its stated size and matching its CRC-32. A damaged file ends the copy and removes it.

## Build and run

```sh
rustup target add wasm32-unknown-unknown; cargo install wasm-bindgen-cli --version <Cargo.lock's>
DBOTA_WEB_REPLAY=$PWD/match.brp web/build.sh        # PROFILE=release for a quicker build
python3 -m http.server --bind 127.0.0.1 --directory web/out 8080   # the page alone, to look at it
```

For play, a matchmaking service serves the page on the origin of its control socket (radovid: `[web] static_dir`, read-
only files over its TLS), so a player opens one URL; games run over secure WebSockets of their own hosts. `build.sh`
copies `assets/` into the folder rather than linking it, since links out of the folder aren't followed. A page served
elsewhere falls back to its host on port 4460. The page plays the replay built into it (`DBOTA_WEB_REPLAY`, part of the
program's memory, so keep it short; `DBOTA_WEB_ARGS` stand in for a command line). `?cache=http` plays an asset cache
served under `/cache` beside the page. `OUT=dir` builds elsewhere; `APIS=webgpu` builds one client only.

## Tests

`web/test/smoke.py` drives headless Chromium and Firefox with Playwright (`pip install --target temp/pwlib playwright &&
PYTHONPATH=temp/pwlib python3 -m playwright install chromium`); its docstring has the build commands of each check.

- Against a folder of hard links to the install's packages: the privacy text and only the two ways to a cache, folder
  picks at every level, the worker's import, the copy kept over a reload, the client reading it, no request beyond the
  page's origin, and deletion.
- `--prepared-page`: bad zips (another build, a path that climbs out, a wrong index) are refused with their reasons;
  the real zip and cache folder are copied, kept over a reload and played.
- `--replays-page`: a match through a real radovid that serves the page and keeps replays; the next visit fetches the
  replay, lists, plays, downloads and reopens it.
- `--replay-page`: a build with a packed replay plays it with chat and the CJK font fallback.

The merge gate runs them all in its `browser` step (`tools/browser_check.sh`).

## Manual checks (the native folder dialog is out of automation's reach)

1. Serve `web/out` and open it in Chrome: the privacy text, "Choose the Dota 2 folder", the prepared caches, and no way
   to play without assets.
2. Pick `dota 2 beta`, then `game` and `dota` (all accepted), then a wrong folder (a message, no import). "Cancel"
   returns to the question and leaves no copy.
3. Reload: "Play" at once; devtools shows the copy in OPFS and no request beyond the page's own files.
4. After a Dota update, "Update from the Dota 2 folder" appears; "Delete the imported copy" empties the storage.
5. Firefox and Safari use the fallback: the browser says it will "upload" the folder, which only lists it; watch
   Network to see that nothing leaves.
