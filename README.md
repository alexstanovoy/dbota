# dbota

A Dota-looking client for bota, a simplified, deterministic
Dota 2 for bots and humans. With dbota a person can play a bota match, watch one live, or replay a
recorded one. It draws with Dota 2's own models, textures, icons and sounds, taken from a local
Dota 2 install. Bots don't need it.

## Setup

1. Put the bota and dbota repositories side by side; dbota builds against `../bota`.

2. Build:

   ```sh
   tools/build.sh                    # target/release/dbota and, beside it, target/release/bota_server
   PROFILE=dist tools/build.sh       # smaller, link-time optimised, in target/dist (~6 min)
   ```

   A live match needs a client and a server built from the same bota: a server that sees another wire
   fingerprint refuses the client. `tools/build.sh` builds the server of `../bota` beside the client, which
   is where LOCAL LOBBY and DEMO HERO look for it first (next the settings' or `$BOTA_SERVER`'s path, then
   the `PATH`); a server of another wire than the client's is skipped, and with none usable the menu says
   which it looked for. A replay opens only with the bota that recorded it. Ship the two files together.

3. Install Dota 2 through Steam (the regular or the Flatpak install is found automatically), then
   make the asset cache from it, one command, about a minute:

   ```sh
   target/release/dbota import       # or: dbota import --dota-path "<...>/dota 2 beta"
   ```

   It converts the models, map, textures, sounds and texts dbota uses into `~/.cache/dbota/v<N>`
   (`N` is `assets-manifest/version`; each version keeps a folder of its own, so older and newer
   clients share a machine). Valve's files stay in that cache for personal use; they are never part
   of this repository. Without it dbota draws stand-ins. Without Dota 2, take a cache someone with it
   prepared (`dbota import-cache <folder|zip>`, made by their `dbota export-cache`): see `tools/README.md`; a
   `~/.cache/dbota/tools` folder left by the old export script can be deleted.

   The cache is stamped with the asset fingerprint of the dbota that made it (the cache version,
   the importer's revision and the manifest's hash). A dbota of another fingerprint does not play on
   it: it opens at a screen saying the assets are out of date for this version, with a button that
   imports them again (a game named on the command line waits for that; captures and checks are
   only warned). The settings show, small in a corner, the client's version (its commit) and the
   asset fingerprint it needs.

## Play

`target/release/dbota` with no game on its command line opens the main menu, laid out as Dota's:

- **Sign in / create account** on a matchmaking service (`--service wss://host:port`, remembered; the
  service and `wss://` games are trusted by the public web's roots, or `--trust-cert FILE.pem` for one's
  own). The login is kept for the next run in `~/.config/dbota/session`, readable by its owner only; LOG OUT
  forgets it. There is no password recovery. The forms work offline: nothing connects while one is
  typed, SIGN IN and CREATE ACCOUNT open the connection when pressed (a refusal or an unreachable
  service is said on the form, with what was typed kept), and the connection is held only while
  something live is on it (a search, a proposal, a lobby, a match under way) and let go after ten
  seconds without; the next request opens it again with the kept login. Tab and Shift+Tab walk a form's
  fields and buttons, and Enter presses the focused button.
- **PLAY**: find a match per ruleset. The search box shows the time searching, the place in the queue and
  when the game servers are full; MATCH FOUND asks to accept or decline within the time shown (declining
  keeps the account out of the queues for a while); then the game opens by itself.
- **MY GAMES**: rejoin or watch the account's running matches (an account may run several at once), cancel
  searches. **LOBBIES**: create one, join one by name and password, or from the list. A lobby is made for a game mode chosen among the ones the service offers. In it, the slots are laid out by team: click a free slot to sit there (or LOOK ON), and the owner moves (MOVE, then a slot) or removes (KICK) the others and changes the game mode, who may sit and the password, which the rest read as plain text. Then a result screen.
What needs no service needs no account either: LOCAL LOBBY, REPLAYS and SETTINGS work
signed out and with no service reachable. There, you play under the profile name of the settings
(OPTIONS, PROFILE), as Dota's local games show your Steam name.

- **LOCAL LOBBY**: below its own lobby, a game server's address to join, `host:port` or `tcp://` (TCP),
  `ws://` or `wss://`, with no service at all (PLAY, or WATCH). Plain TCP to another machine is
  unprotected, and dbota says so once per host. Above it, a game hosted on this machine, as Dota's local lobby: dbota starts `bota_server`
  (beside `dbota`, else `$BOTA_SERVER`, else on the `PATH`) on a port, for this machine
  alone or (CREATE FOR THE LAN) for the local network too, in plain text, and seats you. The lobby has ten
  seats, five a side, whatever its **Rules**: FULL RULES (the whole map, the default) or SOLO MID change the
  rules only. The others join by this machine's address under LOCAL LOBBY, which the hero screen and the
  LOCAL LOBBY page show while a seat is open. The match starts once every player seated is ready, with
  exactly those players: nothing fills the empty seats, so one player is one hero. The server ends when you
  quit dbota.
- **DEMO HERO**: Dota's Demo Hero: the hero chosen alone on our map, with every cheat and Dota's demo
  panel at the side (spawn an allied or enemy hero, creep waves, neutrals, levels, gold, items, free
  spells, invulnerability, respawn, teleport, reset, the enemies' level, creep spawning, night, all
  vision, refresh). Its words come from the install's Demo Hero (`dota_addons/hero_demo`). Quit ends
  its server.
- **REPLAYS**: the replays this client keeps, by default, the newest first, each with its date, mode,
  players and how it ended; WATCH opens one in the replay viewer, DELETE removes it. A match played through
  the matchmaking service has its replay fetched from the service when the match ends (by whoever sat in it;
  a lost or left match too; a service that keeps none, `[artifacts] save_replays` off in radovid, is told on
  the page and nothing is saved from it); a failed fetch is tried again in the background, and at the next
  sign-in. A local lobby's and Demo Hero's replays are the server's own file, kept when the game is left (whole when the match ended or was left for good with LEAVE GAME; a game that is only disconnected from keeps what the server had written to disk so far). Where:
  `$XDG_DATA_HOME/dbota/replays` (`~/.local/share/dbota/replays`) on Linux, `~/Library/Application Support/dbota/replays`
  on macOS, `%LOCALAPPDATA%\dbota\replays` on Windows, named `YYYYMMDD-HHMMSS-mode-matchid.brp` (UTC) with a small
  `index.txt` beside them; in a browser, the page's private storage (OPFS, else IndexedDB), where each row also has
  DOWNLOAD .BRP and the page has OPEN A .BRP FILE, so a replay moves between the browser and a desktop.
  SETTINGS, Options, REPLAYS turns saving off and caps the count and the bytes (50 and 1 GB on a desktop, 25 and
  256 MB in a browser, by default); the oldest go when a new one is saved. A file that cannot be watched says why
  (damaged, or recorded by another version), and one with no match in it (under 1 KB: a game that never started) is
  not kept. On a desktop any replay file also opens by its path.

Cheats are Dota's, typed into the chat (`Enter`, then `-gold 5000`) or the console (`\`, also in
their console forms: `dota_create_item item_blink`): `-gold N`, `-lvlup N`, `-levelbots N`,
`-wtf`/`-unwtf`, `-refresh`, `-respawn`, `-item NAME`, `-createhero NAME [enemy]` (at the cursor),
`-spawncreeps`, `-disablecreepspawn`/`-enablecreepspawn`, `-spawnneutrals`, `-killcreeps [side]`,
`-startgame`, `-allvision`/`-normalvision`, `-teleport` (to the cursor). Only a match its host
started with cheats honours them: LOCAL LOBBY's "Enable cheats", DEMO HERO, or
`bota_server --cheats`. Matchmaking's
matches never cheat.

The power button at the top right quits, after Dota's question ("Confirm Quit"); `Esc` on the menu
backs out of a page to the top one, and there asks the same. `Esc` in a game (with nothing to cancel or
close) opens the dashboard over it, as Dota's: RETURN TO GAME (also the arrow at the top left, or `Esc`
again), DISCONNECT, and the power button. DISCONNECT leaves a seat at once and keeps it: RECONNECT takes
the same seat again (a direct or local game claims its seat with a key of its own); a game watched or a
replay is left after asking. `Esc` in a match that is over goes back to the menu. `--addr`, `--spectate`,
a seat key or `--replay` open that game first, with the menu under it.

### Trying the matchmaking menu on one machine

Bring up a service of your own as described in radovid's README ("A development setup on one machine":
`radovid dev-cert`, a `local` launcher for `bota_server`), then:

```sh
target/release/dbota --service wss://localhost:4460 --trust-cert dev/cert.pem
```

CREATE ACCOUNT, then PLAY / FIND MATCH needs a second account in the same ruleset (a bot client of
the service, or a second dbota with `--config` and `XDG_DATA_HOME` of its own so the two keep
separate logins). In a LOBBY the owner's READY stays dimmed ("Waiting for more players") until
the lobby is full.

`--script "STEP;STEP;..."` drives the window like a person, for end-to-end tests (see
`tests/menu_quit.rs`): `key:NAME`, `text:TEXT` (into the focused field; `key:Tab` for the next),
`mouse:X,Y` (shares of the window), `lclick`, `rclick`, `down:KEY` / `up:KEY`, `click:BUTTON` (the label in lower case; in a game `pick ready`, `pick <hero>`; a dimmed button
ends the run with exit code 2), `await:menu|game|popup|nopopup|button:NAME` (`popup` covers the
match-found offer), `shot:FILE`, `wait:SECONDS`. A step that waits more than two minutes ends the run
with exit code 2.

Two people without a service: start a server and connect two dbota clients (from another machine use
`--addr <host>:4455`, or LOCAL LOBBY's address in the menu):

```sh
target/release/bota_server --map 2      # listens on 127.0.0.1:4455
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
| Right click | move, or attack what is under the cursor; on an ally, what Dota's "Right-Click Allies" says (`right_click_allies`, OPTIONS, CONTROLS): `follow` (the default, as in Dota) follows it; `attack` attacks it when the game lets it be denied (a lane creep or ward below half its health, a tower below a tenth) and else does nothing but tell why; `deny` attacks it when it can be denied and else follows it. A building is only attacked with `A` |
| Left click | select; confirms a targeted ability or item |
| `A` / `M` then left click | attack-move / move there, whatever is under the cursor |
| `S` / `H` | stop / hold position |
| Directional Move (unbound, as in Dota) | held, the hero walks the straight line towards the cursor with no pathfinding (the ground and the trees stop it where they are), the direction following the cursor; letting go stops it |
| `Q` `W` `E` `D` `F`, `R` | the ability slots (Shadowraze near, mid, far, Feast of Souls), the ultimate |
| `O`, then an ability key; or `Ctrl` + an ability key | learn that ability (spend a skill point) |
| `U` | open the talent tree (the Attribute Bonus is learned there too) |
| `Z` `X` `C` `V` `B` `N` | use the item in that inventory slot |
| `T` | town portal scroll: take it up and click a building to teleport near it; `T` again goes to the fountain. Drag a scroll stack from the stash onto the round slot beside the inventory. Every teleport under way shows on the map (Dota's own effect, in the seat's colour) and the minimap, for both sides |
| `F4` | shop, laid out as Dota's: the BASICS, UPGRADES and NEUTRALS tabs with Dota's categories (from the asset cache; bota's own groups without it), a search by name or Dota's aliases (`tp`, `bfly`). as Dota's mouse: left click an item to choose it and see its build tree (its parts and recipe below it, what it builds into above), right click (or a double click) to buy it, also in the tree; shift-click sets (or clears) the quick-buy item, whose missing parts the quick-buy row in the corner offers; drop an item on the panel's foot to sell it; in the search line `Enter` buys the first answer. Items bota does not have are left out |
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
| Arrows, screen edges, middle button held | pan the camera (all four edges, the HUD bars included; the middle button drags the map); mouse wheel zooms (the camera's settings below) |
| hold `Alt` | show attack, tower and ability ranges and the neutral camps' spawn boxes (they are never drawn otherwise), and the heroes' own icons on the minimap |
| `Esc` | cancel / close; with nothing open, the dashboard (DISCONNECT, quit, SETTINGS; also the up arrow at the top left, Dota's dashboard button); leaves a match that is over; on the menu, back, and on its top page quit |
| `F9`, `=` / `-`, `.`, `Tab` | in a replay: pause, faster / slower, one tick, switch vision (both teams, Radiant, Dire; `Tab` as a spectator too) |

The settings screen (SETTINGS in the main menu and on the dashboard over a game) has Dota's tabs: HOTKEYS,
OPTIONS (profile name, interface size, damage numbers, animated portrait, the scoreboard's items, Dota's "Require Hold/Cancel to Stop" options for teleports and for channelled abilities (off, as in Dota: an order given meanwhile waits for the channel to end, and only a stop or a hold breaks it; the last order given is the one carried out), Right-Click Allies, edge pan), VIDEO (display mode, frame
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
bars are the side's colour whoever watches: Radiant green, Dire red. Over every unit the bar
marks its health as Dota's does, a small mark every 250 and a big one every 1000; a courier's or a
ward's health, counted in hits, stands as one block a hit. Each seat has Dota's player colour
(read from the install: Radiant's blue, teal, purple, yellow, orange; Dire's pink, olive, light blue,
dark green, brown): the strip over its portrait in the top bar, its row of the scoreboard, its pings and
teleports, and its hero's mark on the minimap, ringed in its side's colour.

The map's objectives stand where the match's rules stand them (the Dota map's default: Watchers,
Outposts and Shrines of Wisdom; the mid-only map none), as Dota's models showing their state: a
Watcher ready, lit (by day or by night) or put out, an Outpost a side holds, a Shrine of Wisdom
ready to be gathered. They have no health bar, and the minimap shows each by Dota's icon (an Outpost
in its holder's colour, a Watcher or a Shrine dimmed while it cannot be used). A hero channelling at
a Watcher or an Outpost plays Dota's generic channel. An Outpost captured and a Shrine gathered
are told in the kill feed in Dota's words with its icons, and a Shrine gathered in the chat log too.

Runes appear where and when the match's rules spawn them (the Dota map's default: all of them; the mid-only
map none unless its server is started with `--runes`). Each one the side sees is Dota's model turning at its
spot in its glow, a Bounty Rune appearing in its swirl, and its icon on the minimap, which keeps a rune the fog
took until its spot is seen without it; a water or power rune appearing at the river shows Dota's toast (top or
bottom). A right click on a rune walks the hero to it and takes it, `A` and a click on it destroys it. A rune
taken sounds as in Dota, with the hero's own line for it, the chat and the kill feed tell it in Dota's words,
and its buff shows on the hero (its loop) and on the buff bar; under an Invisibility Rune the hero is drawn
see-through for its own side. The Bottle is drawn by its charges and by the rune it keeps, with the seconds
before that rune goes off at the slot's left and the rune named in its tooltip; drinking from it pours over
the hero, and a rune put into it corks it. `dota_spawn_rune <rune>` (a match with cheats) puts a rune at the
spot nearest the cursor.

Between the picks and the horn comes Dota's strategy screen: every seat's hero under its player's colour
and the clock over STRATEGY TIME; at the left the hero you enter battle as, its name and abilities; at
the right STARTING ITEMS: the gold that remains, the inventory as your choices fill it (what stacks in
Dota stacks), and the shop's basic items, bought with a right click (or a double click) when the match
begins; a right click on an inventory item takes it back. Its words are the install's.

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
the cursor stays free; the cursor is never locked instead.

In a browser the canvas goes fullscreen instead (VIDEO's windowed fullscreen, or F11), and the
cursor is kept in the game at the same times by locking the pointer (a page cannot confine it):
the game then draws its own cursor, moved by the mouse and held inside the canvas, and every
click, drag, hover and the edge pan go where it is. A browser locks only on a gesture: the
pointer is taken by the click or key that brings a match on screen or closes the settings, by a
press on the canvas, and by F11; Esc lets it go (the browser's own key), and the next press on
the canvas takes it again. Until it does (after an Esc, or when the browser refused the lock, which
the console says with `pointer lock refused`), the browser's own cursor is free and can leave the
page: a page cannot keep it in without the lock. Only a move moves the game's cursor; a press or
a release happens where the cursor is.

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
| `animate_portrait` | Dota's "Animate Portrait": the bottom panel's portrait plays the unit's idle animation, drawn up to 30 times a second; a hero's face plays its idle expression and moves with the line your hero says (its phonemes and the facial curves Dota bakes into the voice line); off, it shows Dota's picture of a hero, or a render of another unit's model made once | `true` |
| `camera_pitch`, `camera_fov` | the camera's angle below the horizon and its lens (vertical field of view), in degrees | `58`, `25` |
| `camera_distance`, `camera_distance_min`, `camera_distance_max` | how far out the camera starts, and how near and far the mouse wheel takes it, in world units | `4400`, `1600`, `6500` |
| `camera_pan_speed` | world units a second the camera pans at its starting distance (Dota's `dota_camera_speed`); nearer it pans slower, farther faster | `3031` |

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
buttons in the top-left block, every hero's health bar in its side's colour, round buff icons; and for each hero, that its portrait's face says a line (`--talk-check HERO`: with the body and idle expression held still, the face along the line differs from the face at rest and from one frame to the next, while the rest face drawn twice is the same picture; the frames idle, talking and checked are saved too; then each of its move and attack lines, held along, must move the face's morph weights); and, in a game on a local server, that each hero answers a move and an attack order with its line and its portrait's jaw follows the line's phonemes, back at rest once it has ended (`tests/portrait_talk.rs`). It needs the asset cache, and exits non-zero when a check fails.

`tools/bench.sh REPLAY.brp` measures the binary's size, cold and warm start, and the frame times,
CPU, RAM and VRAM of a busy stretch of the replay and of a live match against a local
`bota_server` (settings at the top of the script). It rests on `--bench SECONDS`, which times
that many seconds of frames once the scene is whole and prints them on one `bench:` line.

`tools/gate.sh [REPLAY.brp]` is the merge gate: fmt, clippy, both browser clients' checks, an asset cache of this build (imported from
your Dota 2 install once per asset stamp into `~/.cache/dbota-gate/<stamp>`, or `DBOTA_GATE_ASSETS`/`DBOTA_ASSETS`; the tests that start
the client read it), the tests, the release build and `cargo deny check`, then the importer's ignored tests (when the reference cache they
compare with is there) and `tools/visual_check.sh` on the given replay (default: one the bota example `local_match` records). It stops
at the first failure and prints `GATE-OK`; `GATE_REQUIRE_INSTALL=1` makes a missing install a failure, `GATE_SKIP="deny"` leaves steps out
while you iterate, `GATE_LEAN=1` runs only fmt, clippy, one browser client check, the asset cache and the tests, and `GATE_FPS_CMD` and
`GATE_FPS_BASE` add a frame-time comparison (`$GATE_FPS_CMD $GATE_FPS_BASE HEAD`) as the last step.

### Without windows on the desktop

`tools/headless.sh COMMAND...` runs a command in a private headless Wayland compositor (Weston's
headless backend with its GL renderer; the `weston` package) started for it and gone with it: the
client draws on the GPU as on a desktop and its screenshots are read back from its own frames, but no
window shows. `tools/visual_check.sh`, `tools/bench.sh` and the tests that draw (`menu_quit`,
`refused_replays`) run in one by default; for a capture of your own, put it in front:

```sh
tools/headless.sh target/release/dbota --replay match.brp --screenshot-at 4000 --display-mode windowed
```

`DBOTA_SHOW=1` shows the windows on the desktop instead (debugging a script by eye);
`DBOTA_HEADLESS_SIZE=WxH` and `DBOTA_HEADLESS_SCALE=N` set the virtual output (default 1920x1080 at
scale 1), so a borderless or fullscreen capture is that size, and a windowed one is the `--window`
size at that scale. Measured checks judge the same scene equally on a desktop and headless (the
numbers move a little with the size and scale of the picture). Frame times are the client's own
and steadier headless, where no desktop competes for the GPU (a busy replay stretch at 2400x1350:
3.5 to 3.9 ms a frame headless over runs, 2.9 to 7.8 ms in a window on a desktop in use): compare
benches made the same way. Running the game normally is untouched: only commands put behind the
script draw headless.

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
