# dbota in a browser

The page is a thin shell around the client (compiled to wasm, drawn through WebGPU or WebGL2): everything a player
meets, the first run included, is drawn by the client in its canvas, as on the desktop. Without a copy of the assets
the client opens at its first-run screen (`src/firstrun.rs`, the same screen as the native one): what dbota is, that
it needs the player's own Dota 2 (free on Steam), "CHOOSE THE DOTA 2 FOLDER", the import with its progress and CANCEL,
then the page loads again on the copy, at the sign-in screen. **Valve's files never leave the player's machine**: the
folder is read inside the tab with ranged reads, the copy lives in the browser's Origin Private File System, nothing
sends it anywhere, and the page's Content-Security-Policy allows connections only to its own origin and the game
servers. The settings' RE-IMPORT GAME FILES brings the screen back.

## What stays in JavaScript, and why

The client cannot do these itself:

| File | Why it is JavaScript |
| --- | --- |
| `index.html`, `page.css` | the canvas, and the page's own screens for what happens before the client runs or after it stopped (a browser that lacks what it needs, a download that fails, a panic: a wasm that panicked can no longer draw); the CSP |
| `boot.js` | a classic script, so that it runs where modules cannot (a browser too old, a module that did not load): "Loading" at once and the one failure screen, never a blank page |
| `main.js` | the loader: what the browser lacks (private storage, a folder dialog, workers, WebGPU or WebGL 2: without them the client cannot start, so the page says so and that a desktop dbota exists), which of the two clients to download (Bevy fixes its graphics API at compile time), the download with its progress and limits, and the end of a client that stopped |
| `busy.js` | a server busy sending pages answers 503 with Retry-After: the loader's fetches, modules and the import worker's module go through here, wait as asked (1 to 60 s) and try once more |
| `import_worker.js` | a worker starts from a script: these few lines load the importer (`crates/dbota_import_web`, Rust) and hand it the worker; the import itself is Rust |
| `voice.js`, `voice_worker.js`, `bounded.js` | the hero's voice packs read off the page's thread (a worker's script again) |
| `replays.js` | the player's replays in OPFS (else IndexedDB), published as `globalThis.dbota_replays` |
| `audio.js`, `keys.js`, `pointer.js` | the browser's own rules: an AudioContext resumed by a gesture, which keys are the browser's, the pointer lock |
| `native.js` | where the native client's download links point: none (no links shown) unless `web/build.sh` is given `DBOTA_NATIVE_DOWNLOADS` |
| `report.js` | `?report`: console lines to the page's server, for local debugging |

The Rust side: `src/firstrun.rs` with `src/firstrun/browser.rs` (the folder dialog: the directory picker where the
browser has one, else a folder upload field; the worker; loading the page again) and `src/firstrun/look.rs` (the
screen); `src/webcache.rs` (the copy, found before the app is built and read as the client asks);
`crates/dbota_import_web` (the import worker); `crates/dbota_import/src/browser.rs` (the copy's layout, which files of
a picked folder are read).

## The copy in private storage

The Origin Private File System holds `dbota/current.txt`, naming the finished copy, and the copy in `dbota/copy-<time>/`:
its files, `index.txt` and `stamp.txt` (the asset fingerprint of the build that made it, then the size and modification
time of `pak01_dir.vpk`). A whole install's copy is about 1.1 GB. An import writes a new folder and names it in
`current.txt` with a writable file stream after its index and stamp are written. The browser commits that pointer
when the stream closes. Web Locks serialize imports across tabs, and an import preserves the previous copy until
publication succeeds. Unfinished copies are reclaimed under the lock. A copy another
build made, one of an older layout, or one without its index is not read: the first-run screen asks for the folder
again. Before it starts, an import asks the browser for as much room as an import may write (1.6 GB) and says how much
there is when the browser gives less. As it starts, the client asks the browser to keep the copy for good
(`navigator.storage.persist()`: Firefox asks the player, Chromium decides by itself): a browser short of disk space may
otherwise evict the site's storage, and the client then opens at the first-run screen again.

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

## Build and run

```sh
rustup target add wasm32-unknown-unknown; cargo install --locked wasm-bindgen-cli --version <Cargo.lock's>   # `web/build.sh --check` names it
WASM_OPT=0 DBOTA_WEB_REPLAY=$PWD/match.brp web/build.sh        # PROFILE=release for a quicker build
python3 -m http.server --bind 127.0.0.1 --directory web/out 8080   # the page alone, to look at it
```

For play, a matchmaking service serves the page on the origin of its control socket (radovid: `[web] static_dir`, read-
only files over its TLS), so a player opens one URL; games run over secure WebSockets of their own hosts. `build.sh`
copies `assets/` into the folder rather than linking it, since links out of the folder aren't followed. A page served
elsewhere falls back to its host on port 4460. The page plays the replay built into it (`DBOTA_WEB_REPLAY`, part of the
program's memory, so keep it short; `DBOTA_WEB_ARGS` stand in for a command line). `?cache=http` plays an asset cache
served under `/cache` beside the page (the client fetches its `index.txt` and reads nothing it does not list). A script
of the page itself (the tests' init scripts, never a link) may set `globalThis.dbota_args` to more arguments before the
client loads: the tests drive the first run with `--script`. `OUT=dir` builds elsewhere; `APIS=webgpu` builds one client only.

## Tests

Chromium checks use software Vulkan (SwiftShader) by default. Set `DBOTA_BROWSER_GPU=hardware`
to request the local GPU; both modes run the same checks.

`web/test/first_run.py` and `web/test/smoke.py` drive headless Chromium (and Firefox) with Playwright (`pip install
--target temp/pwlib playwright && PYTHONPATH=temp/pwlib python3 -m playwright install chromium`); their docstrings have
the build commands of each check.

- `first_run.py --page`: the first run on a tiny install: welcome, a folder without Dota refused, the import, sign-in;
  RE-IMPORT GAME FILES; copies of another build, without index, with a stamp too long, evicted; a refused write; the
  directory picker; a browser without storage, folder dialog or graphics; a storage that does not answer; served
  indexes that are missing, wrong or endless. Nothing is requested beyond the page's origin.
- `first_run.py --replay-page` (a Dota 2 install): CANCEL mid-import leaves nothing, the full import while the frame
  keeps drawing, every icon equal to a desktop import's, the map drawn from the copy, a stale copy imported again;
  the import's time, the browser's memory and the longest task of the page's thread printed.
- `smoke.py --launch-page`: the WebGL2 client starts and draws its first frames with no panic (fails at once with the message).
- `smoke.py --replays-page`: a match through a real radovid that serves the page and keeps replays; the next visit
  fetches the replay, lists it, plays it with Watch and deletes it. File import and download controls are absent.
- `smoke.py --busy-page`: a server that answers 503 with Retry-After once gets the page's own script, the game's download
  and the import worker's module on the second try, with the wait said on the page; a download refused twice is a failure.
- `smoke.py --replay-page`: a build with a packed replay plays it with chat and the CJK font fallback.

The browser build's lint (`web/lint/clippy.toml`, run by `tools/precheck.sh` on the wasm target) denies the std
clocks and threads, which panic in a browser; time comes from `bevy::platform::time::Instant` or `replays::wall_clock`.

The merge gate runs them all in its `browser` step (`tools/browser_check.sh`).

## Manual checks (the browsers' own folder dialogs are out of automation's reach)

1. Serve `web/out` and open it in Chrome: the client's welcome screen, the privacy text, "CHOOSE THE DOTA 2 FOLDER",
   and no way to play without assets.
2. Pick `dota 2 beta`, then `game`, `dota` and the Steam library (all accepted), then a wrong folder (a message, no
   import). CANCEL returns to the question and leaves no copy (devtools, Application, Storage).
3. After the import the page loads again at sign-in; devtools shows the copy in OPFS and no request beyond the page's
   own files.
4. Firefox and Safari use the folder upload: the browser says it will "upload" the folder, which only lists it; watch
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
malformed, or oversized indexes end startup on the page's failure screen. The client uses the
same bounded index parser as native cache discovery.
