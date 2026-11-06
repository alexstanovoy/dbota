# Developing dbota

dbota is a Bevy app over `bota_client_core` (from `../bota`), with the asset importer `crates/dbota_import` built on
the heatel crate (`../heatel`), which reads Valve's formats. The asset pipeline is in [tools/README.md](tools/README.md),
the browser build in [web/README.md](web/README.md).

## Building

```sh
tools/build.sh                    # target/release/dbota and target/release/bota_server of ../bota
PROFILE=dist tools/build.sh       # smaller, link-time optimised, in target/dist (~6 min)
```

A live match needs a client and a server of the same wire fingerprint. LOCAL LOBBY looks for `bota_server` beside the
client, then in the settings' path or `$BOTA_SERVER`, then on the `PATH`, and skips a server of another wire.

The asset cache is stamped with the asset fingerprint of the build that made it (cache version, importer revision,
manifest hash) and lives in `~/.cache/dbota/<fingerprint>` (`dbota cache-dir`), so builds of different versions share a
machine; recently used caches are retained as described below. A dbota of another fingerprint opens at "the assets are out of date" with a
button that imports again. The settings screen shows the client's commit and the fingerprint it needs.

Automatic asset-cache pruning is confined to the default managed root, `~/.cache/dbota`.
A managed root must be a real directory: symlinking that directory to another disk disables
its pruning, with a diagnostic. Cache parents must remain under the user's control; the
filesystem checks do not isolate concurrent changes by other processes with the same access.

A client records when it last uses a cache. Pruning retains recently used caches for 24 hours
by default and bounds the number of managed caches to eight. Completed cache writes and
up-to-date imports maintain this root. Legacy directories are adopted only after their full
name, bounded index, and build stamp validate; invalid directories and custom cache locations
carry no deletion authority. Ownership and last-use metadata are excluded from asset packs.
If all eight slots are recently used, another managed import is refused before its cache is
created. Expired caches release slots first. Managed imports and startup use updates serialize
on the root's lock; a cache with unsafe contents stays in place and is reported while pruning
continues past it.

## Running it for tests and tools

`dbota --help` lists every option, the debug ones included (`--screenshot-at`, `--bench`, `--visual-check`). A seat key
comes only from the environment: `BOTA_SEAT_KEY=<64 hex> dbota --addr ...`; with `--spectate` it watches through that
seat's eyes.

`--script "STEP;STEP;..."` drives the window like a person, for end-to-end tests (`tests/menu_quit.rs`): `key:NAME`,
`text:TEXT`, `mouse:X,Y` (shares of the window), `lclick`, `rclick`, `down:KEY` / `up:KEY`, `click:BUTTON` (the label in
lower case; in a game `pick ready`, `pick <hero>`), `await:menu|game|popup|nopopup|button:NAME`, `shot:FILE`,
`wait:SECONDS`. A dimmed button or a wait over two minutes ends the run with exit code 2.

Matchmaking on one machine: use the game-specific [launch guide](../bota/crates/bota_radovid/matchmaking.md)
and example beside `bota_radovid`, then
`dbota --service wss://localhost:4460 --trust-cert dev/cert.pem`. A match needs a second account in the same ruleset
(a bot client, or a second dbota with its own `--config` and `XDG_DATA_HOME`). Rebuild control clients with
protocol 14 together with the service. Play shows an explicit unlimited account allowance; missing limit data
is kept distinct from an unlimited allowance.

Files the client keeps: settings in `~/.config/dbota/settings.conf`, the service login in `~/.config/dbota/session` (owner
only), replays in `$XDG_DATA_HOME/dbota/replays` (`~/Library/Application Support/dbota/replays` on macOS,
`%LOCALAPPDATA%\dbota\replays` on Windows) as `YYYYMMDD-HHMMSS-mode-matchid.brp` with an `index.txt`, and in a browser
in OPFS (else IndexedDB).

### Without windows on the desktop

`tools/headless.sh COMMAND...` runs a command in a private headless Weston (its GL renderer on the GPU) that lives as
long as the command: the client draws as on a desktop and its screenshots are read back from its own frames, but no
window shows. `visual_check.sh`, `bench.sh` and the drawing tests use it by themselves; put it in front of your own
captures:

```sh
tools/headless.sh target/release/dbota --replay match.brp --screenshot-at 4000 --display-mode windowed
```

`DBOTA_SHOW=1` shows the windows instead; `DBOTA_HEADLESS_SIZE=WxH` (default 1920x1080) and `DBOTA_HEADLESS_SCALE=N`
set the virtual output. Headless frame times are steadier than a window on a busy desktop: compare benches made the
same way.

### Window and cursor

The game opens borderless (windowed fullscreen); Wayland has no exclusive fullscreen, so `fullscreen` shows as
`borderless`. Captures want `--display-mode windowed` for a window of `--window` size. During a match the cursor is
confined to the window so the edges pan the camera; it is released in menus and on focus loss, and stays free with a
logged warning where the compositor can't confine it. In a browser the page locks the pointer instead (only on a user
gesture) and draws its own cursor.

## Checks

- `tools/visual_check.sh REPLAY.brp` measures the picture of a recorded Map 2 match from both seats: fog darkening and
  its border, every hero model whole on its skeleton, buildings where the map places them, no range rings without
  `Alt`, health bars in side colours, and each hero's talking portrait (`--talk-check HERO`). It needs the asset cache
  and exits non-zero on a failed check.
- `tools/bench.sh REPLAY.brp` measures binary size, cold and warm start, and frame times, CPU, RAM and VRAM of a busy
  replay stretch and of a live match (`--bench SECONDS` underneath). Like `tools/test_client.sh`, which the visual and
  browser checks start the client through, it needs `DBOTA_ASSETS` and runs the client on a private home.
- `tools/precheck.sh` is the quick loop (clippy and tests of what changed); it stamps a green tree.
- `tools/gate.sh [REPLAY.brp]` is the merge gate: an audit that every ignored test runs somewhere, fmt, clippy, the fuzz
  targets' type check, both browser clients, an asset cache of this build (imported once per stamp into
  `~/.cache/dbota-gate/<stamp>`, or `DBOTA_GATE_ASSETS`), the tests, the release build, `cargo deny check`, and with a
  Dota 2 install the importer's golden tests, the visual check and the browser smoke test in headless Chromium and
  Firefox. It stops at the first failure and prints `GATE-OK`; a step that can't run fails it. `GATE_LEAN=1` runs the
  quick subset and says it is not a pass.

## Reading Dota's files

heatel reads Valve's formats (VPK packages, compiled Source 2 resources, KV3, textures, models and animations, worlds,
sounds, KeyValues texts) on the desktop and in a browser. `crates/dbota_import` makes the whole asset cache from the
manifest (`assets-manifest/manifest.txt`); the browser runs the same code in a worker. Its golden tests compare the
output with a reference cache exported by Source2Viewer 20 and need the install and that cache:

```sh
cargo test --release -p dbota_import -- --ignored   # DBOTA_DOTA_PATH, DBOTA_REFERENCE: where they are
```

The fonts in `assets/fonts` are made by `tools/subset_fonts.py`; other scripts come from the system's fonts on the
desktop, and a browser has only the shipped ones.

This keeps accidents out; it is not a sandbox against code that means to leave (a process can still resolve the
system's home from its user id). Nothing that tests, drives or measures the client may touch the user's settings, replays (the replay library prunes the
oldest beyond its limits), `~/.cache/dbota` or the variables that steer the client. Two layers hold that:

- Structure: `tools/precheck.sh`, `tools/gate.sh` and `tools/headless.sh` run the tests and everything started under
  them on a private home (`in_private_environment` in `tools/client_env.sh`: `HOME` and the XDG folders in a scratch
  folder, `CARGO_HOME`, `RUSTUP_HOME` and the Dota install, `DBOTA_DOTA_PATH`, kept), so even a test that starts the
  client carelessly cannot reach real data. Run your own `cargo test` the same way (`in_private_environment cargo test
  ...`). `DBOTA_SHOW=1` shows windows with your own settings and is for looking only.
- Convention: the headless E2E client takes `DBOTA_ASSETS`, a test asset cache that is no part of `~/.cache/dbota`, and a
  private folder for its files; the tests that start the client binary use `tests/common`, the tools
  `tools/test_client.sh` or `tools/client_env.sh` (the scrubbed variables are listed in `tools/client_environment.txt`).
  `tests/client_guard.rs` is a lint over the sources for the usual ways round them (it cannot be complete: the first
  layer is what holds).
