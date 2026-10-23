# dbota in a browser

The page asks for the player's own Dota 2 folder, builds a private copy of the assets the client
uses (models, the map, textures, sounds, texts) inside the browser, then starts the client (the game compiled to
wasm, drawn through WebGPU or WebGL2). **Valve's files never leave the player's machine**: the
folder is read inside the tab with ranged reads, the converted copy lives in the browser's own
storage (the Origin Private File System), the page has no code that sends it anywhere, and its
Content-Security-Policy only allows connections to its own origin (game sockets are added when the page gets networking).
"Play with stand-ins" is always offered; the client then draws its built-in shapes.

## Parts

| File | What it does |
| --- | --- |
| `index.html`, `page.css` | the gate (folder question, progress, ready) over the client's canvas; the CSP |
| `main.js` | the flow: stored copy? folder grant, import progress, update check, delete, start the client |
| `grant.js` | the folder grant: `showDirectoryPicker` (Chromium, the handle is remembered in IndexedDB so the copy can be checked for staleness without asking again), else `<input webkitdirectory>`; only `*.vpk` of `game/dota` and `maps/` are kept, whatever folder level was picked |
| `import_worker.js` | a module Web Worker: runs `dbota_import` (wasm), writes each item's files with synchronous OPFS handles, stops on request |
| `cache.js` | the OPFS cache: layout, reads, index and stamp, what is published to the client (`globalThis.dbota_cache`) |
| `report.js` | `?report`: console lines to the page's server, for local debugging |
| `test/smoke.py` | the Playwright smoke test (below) |
| `build.sh` | builds `web/out` |

The Rust side: `crates/dbota_assets` (`import.rs`: the manifest's plan, the step-wise importer with
read/write budgets and checked output paths), `crates/dbota_import` (the wasm module: the picked
`File`s as a `Source` read with `File.slice` + `FileReaderSync`), and in the client `src/webcache.rs`
+ `src/cachefs.rs` (the OPFS reader: `CacheReader`'s browser backend; the few files the client reads
synchronously at start are loaded into memory by the page first) + `src/manifest.rs` (what is there
comes from the cache's index).

## Stamps

The cache lives in `dbota/` in OPFS next to `index.txt` (`file <size> <path> [small]` per file, written
last: a cache without it is unfinished) and `stamp.txt`: `v<manifest version>/rev<importer revision>/<size
of pak01_dir.vpk>/<its modification time>`. Dota's updater rewrites `pak01_dir.vpk` with every content
change. A page with a stored copy goes straight to "Play"; when the remembered folder is readable
without a prompt it is compared in the background and "Update from the Dota 2 folder" appears if the
stamp differs. A new importer revision or manifest version changes the stamp too.

## What is imported

Everything the manifest lists: the 54 models (static ones, wearables and the 25 animated ones with their
clips; glb with their images), the map's band around the mid lane (`map/mid/mid.gltf`, its geometry, its
~300 images, `trees.txt`, `buildings.txt`) with the terrain's layer textures, 120 textures (PNG), 98
sounds (WAV/MP3) and the tooltip texts (`text/tooltips.json`). A full import writes ~550 MB, 836 files.

## Build and run

```sh
rustup target add wasm32-unknown-unknown; cargo install wasm-bindgen-cli --version <Cargo.lock's>
DBOTA_WEB_REPLAY=$PWD/match.brp web/build.sh        # PROFILE=release for a quicker build
python3 -m http.server --bind 127.0.0.1 --directory web/out 8080
```
The page plays the replay built into it (keep it short: it is part of the program's memory;
`DBOTA_WEB_ARGS` are the command line a page lacks). `?cache=http` plays an asset cache served
under `/cache` next to the page (a developer's own export, e.g. `ln -s ~/.cache/dbota/v<N>
web/out/cache`) and `?assets=none` skips the question. With `?report`, the console goes to the
server's log as `/report?...` requests.

## Tests

`web/test/smoke.py` drives headless Chromium (Playwright for Python; `pip install --target temp/pwlib
playwright && PYTHONPATH=temp/pwlib python3 -m playwright install chromium`), against a folder made
of hard links to the install's packages: privacy text and stand-ins offered, folder picks, the
worker imports, the copy persists over a reload, the client starts and reads it, nothing but the page's
own origin is requested and nothing is sent, the copy is deleted. It also unit-tests the
folder logic (`webkitdirectory` lists, `showDirectoryPicker` folder levels) against a folder tree
made in OPFS. The native importer tests are in `crates/dbota_assets/tests/import.rs`.

Two more checks need no Dota install (the page runs on stand-ins, `?assets=none`); the docstring of
`smoke.py` has the build commands. `--replay-page` opens a build with a Zstandard-packed replay built in
(`DBOTA_WEB_REPLAY`, read through to `--seek` in `DBOTA_WEB_ARGS`) and expects the client's `match known` and
`chat: N lines heard` console lines, the CJK fallback registered and no read error. `--live-page --server`
opens a build without a replay, reaches the direct connection through the menu, joins a local `bota_server`
(built with the `websocket` feature) over `ws://`, and expects the server's frames to be compressed parts
(`ServerMsg::Compressed`) the client unpacks into a match, and a chat line typed in the page to come back
heard; the page's CSP gets that one socket for the run, as a page with networking would. Without an
install the import test is skipped with a message. `OUT=dir web/build.sh` builds somewhere else than `web/out`.

## Manual checks (the native directory dialog is out of automation's reach)

1. Serve `web/out`, open it in Chrome: the gate shows the privacy text, "Choose the Dota 2 folder" and
   "Play with stand-ins" (which works at once).
2. Choose the folder with the picker: `dota 2 beta`, then `game` and `dota` once each (all accepted), and a
   wrong folder (a message, no import). Progress counts up to ~645 steps; "Cancel" returns to the question
   and leaves no copy. The ready screen says "Imported 834 files" (~80 s).
3. Reload: "Play" is offered at once; devtools > Application > Storage shows about 550 MB in the origin-private
   file system; Network shows no request except the page's own files.
4. Chrome, later: the remembered folder is compared without a prompt while the permission lasts; after a
   Dota update "Update from the Dota 2 folder" appears. "Delete the imported copy" empties the storage.
5. Firefox and Safari use the fallback: the browser says it will "upload" the folder, which only lists it;
   nothing leaves the machine (watch Network).
