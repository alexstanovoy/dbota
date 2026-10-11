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
   (cd ../bota && cargo build --release -p bota-server -p bota-bot)
   ```

## Play

Start a server, a bot and dbota (three terminals, or background the first two):

```sh
../bota/target/release/bota-server --map 2 --players 2
../bota/target/release/bota-bot
target/release/dbota --name you
```

On the hero screen click a portrait, then click Ready. The game starts once both seats are ready.

To play against the bots of [drysua](https://github.com/alexstanovoy/drysua), clone it next to bota
and dbota and run its launcher, which builds everything, starts the server and the bot, and opens
dbota:

```sh
python3 drysua/scripts/play_match.py --opponent brain
```

`--help` lists the opponents and options (side, seed, immortal towers or heroes, watching instead
of playing).

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
| `F2` / `F3` | select courier / courier delivers |
| `F1` | select your hero |
| `Space` | centre the camera on your hero |
| `Y` | lock the camera to your hero |
| Arrows, screen edges | pan the camera; mouse wheel zooms |
| hold `Alt` | show attack and ability ranges |
| `M` | mute |
| `F10` | settings (keys can be rebound; saved to `~/.config/dbota/client.conf`) |
| `Esc` | cancel / close / leave |

Talents: the small button left of the abilities opens the talent tree; when you have a point,
click a talent to learn it. Buffs and debuffs of the selected unit sit above the abilities.

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
