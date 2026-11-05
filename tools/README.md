# Dota 2 asset import

`dbota import` converts the Dota 2 models, map, textures, sounds and texts that dbota draws into a local cache, from
your own Dota 2 install. **Valve's files are for personal use only**: they live in the cache, outside any repository,
and are never committed or published. Only our code and the manifest are in git. Nothing is downloaded: the converter
(`crates/dbota_import`, over the heatel crate) is part of dbota, and the browser runs the same code on the folder the
player picks.

```sh
target/release/dbota import                       # Flatpak Steam, then ~/.steam/steam, then ~/.local/share/Steam
target/release/dbota import --dota-path "/path/to/steamapps/common/dota 2 beta"
DBOTA_ASSETS=/elsewhere target/release/dbota import   # cache location (default: `dbota cache-dir`)
```

An import reads ~1 GB of the install, writes ~810 MB and takes about a minute. It builds beside the cache and swaps in
when whole. `index.txt` lists the files; `stamp.txt` names the manifest version, importer revision and the size and
change time of `pak01_dir.vpk`. Running it again does nothing while the stamp matches (`--force` imports anyway). A
build whose plan (what the manifest asks for; comments and layout don't count), importer or converters differ has
another fingerprint and imports into a new `~/.cache/dbota/<fingerprint>`. `dbota import --verify` checks a cache
against the manifest. An asset that doesn't convert is reported, and dbota draws nothing in its place.

## A cache for someone without Dota 2

```sh
target/release/dbota export-cache cache.zip       # with Dota 2: the cache as a zip (~400 MB)
target/release/dbota import-cache cache.zip       # elsewhere: the zip, or the cache folder itself
```

`import-cache` (also the menu's USE A PREPARED CACHE and the browser page's prepared cache) treats the cache as
untrusted and checks all of it before installing anything: its stamp must be this build's asset fingerprint (both
sides run the same build), `index.txt` must list exactly the files present with their sizes, and every path must be
plain, in one of the cache's folders, with an extension and size the importer makes. A refused cache leaves the old
one untouched. The zip holds Valve's converted assets: hand it only to people you may.

## Files

| File | Role |
| --- | --- |
| `../assets-manifest/manifest.txt` | every imported asset: logical name, VPK source, cache path, clips, effect bindings; format in its header |
| `../assets-manifest/ids.txt` | bota item/ability/hero ids → Dota names, and whose tooltip texts to import |
| `inspect_gltf.py` | prints a model's meshes, skins and animations (`--json` for machines) |

## What the cache holds

Models (`models/`: heroes with their default wearables, lane and neutral creeps, towers, buildings with their
destruction models, trees, couriers, Roshan and the Tormentor, the map's objectives, items on the ground, runes,
wards, and Dota's portrait framings), the mid band of the map (`map/mid/`), terrain and particle textures, particle
systems as JSON (`particles/`), icons and HUD art, texts out of the install's localization and scripts (`text/`:
tooltips, the shop's categories, interface labels) and sounds (`sounds/`).

Formats: glTF 2.0 binary with KTX2 textures beside it, PNG, WAV, MP3. Model and map textures stay block-compressed
(BC1, BC3, BC5) with every mip level; interface and terrain layer textures are PNG. WAVs are PCM 44.1/22.05 kHz; voice
lines stay MP3 (Bevy's `mp3` feature plays them).

## Using the models

- **Coordinates.** glTF Y-up metres (Source inches × 0.0254): `gltf.x = dota.y`, `gltf.y = height`,
  `gltf.z = dota.x`. bota positions are Dota + 9216 on both axes. Models face +Z (Dota +x at yaw 0). `model_scale=` in
  the manifest is the unit's Dota `ModelScale`.
- **Clips.** `clip` lines map logical clips (`idle`, `run`, `attack`, `cast1..6`, `death`, ...) to glTF animations and
  durations; `point=` is Dota's attack or cast point at base speed. A clip lasts to its last frame, `(frames - 1) / fps`,
  as Dota plays it, holding an end pose. Root motion is baked into the root bones; the client holds roots in place.
- **Neutral creeps.** One `model` block per model and one `unit <npc_units.txt name> <model> model_scale=...` line per
  unit that wears it. `cast1..6` are Dota's `ACT_DOTA_CAST_ABILITY_1..6` (`src/ability_art.rs` keys each neutral ability
  to its clip). `unit <name> none` draws nothing. Models load when their first unit comes into view.
- **Wearables** are separate skinned meshes: spawn them with the hero and bind their joints to the hero's joints of
  the same name, **ignoring case**. `dbota import --verify` checks that every wearable joint exists.
- **Materials** are mapped by texture name, as Source2Viewer 20 does without Valve's compiled shaders: base colour and
  normals are right, metallic/roughness/occlusion are approximate. Every material keeps Valve's parameters in
  `extras.vmat`.
- **Buildings and siege creeps** die by swapping in their `*.destruction` model and playing `death` once.
- **Map objectives** (Watchers, Outposts, Shrines, Lotus Pools, a planted Banner) are left out of the map mesh and drawn
  only where the match stands them; their places stay in `buildings.txt`. Team-coloured material groups (an owned
  Outpost, a lit Watcher) are not written yet.

## The mid-lane map (`map.mid`)

`maps/dota.vpk`'s world is cut to the band `|dota.x − dota.y| / √2 ≤ 13100`, `−11800 ≤ (dota.x + dota.y) / √2 ≤ 11700`:
mid lane fountain to fountain, both base plateaus and the ground beside it. Towers, trees, barracks, ancients and
fountains are removed (the client spawns them as units; `NOT_IN_WORLD` in `crates/dbota_import/src/import.rs`). Nodes are
flattened to world matrices; a triangle list using under half of its mesh set's vertices gets its own vertices (a glTF
loader would otherwise copy the shared buffers per piece: 2.6 GB loaded for 70 MB of geometry).

- **Terrain** uses Valve's 4-layer `multiblend.vfx`: the glTF material carries layer 1, the blend weights are in
  `_TEXCOORD_1.._3`, `extras.vmat.TextureParams` names every layer texture (in `textures/ground/`), and the client
  bakes the layers into vertex colours on load.
- **Water** has no colour texture; its normal, noise and flow maps are in `textures/ground/`.
- **Trees.** `map/mid/trees.txt` lists every tree of the whole map: `model dota_x dota_y height yaw_deg scale`.

## Not imported

- Particle features the client does not simulate: dynamic lights, model renderers, curl-noise forces, ground
  collision, a spline ramp and a distance remap (`crates/dbota_import/tests/particles.rs`).
- Tower destruction models and event, crystal and mega creep variants.
- The map outside the mid band: raise `half_width` / `v_min` / `v_max` on the manifest's `world` line (the whole map
  with materials is ~330 MB).
