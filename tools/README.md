# Dota 2 asset import

`dbota import` converts the Dota 2 models, map, textures, sounds and texts that dbota draws into a
local cache, from your own Dota 2 install. **Valve's files are for personal use only**: they live in
the cache, outside any repository, and are never committed or published. Only our own code and the
manifest are in git. Nothing is downloaded: the converter (`crates/dbota_import`, over the heatel library) is part of dbota,
and the browser build runs the same code on the folder the player picks.

```sh
target/release/dbota import                       # Flatpak Steam, then ~/.steam/steam, then ~/.local/share/Steam
target/release/dbota import --dota-path "/path/to/steamapps/common/dota 2 beta"
DBOTA_ASSETS=/elsewhere target/release/dbota import   # cache location (default ~/.cache/dbota/v<N>)
```

An import reads ~1 GB of the install at most, writes ~550 MB (830 files) and takes about a minute.
It is made in a folder beside the cache and replaces it only once whole; `index.txt` lists the files
and `stamp.txt` says what made it (the manifest version, the importer's revision, the size and
change time of `pak01_dir.vpk`). Running it again does nothing while the stamp matches (`--force`
imports anyway); a Dota update or a new dbota changes the stamp. A new manifest version imports into
a new folder, `~/.cache/dbota/v<N>`: the client reads the folder of the version it was built with,
and older folders can be deleted. Caches up to version 6 were exported by a separate Source2Viewer
script that kept its own tool in `~/.cache/dbota/tools`; that folder is no longer used and can be
deleted. An asset that does not convert is reported and drawn as a stand-in.

## Files

| file | role |
| --- | --- |
| `../assets-manifest/manifest.txt` | every imported asset: logical name, VPK source, cache path, clips; format in its header |
| `../assets-manifest/version` | the cache version: the cache folder `v<N>` |
| `../assets-manifest/ids.txt` | bota item/ability/hero ids → Dota names, and whose tooltip texts to import |
| `visual_check.sh` | measures the picture of a recorded match (see the main README) |
| `bench.sh` | measures the binary's size, start and frame times |
| `verify_assets.py` | checks a cache: glTF meshes/skins/clips/textures, wearable joints, PNG sizes, WAV/MP3 format (`verify_assets.py ../assets-manifest/manifest.txt <cache>`) |
| `inspect_gltf.py` | prints a model's meshes, skins and animations (`--json` for machines) |

## What the cache holds (~680 MB per `v<N>` folder)

| path | content | size |
| --- | --- | --- |
| `models/hero/nevermore/` | Shadow Fiend, 17 clips, plus `nevermore_{head,shoulders,arms}/` default wearables | 16 MB |
| `models/hero/pudge/` | Pudge, 18 clips, plus `pudge_{weapon,back,offhand,shoulder,arms,head,belt}/` default wearables | 4 MB |
| `models/creep/` | Radiant/Dire melee, ranged, flagbearer (6 clips each); siege (3 clips) and its `*_destruction` break-apart model | 41 MB |
| `models/neutral/` | the 44 models of the 47 neutral creep kinds (golems and some others share one), each with idle, run, attack, death and, where the model has them, a second attack, stun and a cast pose | 126 MB |
| `models/tower/` | Radiant (`radiant_tower002` + base) and Dire (`dire_tower002`) tier-1 towers | 4 MB |
| `models/tree/` | the 9 tree models the map uses around mid (standing mesh + stump) | 8 MB |
| `models/building/` | ancients, melee/ranged barracks (static, plus `*_destruction` collapse models), fountains | 73 MB |
| `models/courier/` | default donkey courier, Radiant and Dire, walking and flying | 4 MB |
| `models/portraits.txt`, `models/portrait_backgrounds/` | Dota's portrait framing of each model above (camera, light, ambient, the background's corner tints) and the background images it names | 1 MB |
| `map/mid/` | `mid.gltf` with its geometry in `mid.bin`, the default map from fountain to fountain around mid, its textures, `trees.txt`, `buildings.txt` (the towers' places) | 354 MB |
| `textures/ground/` | terrain layer textures (also used by `map.mid`) | 37 MB |
| `textures/particle/` | 16 effect textures (smoke, glow, sparks, ground cracks, fire, the Glyph's rings, ...) | 1 MB |
| `icons/`, `hud/`, `minimap/` | item and ability icons, hero portraits, minimap hero/creep/tower/building icons, HUD and shop art, minimap | 2 MB |
| `text/tooltips.json` | the names, descriptions, lore, notes and number labels of the items and abilities in `ids.txt`, read out of the install's own localization and scripts; the client puts the simulator's numbers into them (without the file, tooltips show only the simulator's plain names and numbers), and the aliases the shop's search finds items by | 30 KB |
| `text/shop.json` | the shop's categories and the items each lists, in Dota's order (`scripts/shops.txt`), and the shop's words (category, tab and button names) from the install's localization; without it the shop shows bota's own groups and plain words | 16 KB |
| `text/labels.json` | the interface's labels the manifest's `label` lines name (the stats tooltip's rows and lines, the talent tree's title and Attribute Bonus), out of the install's localization; the client fills in the simulator's numbers (without the file, its own words) | 2 KB |
| `sounds/` | 48 WAVs and 50 MP3s (announcer, hero move/attack voice lines, the horns, `ambient/morning.wav` and `night.wav`) resolved through sound events | 17 MB |

Formats: glTF 2.0 binary (`.glb`) with KTX2 textures next to it (relative URIs), PNG, WAV, MP3. A model's or
the map's texture is a KTX2 file as Dota stores it, block-compressed (BC1, BC3) with every mip level; an image
the importer makes from Dota's (a normal kept in DXT5's alpha and green, packed occlusion/roughness/metallic,
a channel as grey) is compressed from its decoded levels, to BC5 for a normal (its blue is worked out when
drawn) and BC1 or BC3 otherwise, and a DXT5 colour without its alpha becomes BC1 block for block. Interface
textures and the terrain's layers stay PNG. All WAVs
are PCM 44.1/22.05 kHz, 16-bit except `sounds/ui/ui_general_deny.wav` (8-bit, as Valve ships it);
Bevy's `wav` feature plays both. Voice lines (`sounds/vo/`) ship as MPEG-1 layer III, 44.1 kHz mono,
and are kept as `.mp3` (the stdlib cannot transcode them); playing them needs Bevy's `mp3` feature.

## Using the models

- **Coordinates.** glTF Y-up metres (Source inches × 0.0254): `gltf.x = dota.y`, `gltf.y = height`,
  `gltf.z = dota.x`. bota positions are Dota + 9216 on both axes. Models face +Z (Dota +x at yaw 0).
  `model_scale=` in the manifest is the unit's Dota `ModelScale`.
- **Clips.** `clip` lines map logical clips (`idle`, `run`, `attack`, `cast1..3`, `requiem`,
  `death`, ...) to glTF animation names and durations; `point=` is the Dota attack/cast point in
  seconds at base speed (SF attack 0.5, raze 0.55, requiem 1.67; Pudge attack 0.5, hook and
  dismember 0.3, rot 0; creeps 0.467 melee / 0.5 ranged / 0.7 siege; towers 0.6). Raze 1/2/3 use
  `cast1_shadowraze_a/b/c` (`ACT_DOTA_RAZE_1/2/3`). Pudge: `cast1` Meat Hook throw, `hook_end` while
  the hook returns, `cast2` Rot, `dismember` then the looping `dismember_channel`. Siege creeps have
  only idle/run/attack; they die by swapping in `creep.<side>.siege.destruction` (play `death` once).
  A clip lasts to its last frame, `(frames - 1) / fps`, as Dota plays it: a run of equal values
  keeps only its ends, the last frame's among them, so a clip that ends holding a pose (Pudge's
  hook throw, 87 frames at 30 fps, 2.867 s; a neutral's death) holds it to the end. Root motion is baked into the root
  bones; the client holds roots in place across the ground.
- **Neutral creeps.** One `model` block per model (`neutral.<name>.model`) and one `unit <npc_units.txt name>
  <model> model_scale=<ModelScale>` line per unit that wears it: the client finds a neutral's line by the
  game's unit name. `point=` is the unit's `AttackAnimationPoint`. `unit <name> none` draws nothing (the
  Wildwing's Tornado is an attachment model without a mesh). Models load when their first unit comes into view.
- **Wearables.** Shadow Fiend's default head, shoulders and arms (and Pudge's seven default items) are separate skinned meshes. Spawn
  them alongside the hero and point their skin joints at the hero's joint entities with the same
  name, **ignoring case** (`root0_jnt` vs `Root0_JNT`); `verify_assets.py` checks every wearable joint
  exists in the hero skeleton.
- **Materials** are mapped by texture name, as Source2Viewer 20 maps them when it cannot read a
  game's compiled shaders: base colour and normal maps are right; the metallic/roughness,
  occlusion and `KHR_materials_specular` channels come from Valve's mask textures and are
  approximate (setting metallic to 0 usually looks closer to Dota). Emissive textures carry the
  self-illumination mask with the default `emissiveFactor` 0. Every material keeps Valve's shader
  parameters in `extras.vmat`.
- **Buildings** (ancients, barracks, fountains) are static meshes without animations. When one dies,
  swap in its `*.destruction` model and play its `death` clip once.
- **Couriers**: `courier.<side>.model` is the walking donkey (idle/run/death/spawn),
  `courier.<side>.flying.model` the winged one (idle/run/death).
- **Flagbearers** use the melee meshes with their own animations; Dota attaches the banner at runtime
  on attachment `attach_banner`, which is not part of the exported model.

## The mid-lane map (`map.mid`)

`maps/dota.vpk`'s world is read whole and cut to the nodes touching the band
`|dota.x − dota.y| / √2 ≤ 13100`, `−11800 ≤ (dota.x + dota.y) / √2 ≤ 11700`: the mid lane from
fountain to fountain with both base plateaus and the ground the band touches either side of it. Towers, trees, barracks, ancients and fountain
buildings are removed (the client spawns them as units; see `NOT_IN_WORLD` in `crates/dbota_import/src/import.rs`); shops,
statues, walls, decorations and the fountains' water/glow meshes stay. Nodes are flattened to world
matrices; skins, animations and morph targets are dropped. Each triangle list using less than half of
its mesh set's vertices gets its own vertices: a mesh set shares its whole vertex buffers among
its pieces, and a glTF loader copies them whole for every piece (2.6 GB of vertices loaded for 70 MB
of geometry without this; 236 MB with it, for a 213 MB file instead of 129 MB). The result is what
Source2Viewer 20's export of the world, cropped this way, was (the golden tests compare them).

- **Terrain** meshes use Valve's 4-layer `multiblend.vfx`; the glTF material carries only layer 1
  (`g_tColor1`, `g_tNormal1`), the blend weights are in the custom vertex attributes `_TEXCOORD_1.._3`,
  and `extras.vmat.TextureParams` names every layer texture (`g_tColor0..3`, `g_tNormal0..3`, masks),
  exported to `textures/ground/<basename>.png`. The client bakes the four layers, by those weights and each
  layer texture's mean colour, into the vertex colours as it loads the map.
- **Water** (`water_dota.vfx`) has no colour texture; its normal/noise/flow maps are in `textures/ground/`.
- **Trees.** `map/mid/trees.txt` lists every tree on the whole map:
  `model dota_x dota_y height yaw_deg scale` (models under `models/tree/` exist for the 9 kinds used
  around mid; meshes `<tree>_model` and the stump).

## Not imported

- Particle systems (only a few of their textures, first frame of sprite sheets).
- Tower destruction models and non-default (event/crystal/mega) creep variants.
- The full map outside the mid band (import it by raising `half_width` / `v_min` / `v_max` on the
  `world` line (they are 13100 and −11800 / 11700 now); the whole map with materials is ~330 MB).
