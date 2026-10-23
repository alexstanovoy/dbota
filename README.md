# dbota

A Dota-looking client for bota, a simplified, deterministic
Dota 2 for bots and humans. With dbota a person can play a bota match, watch one live, or replay a
recorded one. It draws with Dota 2's own models, textures, icons and sounds, taken from a local
Dota 2 install. Bots don't need it.

## Setup

1. Put the bota and dbota repositories side by side; dbota builds against `../bota`.

2. Build:

   ```sh
   cargo build --release             # binary: target/release/dbota
   cargo build --profile dist        # smaller, link-time optimised: target/dist/dbota (~6 min)
   (cd ../bota && cargo build --release -p bota_server)
   ```

   A live match needs a client and a server built from the same bota: a server that sees another wire
   fingerprint refuses the client. A replay opens only with the bota that recorded it.

3. Install Dota 2 through Steam (the regular or the Flatpak install is found automatically), then
   make the asset cache from it, one command, about a minute:

   ```sh
   target/release/dbota import       # or: dbota import --dota-path "<...>/dota 2 beta"
   ```

   It converts the models, map, textures, sounds and texts dbota uses into `~/.cache/dbota/v<N>`
   (`N` is `assets-manifest/version`; each version keeps a folder of its own, so older and newer
   clients share a machine). Valve's files stay in that cache for personal use; they are never part
   of this repository. Without it dbota draws stand-ins. See `tools/README.md`; a
   `~/.cache/dbota/tools` folder left by the old export script can be deleted.

## Play

`target/release/dbota` with no game on its command line opens the main menu, laid out as Dota's:

- **Sign in / create account** on a matchmaking service (`--service wss://host:port`, remembered; the
  service and `wss://` games are trusted by the public web's roots, or `--trust-cert FILE.pem` for one's
  own). The login is kept for the next run in `~/.config/dbota/session`, readable by its owner only; LOG OUT
  forgets it. There is no password recovery.
- **PLAY**: find a match per ruleset. The search box shows the time searching, the place in the queue and
  when the game servers are full; MATCH FOUND asks to accept or decline within the time shown (declining
  keeps the account out of the queues for a while); then the game opens by itself.
- **MY GAMES**: rejoin or watch the account's running matches (an account may run several at once), cancel
  searches. **LOBBIES**: create one, join one by name and password, or from the list. Then a result screen.
- **DIRECT CONNECT**: a game server's address, `host:port` or `tcp://` (TCP), `ws://` or `wss://`, with no
  service at all. Plain TCP to another machine is unprotected, and dbota says so once per host.

In a game from the menu, `Esc` once the match is over (or on the hero screen) goes back to the menu.
`--addr`, `--spectate`, a seat key or `--replay` skip the menu, as below.

Two people without a service: start a server and connect two dbota clients (from another machine use
`--addr <host>:4455`, or DIRECT CONNECT in the menu):

```sh
../bota/target/release/bota_server --map 2      # listens on 127.0.0.1:4455
target/release/dbota --addr 127.0.0.1:4455 --name alice
target/release/dbota --addr 127.0.0.1:4455 --name bob
```

On the hero screen click a portrait, then Ready; the match starts once both seats are ready.
Alone: add `--dummy-seat` to the server. A bot is any program that speaks bota's protocol.

| Flag | Default | What it controls | Change it when |
|---|---|---|---|
| `--addr ADDR` | none (the main menu opens) | the server to join directly: `host:port` (TCP), `ws://...` or `wss://...`; `127.0.0.1:4455` with `--spectate` or a seat key | the server is elsewhere |
| `--service URL`, `--trust-cert FILE` | remembered | the matchmaking service the menu signs in to; a certificate of one's own to trust | |
| `--name NAME` | `player` | the name the lobby shows | |
| `--spectate` | off | watch instead of playing | |
| `--replay FILE`, `--pov SEAT`, `--follow SEAT` | none | play a replay; as one seat sees it; camera on one seat's hero | |
| `--cache-dir DIR` | `$DBOTA_ASSETS`, else `~/.cache/dbota/v<N>` | the exported asset cache | |
| `--config FILE` | `~/.config/dbota/client.conf` | the settings file | |
| `--set NAME=VALUE` | none | one setting for this run, over the file's | |
| `--window WxH` | `1600x900` | the window's size when windowed | |
| `--display-mode MODE` | `borderless` | `windowed`, `borderless` (windowed fullscreen) or `fullscreen` (exclusive); over the setting `display_mode` | |
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
| `B` | shop, laid out as Dota's: the BASICS, UPGRADES and NEUTRALS tabs with Dota's categories (from the asset cache; bota's own groups without it), a search by name or Dota's aliases (`tp`, `bfly`). Left click an item to buy it and see its build tree (its parts and recipe below it, what it builds into above; a click in the tree buys that piece); right click or shift-click sets (or clears) the quick-buy item, whose missing parts the quick-buy row in the corner offers; drop an item on the panel's foot to sell it; in the search line `Enter` buys the first answer. Items bota does not have are left out |
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
| `F10` | settings: volume, damage numbers, your hero's voice answers, edge pan, display mode, picture quality, visual clarity; keys can be rebound by editing `~/.config/dbota/client.conf` |
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
stash shows in the shop's corner while the shop is open or at the home shop; there GRAB ALL moves
what it holds into the bag, a slot a tick.

## Window and cursor

On a desktop the game opens as windowed fullscreen, as Dota does. `display_mode` in
`~/.config/dbota/client.conf` (or the F10 overlay, or `--display-mode` for one run) is `windowed`,
`borderless` (windowed fullscreen, the default) or `fullscreen` (exclusive); it switches while
the game runs. On Wayland there is no exclusive fullscreen (the compositor owns the video mode),
so `fullscreen` shows as `borderless`. Captures (`--screenshot-at`, `--bench`) want
`--display-mode windowed` for a window of the `--window` size.

While a match is on screen and the window has focus the cursor is confined to the window, so the
screen edges pan the camera on every side. It is let go in the menus, with the F10 overlay open
and when the window loses focus (Alt-Tab), and is not confined during captures. Where the platform
refuses to confine (a Wayland compositor without pointer constraints), a warning is logged and
the cursor stays free; the cursor is never locked instead. A browser keeps its canvas in the page:
none of this applies there.

## Picture quality

For a slow machine, `~/.config/dbota/client.conf` (or `--set NAME=VALUE` for one run, as many
times as needed) sets what the picture may cost:

| setting | values | default |
|---|---|---|
| `quality` | `high`, `medium`, `low`, `potato`: sets all of the below at once | `high` |
| `shadows` | `off`, `low` (one cascade, 1024 texels), `high` (two, 2048) | `high` |
| `anti_aliasing` | `off`, `fxaa`, `smaa_low`, `smaa_medium`, `smaa_high` (all three over the finished picture), `msaa_2x`, `msaa_4x` (geometry edges only); every one runs in a browser too | `fxaa` |
| `render_scale` | the 3D scene's resolution against the window's, `0.25` to `1`; the interface stays sharp | `1` |
| `texture_size` | largest texture side of models and the map; bigger ones are halved as they load | `16384` |
| `props` | the map's walls, statues, rocks and decals | `true` |
| `fps_cap` | frames a second at most, `0` for none | `0` |
| `gpu_batching` | cull and batch draws on the GPU with compute shaders; off for a software renderer, which runs them several times slower | `true` |
| `auto_quality` | lower anti-aliasing, shadows, then resolution while frames run slower than the cap (or 30 fps) | `false` |
| `visual_clarity` | sets units apart from the ground: the scenery a little duller and darker, shade around the forest, a soft dark disc under every unit and a faint light round its rim; `off`, `low` or `high` (F10 steps through them) | `high` |
| `animate_portrait` | Dota's "Animate Portrait": the bottom panel's portrait plays the unit's idle animation, drawn up to 30 times a second; off, it shows Dota's picture of a hero, or a render of another unit's model made once | `false` |

`medium` is shadows `low`, `anti_aliasing` `fxaa` and 2048 textures; `low` adds no shadows, no anti-aliasing,
`render_scale` 0.75 and 1024 textures; `potato` is `render_scale` 0.5, 512 textures, no props,
no GPU batching and a 30 fps cap. Later lines override earlier ones (`quality = low` then
`shadows = low`). `texture_size`, `props` and `gpu_batching` take effect on the next start; the
rest at once. `--window WxH` sets the window's starting size. With no GPU at all, Mesa's software
Vulkan driver (lavapipe, `VK_ICD_FILENAMES=.../lvp_icd.json`) draws it at `--set quality=potato`.

## Watch a live match

```sh
target/release/dbota --spectate --addr 127.0.0.1:4455
```

A replay or a spectator sees both teams' vision together (fog where neither side sees), as in Dota; `Tab` cycles both teams → Radiant → Dire, and the mode shows under the clock. Neutral creeps stand dulled in that fog too (and show on the minimap only where the vision sees them); a spectator watching through a seat's eyes never gets them there.

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
fogged ground at most 50% of its own unfogged luminance and 45% of its saturation, seen ground untouched, and a 1–5 cell border (each cell is measured drawn with the fog and without it, so any tick works; `VISUAL_TICK`, `VISUAL_RUNS` and `VISUAL_DIR` set the tick, the repeats of the hero-model check (cold and warm, the animated portrait on and off) and the output folder), seen from both seats: every hero model loads whole, each part on the hero's own skeleton and no scene replaced once drawn, and is drawn with detail, buildings stand as the map places them, enemy buildings are seen through the fog and the enemy base is fogged, no range rings without `Alt`, no gold or souls
in the top-left block, round buff icons. It needs the asset cache and a display, and exits non-zero when a check fails.

`tools/bench.sh REPLAY.brp` measures the binary's size, cold and warm start, and the frame times,
CPU, RAM and VRAM of a busy stretch of the replay and of a live match against a local
`bota_server` (settings at the top of the script). It rests on `--bench SECONDS`, which times
that many seconds of frames once the scene is whole and prints them on one `bench:` line.

## Reading Dota's files

`crates/dbota_assets` reads Valve's formats (VPK packages, compiled Source 2 resources, KV3,
textures, models and animations, the world, sounds, KeyValues texts) straight from a Dota 2
install, on the desktop and in a browser, and `import` makes the whole asset cache out of them
(`dbota import`; the browser page runs the same code in a worker through `crates/dbota_import`).
Its golden tests compare what it makes with a reference cache exported by Source2Viewer 20 (manifest
version 6, `~/.cache/dbota/v6`); they need the install and that cache, so they run only when asked:

```sh
cargo test --release -p dbota_assets -- --ignored   # DBOTA_DOTA_PATH, DBOTA_REFERENCE: where they are
```

## License

MIT, see [LICENSE](LICENSE).
