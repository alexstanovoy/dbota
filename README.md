# dbota

A Dota-looking client for bota: play against a bot, watch a bot game live, or replay a match.
It draws with Dota 2's own models, textures, icons and sounds, taken from your local Dota 2
install. Bots never need it; it is for people.

## Play against the brain (the usual way)

From the workspace root (`~/Workspace/bots`):

```sh
./play.sh --opponent brain
```

That builds bota, drysua and dbota, starts a server and the bot, and opens dbota. The first run
builds for a few minutes. In the lobby press `2` for Shadow Fiend (`1` is Pudge), then `R` when
ready. The game starts once both sides are ready.

Useful options:

| Option | What it does |
|---|---|
| `--opponent teacher` / `harass-push` / `farmer` / `dummy` | another bot (`dummy` stands in its fountain) |
| `--human-side dire` | play Dire |
| `--seed N` | another match seed |
| `--immortal-towers`, `--immortal-heroes` | practice without the game ending |
| `--watch brain` | watch the brain play instead of you (spectator) |
| `--no-build` | skip rebuilding |

The match is recorded to `drysua/artifacts/temp/play-*/match.brp`.

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

## Watch (spectate)

Live, with bots playing each other:

```sh
./play.sh --watch brain --opponent teacher
```

Or connect to any running bota server yourself:

```sh
dbota --spectate --addr 127.0.0.1:4455
```

`Tab` switches whose vision you see.

## Replays

```sh
dbota --replay path/to/match.brp            # whole map
dbota --replay match.brp --pov 0            # as seat 0 sees it (its fog, panel, shop)
dbota --replay match.brp --follow 1         # camera stays on seat 1's hero
```

`Space` pause, `=` / `-` faster / slower, `.` one step, `Tab` switch vision.
Replays only open with the bota version that recorded them.

## Dota assets (once)

dbota needs Dota 2 installed (Steam, Flatpak path is found automatically). Export the assets it
uses into `~/.cache/dbota`:

```sh
tools/import_assets.sh            # or: tools/import_assets.sh --dota-path "<...>/dota 2 beta"
```

Valve's files stay in that cache for personal use; they are never committed.

## Build by hand

```sh
cargo build --release             # binary: target/release/dbota
```

It builds against `../bota` (bota-client-core and bota-proto), so keep the two checkouts side by side.
`dbota --help` lists the rest, including the debug options for screenshots.
