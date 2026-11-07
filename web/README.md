# dbota in a browser

The page asks for the player's own Dota 2 folder, builds a private copy of the assets inside the browser, then starts
the client (compiled to wasm, drawn through WebGPU or WebGL2). **Valve's files never leave the player's machine**: the
folder is read inside the tab with ranged reads, the copy lives in the browser's Origin Private File System, the page
has no code that sends it anywhere, and its Content-Security-Policy allows connections only to its own origin and the
game servers. Without a copy the page offers two buttons, "Use Dota 2 folder" and "Use cache (.zip / folder)", and the client doesn't start; with one, "Play" and "Delete and import again". Below both, the native client's download links.

## Parts

| File | What it does |
| --- | --- |
| `index.html`, `page.css` | the gate (folder question, progress, ready) over the client's canvas; the CSP |
| `native.js` | where the native client's download links point (one constant per system, placeholders for now) |
| `boot.js` | a classic script: "Loading" at once, and the one failure screen for a browser too old, a part that won't load or a start that stalls; every later wait has a limit in `main.js` and ends on a screen naming the stage |
| `busy.js` | a server busy sending pages answers 503 with Retry-After: every fetch, module and worker of the page's own origin goes through here, says so on the page, waits as asked (1 to 60 s) and tries once more |
| `main.js` | the flow: stored copy, folder grant, import progress, update check, delete, start the client that suits the browser |
| `grant.js` | the prepared-cache picker and the folder grant (`showDirectoryPicker`, remembered in IndexedDB; else `<input webkitdirectory>`); keeps only the `*.vpk` files and the interface's typefaces (`panorama/fonts/*.otf`) it needs, whatever folder level was picked |
| `import_worker.js` | a Web Worker running `dbota_import` (wasm), writing with synchronous OPFS handles; also checks and copies a prepared cache |
| `cache.js` | the OPFS cache: layout, reads, index and stamp, published to the client as `globalThis.dbota_cache` |
| `replays.js` | the player's replays in OPFS (else IndexedDB), published as `globalThis.dbota_replays` |
| `report.js` | `?report`: console lines to the page's server, for local debugging |
| `keys.js`, `pointer.js` | which keys are the game's; the pointer lock and the game's own cursor |
| `test/smoke.py` | the Playwright smoke tests (below) |
| `build.sh` | builds `web/out`: both clients, the importer, the page |

The Rust side: `crates/dbota_import` (`import.rs` the importer, `pack.rs` the prepared-cache checks),
`crates/dbota_import_web` (the wasm module over picked `File`s) and in the client `src/webcache.rs`, `src/cachefs.rs` and `src/manifest.rs`.

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
WASM_OPT=0 DBOTA_WEB_REPLAY=$PWD/match.brp web/build.sh        # PROFILE=release for a quicker build
python3 -m http.server --bind 127.0.0.1 --directory web/out 8080   # the page alone, to look at it
```

For play, a matchmaking service serves the page on the origin of its control socket (radovid: `[web] static_dir`, read-
only files over its TLS), so a player opens one URL; games run over secure WebSockets of their own hosts. `build.sh`
copies `assets/` into the folder rather than linking it, since links out of the folder aren't followed. A page served
elsewhere falls back to its host on port 4460. The page plays the replay built into it (`DBOTA_WEB_REPLAY`, part of the
program's memory, so keep it short; `DBOTA_WEB_ARGS` stand in for a command line). `?cache=http` plays an asset cache
served under `/cache` beside the page. `OUT=dir` builds elsewhere; `APIS=webgpu` builds one client only.

## Tests

Chromium checks use software Vulkan (SwiftShader) by default. Set `DBOTA_BROWSER_GPU=hardware`
to request the local GPU; both modes run the same checks.

`web/test/smoke.py` drives headless Chromium and Firefox with Playwright (`pip install --target temp/pwlib playwright &&
PYTHONPATH=temp/pwlib python3 -m playwright install chromium`); its docstring has the build commands of each check.

- Against a folder of hard links to the install's packages: the privacy text and only the two ways to a cache, folder
  picks at every level, the worker's import, the copy kept over a reload, the client reading it, no request beyond the
  page's origin, and deletion.
- `--start-page`: the start window in both states (two buttons without a copy; "Play" and "Delete and import again" with
  one), the delete returning to the first without touching the player's replays, and the native client's links.
- `--prepared-page`: bad zips (another build, a path that climbs out, a wrong index) are refused with their reasons;
  the real zip and cache folder are copied, kept over a reload and played.
- `--replays-page`: a match through a real radovid that serves the page and keeps replays; the next visit fetches the
  replay, lists, plays, downloads and reopens it.
- `--busy-page`: a server that answers 503 with Retry-After once gets the page's own script, the served cache's index and
  the game's download on the second try, with the wait said on the page; a download refused twice is a failure.
- `--replay-page`: a build with a packed replay plays it with chat and the CJK font fallback.

The merge gate runs them all in its `browser` step (`tools/browser_check.sh`).

## Manual checks (the native folder dialog is out of automation's reach)

1. Serve `web/out` and open it in Chrome: the privacy text, "Use Dota 2 folder" and "Use cache (.zip / folder)" (the latter offers a
   .zip or a folder), and no way to play without assets.
2. Pick `dota 2 beta`, then `game` and `dota` (all accepted), then a wrong folder (a message, no import). "Cancel"
   returns to the question and leaves no copy.
3. Reload: "Play" at once; devtools shows the copy in OPFS and no request beyond the page's own files.
4. After a Dota update, the page says so; "Delete and import again" empties the storage.
5. Firefox and Safari use the fallback: the browser says it will "upload" the folder, which only lists it; watch
   Network to see that nothing leaves.

The build requires Python 3. Its output directory must be empty or carry the
`.dbota-web-output` marker created by `web/build.sh`. An older unmarked output is refused;
choose a new empty directory. Assets in an owned output carry `.dbota-web-assets` and only
those owned assets may be replaced. Source directories and symbolic links in an output are
refused before the build starts.

The `web` profile uses `opt-level = "s"` and thin LTO in one codegen unit to balance
transfer size and frame time. A warm-dependency build of both clients and the importer
took 365 s with thin LTO versus 856 s with fat LTO under shared load.
In the shared-load measurements, versus `z`/thin, the default used 8.5% more Brotli
bytes on WebGPU while first-frame time (client download excluded) fell from 1.31 to
0.90 s on WebGPU and from 1.37 to 0.66 s on WebGL2.
For the speed-first alternative, run
`CARGO_PROFILE_WEB_OPT_LEVEL=3 WASM_OPT=0 web/build.sh`; its larger modules cost more
transfer bandwidth. Choose the
postprocessing policy explicitly: `WASM_OPT=0` keeps compiler output, while
`WASM_OPT=/path/to/wasm-opt` runs **Binaryen 125** (`-Oz`) on all three modules.
`web/build.sh` now requires `WASM_OPT` for its default `web` profile; deployment
builds must pass `WASM_OPT=0` explicitly to keep compiler output.
The build never searches `PATH` or downloads a tool. Other profiles skip it unless
`WASM_OPT` is set. An invalid executable or failed optimization ends the build.
A smaller raw module can still have larger gzip or Brotli output; compare the
compressed files before enabling this postprocessing.

The browser gate checks a replay scene on both APIs in the shipping `web` profile,
as well as its broader `release` fixtures. It uses `WASM_OPT=0` by default; pass the
verified executable path to check postprocessed modules instead.

The official [version 125 release](https://github.com/WebAssembly/binaryen/releases/tag/version_125)
provides `binaryen-version_125-x86_64-linux.tar.gz`, whose SHA-256 is
`7c3bc16599c8274a04d34a504fe4be2047884f900e0e2da2f6fb9cd667183be4`.
Verify the archive before extracting it. Before executing `bin/wasm-opt`, the build
also verifies its SHA-256:
`23c1f4106e34eeaf44e888f947671f04d0fec12c232cca19cc5c4b3cf7c96f08`.
Only that executable is supported; other platforms can select `WASM_OPT=0`.

A cache served with `?cache=http` must include `index.txt` from the same build. Missing,
malformed, or oversized indexes end startup with an error. The browser uses the same bounded
index parser as native cache discovery and prepared-cache installation.
