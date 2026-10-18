# dbota

A Dota-looking client for bota, a simplified, deterministic
Dota 2 for bots and humans. With dbota a person can play a bota match, watch one live, or replay a
recorded one. It draws with Dota 2's own models, textures, icons and sounds, taken from a local
Dota 2 install. Bots don't need it.

## Setup

1. Put the bota and dbota repositories side by side; dbota builds against `../bota`.

2. Install Dota 2 through Steam (the regular or the Flatpak install is found automatically), then
   export the assets dbota uses into `~/.cache/dbota/v<N>` (`N` is `assets-manifest/version`; each
   export version keeps a folder of its own, so older and newer clients share a machine):

   ```sh
   cd dbota
   tools/import_assets.sh            # or: tools/import_assets.sh --dota-path "<...>/dota 2 beta"
   ```

   Valve's files stay in that cache for personal use; they are never part of this repository.

3. Build:

   ```sh
   cargo build --release             # binary: target/release/dbota
   cargo build --profile dist        # smaller, link-time optimised: target/dist/dbota (~6 min)
   (cd ../bota && cargo build --release -p bota_server)
   ```

   A live match needs a client and a server built from the same bota: a server that sees another wire
   fingerprint refuses the client. A replay opens only with the bota that recorded it.

## Play

```sh
../bota/target/release/bota_server --map 2      # listens on 127.0.0.1:4455
target/release/dbota --name alice
target/release/dbota --name bob
```

On the hero screen click a portrait, then Ready; the match starts once both seats are ready.
Alone: add `--dummy-seat` to the server. A bot is any program that speaks bota's protocol.

| Flag | Default | What it controls | Change it when |
|---|---|---|---|
| `--addr ADDR` | `127.0.0.1:4455` | the server: `host:port` (TCP), `ws://...` or `wss://...` | the server is elsewhere |
| `--name NAME` | `player` | the name the lobby shows | |
| `--spectate` | off | watch instead of playing | |
| `--replay FILE`, `--pov SEAT`, `--follow SEAT` | none | play a replay; as one seat sees it; camera on one seat's hero | |
| `--cache-dir DIR` | `$DBOTA_ASSETS`, else `~/.cache/dbota/v<N>` | the exported asset cache | |
| `--config FILE` | `~/.config/dbota/client.conf` | the settings file | |
| `--set NAME=VALUE` | none | one setting for this run, over the file's | |
| `--window WxH` | `1600x900` | the window's starting size | |
| `--no-vsync`, `--mute` | off | present without vsync; no sound | |

A server with reserved seats seats only the holder of a seat's key. dbota takes it from the
environment only, never from an argument: `BOTA_SEAT_KEY=<64 hex digits> target/release/dbota`.
With `--spectate` the key watches through that seat's eyes, in its fog. A refused client exits
with the reason. Plain TCP to another machine is fine on your LAN, not safe across the
internet (dbota says so on stderr); `wss://` trusts the public web's certificate authorities. `dbota --help` lists
the debug options too.

## Controls

| Key or mouse | Action |
|---|---|
| Right click | move, or attack what is under the cursor |
| Left click | select; confirms a targeted ability or item |
| `A` then left click | attack-move |
| `S` / `H` | stop / hold position |
| `Q` `W` `E` | Shadowraze near / mid / far |
| `R` | Requiem of Souls |
| `D` `F` | other ability slots |
| `1` … `6` | use the item in that slot |
| `T` | town portal scroll: take it up and click a building to teleport near it; `T` again goes to the fountain. Drag a scroll stack from the stash onto the small slot beside the inventory. Every teleport under way is ringed on the map and the minimap, for both sides |
| `B` | shop: left click an item to buy it; right click sets (or clears) the quick-buy item; drop an item on the strip at the panel's foot to sell it; in the search line `Enter` buys the first answer |
| `F5` | buy the quick-buy item |
| `Ctrl` + ability key | learn that ability (spend a skill point) |
| `Enter` | console line to the server (a live match on a server started with `--cheats`); `Enter` sends, `Esc` drops it |
| `Tab` | in a replay or as a spectator: switch vision (both teams, Radiant, Dire) |
| minimap | left click or drag moves the camera; right click orders your hero there |
| `F2` / `F3` | select courier (its bag shows in the panel) / courier delivers |
| `F1` | select your hero |
| `Space` | centre the camera on your hero |
| `Y` | lock the camera to your hero |
| Arrows, screen edges | pan the camera (all four edges, the HUD bars included); mouse wheel zooms |
| hold `Alt` | show attack, tower and ability ranges and the neutral camps' spawn boxes (they are never drawn otherwise) |
| `M` | mute |
| `F10` | settings: volume, damage numbers, your hero's voice answers, edge pan, picture quality; keys can be rebound by editing `~/.config/dbota/client.conf` |
| `Esc` | cancel / close / leave |

Keys are rebound by writing `key.<action> = <key>` lines in `~/.config/dbota/client.conf` (the
settings overlay changes volume and the toggles, not keys); a key is a letter, a digit, `F1`..`F12`,
`Space`, `Tab` or one of the punctuation names the file accepts. The actions and their keys out of
the box:

| action | key | action | key |
|---|---|---|---|
| `attack_move` | `A` | `town_portal` | `T` |
| `stop` | `S` | `shop` | `B` |
| `hold` | `H` | `select_hero` | `F1` |
| `ability_1` .. `ability_3` | `Q` `W` `E` | `select_courier` | `F2` |
| `ability_4`, `ability_5` | `D`, `F` | `courier_deliver` | `F3` |
| `ultimate` | `R` | `quick_buy` | `F5` |
| `item_1` .. `item_6` | `1` .. `6` | `camera_lock` | `Y` |
| `center_hero` | `Space` | `settings` | `F10` |
| `mute` | `M` | `show_ranges` | left `Alt` |

Talents: the small button left of the abilities opens the talent tree; when you have a point,
click a talent to learn it; the Attribute Bonus button right of the abilities spends a point on its next level. Buffs and debuffs of the selected unit sit above the abilities. The
stash shows at the home shop.

## Picture quality

For a slow machine, `~/.config/dbota/client.conf` (or `--set NAME=VALUE` for one run, as many
times as needed) sets what the picture may cost:

| setting | values | default |
|---|---|---|
| `quality` | `high`, `medium`, `low`, `potato`: sets all of the below at once | `high` |
| `shadows` | `off`, `low` (one cascade, 1024 texels), `high` (two, 2048) | `high` |
| `msaa` | `1`, `2`, `4` samples a pixel | `4` |
| `render_scale` | the 3D scene's resolution against the window's, `0.25` to `1`; the interface stays sharp | `1` |
| `texture_size` | largest texture side of models and the map; bigger ones are halved as they load | `16384` |
| `props` | the map's walls, statues, rocks and decals | `true` |
| `fps_cap` | frames a second at most, `0` for none | `0` |
| `gpu_batching` | cull and batch draws on the GPU with compute shaders; off for a software renderer, which runs them several times slower | `true` |
| `auto_quality` | lower multisampling, shadows, then resolution while frames run slower than the cap (or 30 fps) | `false` |

`medium` is shadows `low`, `msaa` 2 and 2048 textures; `low` adds no shadows, no multisampling,
`render_scale` 0.75 and 1024 textures; `potato` is `render_scale` 0.5, 512 textures, no props,
no GPU batching and a 30 fps cap. Later lines override earlier ones (`quality = low` then
`shadows = low`). `texture_size`, `props` and `gpu_batching` take effect on the next start; the
rest at once. `--window WxH` sets the window's starting size. With no GPU at all, Mesa's software
Vulkan driver (lavapipe, `VK_ICD_FILENAMES=.../lvp_icd.json`) draws it at `--set quality=potato`.

## Watch a live match

```sh
target/release/dbota --spectate --addr 127.0.0.1:4455
```

A replay or a spectator sees both teams' vision together (fog where neither side sees), as in Dota; `Tab` cycles both teams → Radiant → Dire, and the mode shows under the clock.

## Replays

The server records a match with `bota_server --replay FILE`.

```sh
target/release/dbota --replay match.brp              # whole map
target/release/dbota --replay match.brp --pov 0      # as seat 0 sees it (its fog, panel, shop)
target/release/dbota --replay match.brp --follow 1   # camera stays on seat 1's hero
```

`Space` pause, `=` / `-` (or the numpad's `+` / `-`) faster / slower, `.` one step, `Tab` switch vision.
A replay opens only with the bota version that recorded it.

The browser build is described in `web/README.md`: the page builds a private copy of the assets from
the player's own Dota 2 folder inside the browser (nothing is uploaded) and has a Playwright smoke test.

`dbota --help` lists everything else, including the debug options for screenshots.

## Checking the picture

`tools/visual_check.sh REPLAY.brp` measures dbota's picture of a recorded Map 2 match:
fogged ground at most 50% of its own unfogged luminance and 45% of its saturation, seen ground untouched, and a 1–5 cell border (each cell is measured drawn with the fog and without it, so any tick works; `VISUAL_TICK`, `VISUAL_RUNS` and `VISUAL_DIR` set the tick, the repeats of the hero-model check and the output folder), seen from both seats: every hero model loads whole and is drawn with detail, buildings stand as the map places them, enemy buildings are seen through the fog and the enemy base is fogged, no range rings without `Alt`, no gold or souls
in the top-left block, round buff icons. It needs the asset cache and a display, and exits non-zero when a check fails.

`tools/bench.sh REPLAY.brp` measures the binary's size, cold and warm start, and the frame times,
CPU, RAM and VRAM of a busy stretch of the replay and of a live match against a local
`bota_server` (settings at the top of the script). It rests on `--bench SECONDS`, which times
that many seconds of frames once the scene is whole and prints them on one `bench:` line.

## Reading Dota's files

`crates/dbota_assets` reads Valve's formats (VPK packages, compiled Source 2 resources, KV3,
textures as KTX2: block formats and mip levels kept, the first frame of a sprite sheet) straight
from a Dota 2 install, on the desktop and in a browser, and `import` makes the client's assets out of
them (models, textures, sounds; the browser page runs it in a worker through `crates/dbota_import`). Its golden tests
decode the files behind the asset cache and compare them with it; they need the install and the
cache, so they run only when asked:

```sh
cargo test --release -p dbota_assets -- --ignored   # DBOTA_DOTA_PATH, DBOTA_ASSETS: where they are
```

## License

MIT, see [LICENSE](LICENSE).
