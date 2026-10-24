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

What needs no service needs no account either: LOCAL LOBBY, DIRECT CONNECT, REPLAYS and SETTINGS work
signed out and with no service reachable. There, you play under the profile name of the settings
(OPTIONS, PROFILE), as Dota's local games show your Steam name.

- **LOCAL LOBBY**: a game hosted on this machine, as Dota's local lobby: dbota starts `bota_server`
  (`$BOTA_SERVER`, else beside `dbota`, else on the `PATH`) for so many players on a port, for this machine
  alone or (CREATE FOR THE LAN) for the local network too, in plain text, and seats you. The others join by
  this machine's address under DIRECT CONNECT, which the hero screen and the LOCAL LOBBY page show while a
  seat is open. The server ends when you quit dbota. The seats nobody else takes (**Empty seats**) stay
  OPEN, or take the server's IDLE DUMMY (a hero that stands at its fountain and never acts, to practise or
  test alone), or a BOT: the program set as the bot command (SETTINGS, Options, LOCAL LOBBY; `bot_command`
  in the settings file), run once for each such seat when the lobby opens and ended with it. It is run
  without a shell: its words split at blanks, `'…'` and `"…"` keep blanks inside, `{address}` becomes the
  game's address (`127.0.0.1:port`), and the seat's key comes in the environment as `BOTA_SEAT_KEY` (64 hex
  digits; never an argument, which other users see in the process list). Any program that speaks bota's
  protocol can be the bot, for example `bot_command = /path/to/mybot --server {address}`.
- **REPLAYS**: the replays in `~/.local/share/dbota/replays` (`$XDG_DATA_HOME/dbota/replays`), where a local
  lobby keeps its games', the newest first, each with its date, its heroes and how it ended (read from the
  file on a thread of its own); a file that cannot be watched says why (damaged, or recorded by another
  version), and an empty one (a game that never started) is not listed. Any replay file opens by its path.

The power button at the top right quits, after Dota's question ("Confirm Quit"); `Esc` on the menu
backs out of a page to the top one, and there asks the same. `Esc` in a game (with nothing to cancel or
close) opens the dashboard over it, as Dota's: RETURN TO GAME (also the arrow at the top left, or `Esc`
again), DISCONNECT, and the power button. DISCONNECT leaves a seat at once and keeps it: RECONNECT takes
the same seat again (a direct or local game claims its seat with a key of its own); a game watched or a
replay is left after asking. `Esc` in a match that is over goes back to the menu. `--addr`, `--spectate`,
a seat key or `--replay` open that game first, with the menu under it.

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

The keys are Dota 2's defaults (its "Default" hotkeys, copied from Dota's own key table), and
every one can be bound anew on the settings screen's HOTKEYS tab, which shows the bindings that
clash and resets them to the defaults; they are kept as `key.<action> = <chord>` lines in
`~/.config/dbota/client.conf` (a chord such as `F4`, `Shift+Enter`, `Mouse3`, or `none`).

| Key or mouse | Action |
|---|---|
| Right click | move, or attack what is under the cursor |
| Left click | select; confirms a targeted ability or item |
| `A` / `M` then left click | attack-move / move there, whatever is under the cursor |
| `S` / `H` | stop / hold position |
| `Q` `W` `E` `D` `F`, `R` | the ability slots (Shadowraze near, mid, far, Feast of Souls), the ultimate |
| `O`, then an ability key; or `Ctrl` + an ability key | learn that ability (spend a skill point) |
| `U` | open the talent tree (the Attribute Bonus is learned there too) |
| `Z` `X` `C` `V` `B` `N` | use the item in that inventory slot |
| `T` | town portal scroll: take it up and click a building to teleport near it; `T` again goes to the fountain. Drag a scroll stack from the stash onto the round slot beside the inventory. Every teleport under way is ringed on the map and the minimap, for both sides |
| `F4` | shop, laid out as Dota's: the BASICS, UPGRADES and NEUTRALS tabs with Dota's categories (from the asset cache; bota's own groups without it), a search by name or Dota's aliases (`tp`, `bfly`). Left click an item to buy it and see its build tree (its parts and recipe below it, what it builds into above; a click in the tree buys that piece); right click or shift-click sets (or clears) the quick-buy item, whose missing parts the quick-buy row in the corner offers; drop an item on the panel's foot to sell it; in the search line `Enter` buys the first answer. Items bota does not have are left out |
| `F5` / `F8` | buy the quick-buy item / the town portal scroll kept beside it |
| `F1` | select your hero; held, the camera follows it |
| `F2` / `F3` | select courier (its bag shows in the panel) / courier delivers; its speed burst and shield have buttons and unbound keys |
| `1` … `0`, `Ctrl` + `1` … `0` | select a control group / put the selected unit in it |
| `J` | Glyph of Fortification |
| hold `` ` `` | the scoreboard (the menu's button at the top left pins it open, `Esc` lets it go) |
| `\` | console line to the server (a live match on a server started with `--cheats`); `Enter` sends, `Esc` drops it |
| `Enter` / `Shift+Enter` | chat to your allies / to everyone (a spectator: to the spectators); `Enter` sends, `Esc` drops the line. Text of any script goes in, with an input method too. The log shows over the minimap and fades after ten seconds; it shows in replays as the watched side heard it |
| `Alt` + left click / `Ctrl`+`Alt` + left click | ping the spot or the unit under the cursor (on the minimap, the spot) for your side, or warn of danger there (red). Your side's pings show for three seconds on the ground and the minimap, in the pinger's colour, and in a replay seen through a seat's eyes |
| minimap | left click or drag moves the camera; right click orders your hero there |
| Arrows, screen edges, middle button held | pan the camera (all four edges, the HUD bars included; the middle button drags the map). The camera is Dota's (1200 units out, 60° down, a 70° lens); the mouse wheel brings it in to 1134, and a replay or a spectator may draw it back to 6500 |
| hold `Alt` | show attack, tower and ability ranges and the neutral camps' spawn boxes (they are never drawn otherwise) |
| `Esc` | cancel / close; with nothing open, the dashboard (DISCONNECT, quit, SETTINGS; the settings also from the gear at the top left); leaves a match that is over; on the menu, back, and on its top page quit |
| `F9`, `=` / `-`, `.`, `Tab` | in a replay: pause, faster / slower, one tick, switch vision (both teams, Radiant, Dire; `Tab` as a spectator too) |

The settings screen (SETTINGS in the main menu and the dashboard, the gear in a game) has Dota's tabs: HOTKEYS,
OPTIONS (profile name, interface size, damage numbers, animated portrait, the scoreboard's items, edge pan), VIDEO (display mode, frame
cap, resolution, quality preset, shadows, anti-aliasing, visual clarity, auto quality, and what
takes effect on the next start) and AUDIO (volume, mute, your hero's voice answers). What it
changes is saved when it closes.

The hero panel is laid out as Dota's: the portrait with the unit's numbers over it (hover them for
the stats tooltip), the level ring, the abilities with a gold plate over each one a point could go
into (click it, or Ctrl + the ability's key, to learn a level), the inventory with its round
neutral and town portal slots, and the gold (click it to open the shop) beside the courier's
buttons. Talents: the round button left of the abilities opens the talent tree, lit while you have
a point for it; click a talent, or the Attribute Bonus under the tiers, to learn it. The Glyph of
Fortification is the round button against the minimap. Buffs and debuffs of the selected unit sit
above the abilities. The stash shows in the shop's corner while the shop is open or at the home
shop; there GRAB ALL moves what it holds into the bag, a slot a tick. A dead hero's portrait, in
the panel and in the top bar (either side's), turns grey with its respawn seconds over it. Health
bars are the side's colour whoever watches: Radiant green, Dire red.

The scoreboard is Dota's, at its sizes: each side's kills and name, and a row for each seat with
its colour, hero, player's and hero's names, talents, level, gold, kills, deaths, assists and
ultimate (Dota's columns: last hits and denies are one's own, at the top left); beside the rows,
while its Show Items button is on, every seat's inventory, backpack and neutral slot. In a match
it tells of the other side what Dota tells: the score, the level, and its hero's items as they
were when last seen (dimmed while out of sight); never its gold or ultimate. A replay (as a seat too) shows both sides in full, a spectator all its view tells.

## Window and cursor

On a desktop the game opens as windowed fullscreen, as Dota does. `display_mode` in
`~/.config/dbota/client.conf` (or the settings' VIDEO tab, or `--display-mode` for one run) is `windowed`,
`borderless` (windowed fullscreen, the default) or `fullscreen` (exclusive); it switches while
the game runs. On Wayland there is no exclusive fullscreen (the compositor owns the video mode),
so `fullscreen` shows as `borderless`. Captures (`--screenshot-at`, `--bench`) want
`--display-mode windowed` for a window of the `--window` size.

While a match is on screen and the window has focus the cursor is confined to the window, so the
screen edges pan the camera on every side. It is let go in the menus, with the settings open
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
| `visual_clarity` | sets units apart from the ground: the scenery a little duller and darker, shade around the forest, the units a little more vivid, a soft dark disc under every unit and a ring at every hero's feet; `off` (as Dota: no outline, the side's ring under what is selected, a closing ring on what the cursor comes onto), `low` or `high` | `off` |
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
fogged ground at most 50% of its own unfogged luminance and 45% of its saturation, seen ground untouched, and a 1–5 cell border (each cell is measured drawn with the fog and without it, so any tick works; `VISUAL_TICK`, `VISUAL_RUNS` and `VISUAL_DIR` set the tick, the repeats of the hero-model check (cold and warm, the animated portrait on and off) and the output folder), seen from both seats: every hero model loads whole, each part on the hero's own skeleton and no scene replaced once drawn, and is drawn with detail, buildings stand as the map places them, enemy buildings are seen through the fog and the enemy base is fogged, no range rings without `Alt`, only the menu's
buttons in the top-left block, every hero's health bar in its side's colour, round buff icons. It needs the asset cache and a display, and exits non-zero when a check fails.

`tools/bench.sh REPLAY.brp` measures the binary's size, cold and warm start, and the frame times,
CPU, RAM and VRAM of a busy stretch of the replay and of a live match against a local
`bota_server` (settings at the top of the script). It rests on `--bench SECONDS`, which times
that many seconds of frames once the scene is whole and prints them on one `bench:` line.

## Reading Dota's files

heatel (a crate of its own, checked out beside dbota) reads Valve's formats (VPK packages, compiled
Source 2 resources, KV3, textures, models and animations, the world, sounds, KeyValues texts)
straight from a Dota 2 install, on the desktop and in a browser. `crates/dbota_import` makes the
whole asset cache out of them from the manifest (`dbota import`; the browser page runs the same
code in a worker as the crate's wasm module). Its golden tests compare what it makes with a
reference cache exported by Source2Viewer 20 (manifest version 6, `~/.cache/dbota/v6`); they need
the install and that cache, so they run only when asked:

```sh
cargo test --release -p dbota_import -- --ignored   # DBOTA_DOTA_PATH, DBOTA_REFERENCE: where they are
```

## License

MIT, see [LICENSE](LICENSE). The interface fonts in `assets/fonts` are subsets of Liberation Sans
(Latin, Greek, Cyrillic) and Noto Sans CJK (Han, kana, Hangul), under the SIL Open Font License
(`OFL.txt`, `OFL-NotoSansCJK.txt`); `tools/subset_fonts.py` makes them. Other scripts come from the
system's fonts on the desktop; a browser has the shipped ones only.
