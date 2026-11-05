# dbota

A client for bota, the deterministic Dota 2 simulator, that looks and plays like Dota 2. Play a bota
match against friends or bots, watch one live, or replay a recorded one, on the desktop or in a browser.

dbota draws with Dota 2's own models, map, textures, icons, sounds and texts, so a bota match looks like the real
game: Dota's HUD, shop, scoreboard, hotkeys and menus, heroes that talk with moving faces, Dota's particle effects.

## Valve's assets stay on your machine

dbota contains no Valve content, and none is ever shipped with it or committed to this repository. Every asset comes
from **your own Dota 2 install**: dbota converts the files it needs into a local cache on your computer (or, in a
browser, into the page's private storage, without uploading anything). Without that cache dbota shows only the screen
that makes it. What you convert is for your personal use, under Valve's terms for your copy of the game. dbota is not
affiliated with or endorsed by Valve.

## What you can do today

- **Play locally:** host a lobby of up to ten players on your machine or LAN, on the full Dota map or Solo Mid, or
  join one by address. Empty seats stay empty; one player is one hero.
- **Play online:** sign in to a radovid matchmaking service to queue for rated matches, accept or decline a found
  match, create lobbies or search the open ones and join at once (a locked one asks for its password), rejoin
  running games, and try heroes alone in Demo Hero with Dota's cheat panel.
- **Watch:** spectate live matches, or play replays with pause, speed, either team's vision or one player's view.
  Replays of your own matches are kept and listed for you.
- **Feel at home:** Dota's default hotkeys (all rebindable), the shop with its build trees and quick-buy, talents,
  the courier, control groups, pings, chat, Dota's chat cheats in practice games.
- **In a browser:** the same client compiled to WebAssembly, on WebGPU or WebGL2 (Chromium and Firefox).
- **On a slow machine:** quality presets down to `potato`, and Mesa's software renderer as a last resort.

What is missing: dbota can show only what bota simulates (two heroes, Shadow Fiend and Pudge; see bota's README).
Illusions, box selection and the secret shop are not there yet, some effects and animations are missing, and you will
meet rough edges. It is developed on Linux; other desktop systems are untested. The browser version has no local
lobby: it plays through a matchmaking service.

## Get started

You need Rust, Steam with Dota 2 installed, and the bota repository checked out next to this one (`../bota`).

1. Build the client and, beside it, the matching game server:

   ```sh
   tools/build.sh                # target/release/dbota and target/release/bota_server
   ```

   A client plays only with a server and replays from the same bota version, so keep the two files together.

2. Make the asset cache from your Dota 2 install (found automatically in the usual Steam places; about a minute):

   ```sh
   target/release/dbota import   # or: dbota import --dota-path "<...>/dota 2 beta"
   ```

   A friend without Dota 2 can use a cache you prepared with `dbota export-cache cache.zip` (it holds Valve's
   converted assets, so share it only where you may).

3. Play:

   ```sh
   target/release/dbota          # the main menu: LOCAL LOBBY, REPLAYS and SETTINGS work offline
   ```

   Or skip the menu: start `target/release/bota_server --map 2 --dummy-seat` and join with
   `target/release/dbota --addr 127.0.0.1:4455`. Open a replay with `dbota --replay match.brp` (`--pov 0` to watch
   through seat 0's eyes).

`dbota --help` lists every option. Settings live in `~/.config/dbota/client.conf` and on the SETTINGS screen.

## Controls

Dota 2's default hotkeys, rebindable on the HOTKEYS tab: right click to move or attack (on an own creep below half
health it denies, as Dota's "Right-Click Allies: To Deny" default), `Q` `W` `E` `D` `F` `R` for
abilities, `Z`–`N` for items, `F4` for the shop, `F1`–`F3` for hero and courier, `T` for a town portal scroll, `J` for
the Glyph, `` ` `` for the scoreboard, `Alt` to show ranges, `Alt`+click to ping, `Enter` to chat. In a replay: `F9`
or `Space` pause, `=` / `-` speed, `.` one tick, `Tab` switch vision.

## Settings

The VIDEO tab, `client.conf` or `--set NAME=VALUE` set what the picture may cost:

| Setting | Values | Default |
|---|---|---|
| `quality` | `high`, `medium`, `low`, `potato`: sets all of the below at once | `high` |
| `shadows` | `off`, `low`, `high` | `high` |
| `anti_aliasing` | `off`, `fxaa`, `smaa_low`, `smaa_medium`, `smaa_high`, `msaa_2x`, `msaa_4x` | `fxaa` |
| `render_scale` | 3D resolution against the window's, `0.25` to `1`; the interface stays sharp | `1` |
| `texture_size` | largest texture side, bigger ones are halved | `16384` |
| `props` | the map's walls, statues, rocks and decals | `true` |
| `fps_cap` | frames a second at most, `0` for none | `0` |
| `gpu_batching` | GPU culling and batching; off for a software renderer | `true` |
| `auto_quality` | lower settings while frames run slower than the cap (or 30 fps) | `false` |
| `visual_clarity` | set units apart from the ground: `off`, `low`, `high` | `off` |
| `animate_portrait` | the portrait plays the unit's idle and talks with its lines | `true` |
| `display_mode` | `windowed`, `borderless`, `fullscreen` | `borderless` |
| `camera_pitch`, `camera_fov` | camera angle and field of view, degrees | `58`, `25` |
| `camera_distance`, `camera_distance_min`, `camera_distance_max` | camera distance and zoom limits | `4400`, `1600`, `6500` |
| `camera_pan_speed` | pan speed at the starting distance | `3031` |

`texture_size`, `props` and `gpu_batching` apply on the next start, the rest at once. Without a GPU, Mesa's lavapipe
(`VK_ICD_FILENAMES=.../lvp_icd.json`) draws it at `--set quality=potato`. Hotkeys are `key.<action> = <chord>` lines in
the same file, which also holds every other setting (sound, interface, controls, replays) by name.

## For developers

[DEVELOPMENT.md](DEVELOPMENT.md) covers building, the command line for automation, the picture checks, the merge
gate and headless runs. [tools/README.md](tools/README.md) is the asset import and cache;
[web/README.md](web/README.md) is the browser build.

## License

MIT, see [LICENSE](LICENSE). The interface fonts in `assets/fonts` are subsets of Liberation Sans and Noto Sans CJK
under the SIL Open Font License (`OFL.txt`, `OFL-NotoSansCJK.txt`).
