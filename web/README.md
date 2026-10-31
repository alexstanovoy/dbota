# dbota in a browser

The page asks for the player's own Dota 2 folder, builds a private copy of the assets the client
uses (models, the map, textures, sounds, texts) inside the browser, then starts the client (the game compiled to
wasm, drawn through WebGPU or WebGL2: see "Graphics" below). **Valve's files never leave the player's machine**: the
folder is read inside the tab with ranged reads, the converted copy lives in the browser's own
storage (the Origin Private File System), the page has no code that sends it anywhere, and its
Content-Security-Policy only allows connections to its own origin (game sockets are added when the page gets networking).
"Play with stand-ins" is always offered; the client then draws its built-in shapes. A player without Dota 2 can load a
prepared asset cache instead (see "Prepared caches").

## Parts

| File | What it does |
| --- | --- |
| `index.html`, `page.css` | the gate (folder question, progress, ready) over the client's canvas; the CSP |
| `boot.js` | a classic script that runs where the modules cannot: shows "Loading" at once and the one failure screen (`dbota_boot.fail`), for a browser too old, a part that does not load, code that cannot run, or a page that has not started in 30 s. Every later wait (storage, the graphics adapter, the download of the game, which shows its progress and ends when nothing arrives for 30 s) has a limit in `main.js` and ends on a screen that names the stage |
| `main.js` | the flow: stored copy? folder grant, import progress, update check, delete, start the client that suits the browser; a start that fails ends in a message saying what to do |
| `grant.js` | the prepared-cache picker (a `.zip` or a folder through `<input type=file>`), and the folder grant: `showDirectoryPicker` (Chromium, the handle is remembered in IndexedDB so the copy can be checked for staleness without asking again), else `<input webkitdirectory>`; only `*.vpk` of `game/dota` and `maps/` are kept, whatever folder level was picked |
| `import_worker.js` | a module Web Worker: runs `dbota_import` (wasm), writes each item's files with synchronous OPFS handles, stops on request; also checks and copies a prepared cache |
| `cache.js` | the OPFS cache: layout, reads, index and stamp, what is published to the client (`globalThis.dbota_cache`) |
| `replays.js` | the player's own matches' replays, kept by the client in OPFS (a folder `dbota-replays` of its own, apart from the asset copy), else IndexedDB; published as `globalThis.dbota_replays` (list/read/write/remove, the index, a download, a file picker); the client decides what is kept and for how long (`src/replays`) |
| `report.js` | `?report`: console lines to the page's server, for local debugging |
| `keys.js` | which keys are the game's and which the browser's; F11 is the canvas's fullscreen |
| `pointer.js` | the pointer while the game keeps the cursor: locked in a gesture, the game's cursor moved by its motion inside the canvas and handed to the client as the pointer's own |
| `test/smoke.py` | the Playwright smoke test (below) |
| `build.sh` | builds `web/out`: both clients, the importer, the page |

The Rust side: `crates/dbota_import` (`import.rs`: the manifest's plan, the step-wise importer with
read/write budgets and checked output paths, over heatel's readers; `web.rs`: the wasm module, the
picked `File`s as a `Source` read with `File.slice` + `FileReaderSync`), and in the client `src/webcache.rs`
+ `src/cachefs.rs` (the OPFS reader: `CacheReader`'s browser backend; the few files the client reads
synchronously at start are loaded into memory by the page first) + `src/manifest.rs` (what is there
comes from the cache's index).

## Graphics

Bevy fixes its render paths when it is compiled (storage buffers, compute, limits, shader
definitions), so one build cannot switch between WebGPU and WebGL2 at run time. The page carries two
clients, built with `--features webgpu` (`dbota.js`) and `--features webgl2` (`dbota_webgl2.js`), and
loads one: WebGPU where the browser gives an adapter, else WebGL2 (`?gpu=webgl2` asks for it). Firefox
on Linux has no WebGPU unless `dom.webgpu.enabled` is set, and some browsers (LibreWolf on some
machines) turn WebGL off as well; a browser with neither gets a message saying how to turn one on.

What WebGL2 lacks, the client does without: one shadow cascade, reaching as far as the two of WebGPU
(Bevy's limit there); GPU batching and clustering on the CPU (Bevy falls back by itself); the fog
material without bindless textures. Block-compressed textures are drawn as they are where the browser
has `WEBGL_compressed_texture_s3tc` (and `_srgb`), `EXT_texture_compression_rgtc` and `_bptc`, else decoded as they
load, as on a WebGPU without `texture-compression-bc`. Offscreen targets (minimap, portraits, render
scale) are RGBA, the one 8-bit order WebGL2 uploads.

The canvas is drawn at the screen's own pixels: its drawing buffer and the client's surface are
the canvas's device-pixel size (`devicePixelContentBoxSize`, fractional scales too), so the picture
is as sharp as the desktop client's wherever the browser shows its window 1:1. Chromium on Linux
draws through X11 (Xwayland) unless told otherwise, and on a Wayland desktop scaled by anything but
1 (a fractional scale above all) the compositor then resamples the whole browser window, the game
with it: the picture is softer than the desktop client's, which draws at the screen's pixels. On
Wayland Chromium draws 1:1: set `chrome://flags/#ozone-platform-hint` to Wayland (or start it with
`--ozone-platform=wayland`).

A start that fails never leaves a still canvas: the client hands its panics (src/webpage.rs) and the
allocations the browser refused to the page, which says what happened and what to do (no graphics: how
to turn it on; a renderer that cannot be made with WebGPU: the WebGL2 client instead; out of memory;
a bug), with the details in the page and in the console. "Starting the game..." stays until the
first frame.

## Stamps

The cache lives in `dbota/` in OPFS next to `index.txt` (`file <size> <path> [small]` per file, written
last: a cache without it is unfinished) and `stamp.txt`: the asset fingerprint of the
build that made it (`v<manifest version>/rev<importer revision>/<manifest hash>`, as on the desktop),
then `/<size of pak01_dir.vpk>/<its modification time>`. Dota's updater rewrites `pak01_dir.vpk` with
every content change. At every start the page compares the copy's fingerprint with its own (the
importer module's `cache_version()`): a copy another dbota made is never played. The page holds at
"The assets are out of date for this version" with the re-import right there (one click where the
browser still holds the folder, else from the folder picked again) or stand-ins; the client, given a
copy of another fingerprint anyway, draws stand-ins. A copy of this fingerprint goes straight to
"Play"; when the remembered folder is readable without a prompt it is compared in the background and
"Update from the Dota 2 folder" appears if the install changed. The gate shows the build's asset
fingerprint at its foot, the settings the client's commit too.

## Prepared caches

A friend without Dota 2 plays with Dota's look from a cache that someone with the game made: `dbota import` on
the desktop, then `dbota export-cache cache.zip` (the converted assets, 400 MB zipped; hand it only to people you
may). The page offers "Use a prepared cache (.zip)" and "Use a prepared cache (folder)" (the cache folder as
`dbota import` wrote it, `stamp.txt` and `index.txt` at its top) next to "Use the Dota 2 folder" (the folder through `showDirectoryPicker` where the browser has it, else `<input webkitdirectory>`), and at the question for
a re-import. A zip packed with the cache's folder around it (what a file manager makes) is read too.

Everything in it is untrusted, and all of it is checked in the worker (`crates/dbota_import/src/pack.rs`, the same
code as `dbota import-cache`) before the copy in private storage is touched: the stamp must start with this
build's asset fingerprint, else the page says which assets the cache is for and which this dbota needs; `index.txt`
must list exactly the files present with their sizes; every path must be plain (no `..`, no absolute path, no
backslash), in one of the cache's folders with an extension the importer makes there, within its size limit; a zip
has no encryption, links, zip64 or overlapping members, and each member is inflated into no more than its stated
size and must match its CRC-32. Then the files are copied with progress and the index and stamp last; a file that
turns out damaged ends the copy and removes the half-made one. The copy is published to the client as an import
is, and nothing is requested from anywhere.

## What is imported

Everything the manifest lists: the models (static ones, wearables and the animated ones with their
clips; glb with their textures), the map's band around the mid lane (`map/mid/mid.gltf`, its geometry, its
textures, `trees.txt`, `buildings.txt`) with the terrain's layer textures, the interface textures
(PNG), the sounds (WAV/MP3) and the tooltip texts (`text/tooltips.json`). The models' and the map's textures
are KTX2 files block-compressed with their mip levels (see `tools/README.md`); a WebGPU without
`texture-compression-bc` gets them decoded as they load. The import's summary line gives the size and the file count of a full import.

## Build and run

```sh
rustup target add wasm32-unknown-unknown; cargo install wasm-bindgen-cli --version <Cargo.lock's>
DBOTA_WEB_REPLAY=$PWD/match.brp web/build.sh        # PROFILE=release for a quicker build
python3 -m http.server --bind 127.0.0.1 --directory web/out 8080   # the page alone, to look at it
```
For play, a matchmaking service serves the page itself, on the origin of its control socket, so the page
needs the CSP's `'self'` for it (and `wss:`, for the game servers it names: a game is played over a secure WebSocket of its own host) and a player opens one URL: give the service the built folder
as its static directory (radovid: `[web] static_dir`, read-only files over its TLS; copy `assets/` into
the folder rather than linking it, a link out of the folder is not followed). A page served over HTTPS
takes its own origin for the service; one served by another server falls back to its host on port 4460.
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
made in OPFS. The native importer tests are in `crates/dbota_import/tests/import.rs`.

`--replays-page DIR --server BOTA_SERVER --radovid RADOVID` (a build without a replay, any assets: `?assets=none`) plays a match through a real radovid that serves the page itself and keeps replays (`[artifacts] save_replays = true`): a practice lobby is entered alone and the page closed; the game server ends, and the next visit signs in with the kept session, fetches the replay it was away for and stores it in OPFS; the REPLAYS tab lists it and plays it, DOWNLOAD .BRP hands over the same bytes and OPEN A .BRP FILE plays the file again. `--shots` saves a screenshot of each step to `temp/smoke-replays` (the click positions in the script are for a 1280x720 page).
`--prepared-page DIR --cache-zip cache.zip --cache-folder <cache folder>` (a build with a replay built in, so the client draws the map) needs no install, only a cache
`dbota export-cache` made with this build (the `smoke.py` docstring has the commands): zips that must be refused
(another build, a path that climbs out, an index that does not match) are refused with their reasons and copy nothing;
the directory picker (stubbed with a folder in private storage) is walked and its folder refused with its reason; the real zip is copied into private storage, kept over a reload and played until the scene is drawn; the cache
folder is then loaded the same way.

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
   wrong folder (a message, no import). Progress counts up to its last step; "Cancel" returns to the question
   and leaves no copy. The ready screen says how many files were imported (about a minute and a half).
3. Reload: "Play" is offered at once; devtools > Application > Storage shows the copy (hundreds of MB) in the origin-private
   file system; Network shows no request except the page's own files.
4. Chrome, later: the remembered folder is compared without a prompt while the permission lasts; after a
   Dota update "Update from the Dota 2 folder" appears. "Delete the imported copy" empties the storage.
5. Firefox and Safari use the fallback: the browser says it will "upload" the folder, which only lists it;
   nothing leaves the machine (watch Network).
