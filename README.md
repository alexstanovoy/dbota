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
   (cd ../bota && cargo build --release -p bota_server)
   ```

## Play

A bot is any program that speaks bota's protocol: it takes a seat as a player does.

Two people: start a server and connect two dbota clients (from another machine use
`--addr <host>:4455`):

```sh
../bota/target/release/bota_server --map 2 --players 2
target/release/dbota --name alice
target/release/dbota --name bob
```

Alone, for practice: `bota_server --map 2 --players 2 --dummy-seat` gives the other seat to a hero
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
| `T` | town portal scroll: take it up and click a building to teleport near it; `T` again goes to the fountain. Drag a scroll stack from the stash onto the small slot beside the inventory. Every teleport under way is ringed on the map and the minimap, for both sides |
| `B` | shop (right click an item: buy; drag an item onto the shop: sell) |
| `F5` | quick-buy |
| `F2` / `F3` | select courier (its bag shows in the panel) / courier delivers |
| `F1` | select your hero |
| `Space` | centre the camera on your hero |
| `Y` | lock the camera to your hero |
| Arrows, screen edges | pan the camera (all four edges, the HUD bars included); mouse wheel zooms |
| hold `Alt` | show attack, tower and ability ranges (they are never drawn otherwise) |
| `M` | mute |
| `F10` | settings: volume, damage numbers, your hero's voice answers, edge pan, picture quality; keys can be rebound (saved to `~/.config/dbota/client.conf`) |
| `Esc` | cancel / close / leave |

Talents: the small button left of the abilities opens the talent tree; when you have a point,
click a talent to learn it. Buffs and debuffs of the selected unit sit above the abilities. The
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

`Space` pause, `=` / `-` faster / slower, `.` one step, `Tab` switch vision.
A replay opens only with the bota version that recorded it.

`dbota --help` lists everything else, including the debug options for screenshots.

## Checking the picture

`tools/visual_check.sh REPLAY.brp` measures dbota's picture of a recorded Map 2 match:
fogged ground at most 45% of seen ground's luminance and saturation with a 1–5 cell border, heroes
drawn with their models, buildings that never turn, no range rings without `Alt`, no gold or souls
in the top-left block, round buff icons. It needs the asset cache and a display, and exits non-zero when a check fails
(`VISUAL_REPLAY=REPLAY.brp cargo test --release -- --ignored` runs it too).
`DBOTA_FRAMES=1` logs frame times (median, 95th and 99th percentiles) every ten seconds.

`tools/bench.sh REPLAY.brp` measures the binary's size, cold and warm start, and the frame times,
CPU, RAM and VRAM of a busy stretch of the replay and of a live match against a local
`bota_server` (settings at the top of the script). It rests on `--bench SECONDS`, which times
that many seconds of frames once the scene is whole and prints them on one `bench:` line.

## License

MIT, see [LICENSE](LICENSE).
