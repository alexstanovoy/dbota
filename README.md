# dbota

A Dota-looking client for [bota](https://github.com/alexstanovoy/bota), a simplified, deterministic
Dota 2 for bots and humans. With dbota a person can play a bota match, watch one live, or replay a
recorded one. It draws with Dota 2's own models, textures, icons and sounds, taken from a local
Dota 2 install. Bots don't need it.

## Setup

1. Clone dbota and bota side by side (dbota builds against `../bota`):

   ```sh
   git clone https://github.com/alexstanovoy/bota
   git clone https://github.com/alexstanovoy/dbota
   ```

2. Install Dota 2 through Steam (the regular or the Flatpak install is found automatically), then
   export the assets dbota uses into `~/.cache/dbota`:

   ```sh
   cd dbota
   tools/import_assets.sh            # or: tools/import_assets.sh --dota-path "<...>/dota 2 beta"
   ```

   Valve's files stay in that cache for personal use; they are never part of this repository.

3. Build:

   ```sh
   cargo build --release             # binary: target/release/dbota
   (cd ../bota && cargo build --release -p bota-server)
   ```

## Play

Against a bot: the bots live in [drysua](https://github.com/alexstanovoy/drysua). Clone it next to
bota and dbota and run its launcher, which builds everything, starts the server and the bot, and
opens dbota:

```sh
python3 drysua/scripts/play_match.py --opponent brain
```

`--help` lists the opponents and options (side, seed, immortal towers or heroes, watching instead
of playing).

Two people: start a server and connect two dbota clients (from another machine use
`--addr <host>:4455`):

```sh
../bota/target/release/bota-server --map 2 --players 2
target/release/dbota --name alice
target/release/dbota --name bob
```

Alone, for practice: `bota-server --map 2 --players 2 --dummy-seat` gives the other seat to a hero
that never leaves its fountain.

On the hero screen click a portrait, then click Ready. The game starts once both seats are ready.

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
| `B` | shop (right click an item: buy; drag an item onto the shop: sell) |
| `F5` | quick-buy |
| `F2` / `F3` | select courier (its bag shows in the panel) / courier delivers |
| `F1` | select your hero |
| `Space` | centre the camera on your hero |
| `Y` | lock the camera to your hero |
| Arrows, screen edges | pan the camera (all four edges, the HUD bars included); mouse wheel zooms |
| hold `Alt` | show attack, tower and ability ranges (they are never drawn otherwise) |
| `M` | mute |
| `F10` | settings: volume, damage numbers, your hero's voice answers, edge pan; keys can be rebound (saved to `~/.config/dbota/client.conf`) |
| `Esc` | cancel / close / leave |

Talents: the small button left of the abilities opens the talent tree; when you have a point,
click a talent to learn it. Buffs and debuffs of the selected unit sit above the abilities. The
stash shows at the home shop.

## Watch a live match

```sh
target/release/dbota --spectate --addr 127.0.0.1:4455
```

`Tab` switches whose vision you see.

## Replays

The server records a match with `bota-server --replay FILE`.

```sh
target/release/dbota --replay match.brp              # whole map
target/release/dbota --replay match.brp --pov 0      # as seat 0 sees it (its fog, panel, shop)
target/release/dbota --replay match.brp --follow 1   # camera stays on seat 1's hero
```

`Space` pause, `=` / `-` faster / slower, `.` one step, `Tab` switch vision.
A replay opens only with the bota version that recorded it.

`dbota --help` lists everything else, including the debug options for screenshots.

## Checking the picture

`tools/visual_check.sh` records a short match of two bota bots and measures dbota's picture of it:
fogged ground at most 45% of seen ground's luminance and saturation with a 1–5 cell border, heroes
drawn with their models, buildings that never turn, no range rings without `Alt`, no gold or souls
in the top-left block, round buff icons. It needs bota next to dbota, the asset cache and a display,
and exits non-zero when a check fails (`cargo test --release -- --ignored` runs it too).
`DBOTA_FRAMES=1` logs frame times (median, 95th and 99th percentiles) every ten seconds.
