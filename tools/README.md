# Dota 2 asset import

`import_assets.sh` exports the Dota 2 models, map, textures and sounds that dbota draws into a local
cache. **Valve's files are for personal use only**: they live in the cache, outside any repository,
and are never committed or published. Only our own tooling and the manifest are in git.

```sh
tools/import_assets.sh                      # Flatpak Steam, then ~/.steam/steam, then ~/.local/share/Steam
tools/import_assets.sh --dota-path "/path/to/steamapps/common/dota 2 beta"
DBOTA_ASSETS=/elsewhere tools/import_assets.sh   # cache location (default ~/.cache/dbota/v<N>)
```

Requirements: an installed Dota 2, `curl`, `unzip`, `sha256sum`, `python3` (stdlib only), Linux x64.
The script downloads [Source2Viewer-CLI](https://github.com/ValveResourceFormat/ValveResourceFormat)
**20.0** (`cli-linux-x64.zip`, released 2026-08-17, SHA256
`3e8af47cd6ce52e8068904f2aa1dda23c56a6b96a8310b25090f0711cda76a8a`) into `~/.cache/dbota/tools/s2v-20.0/`
(shared by every version's folder; `$DBOTA_ASSETS/tools/` when that is set)
and refuses an archive with any other checksum.

A first run takes about 3.5 minutes; it stages the full map export (~330 MB) in a `mktemp` directory
under `$TMPDIR` and removes it on exit. Re-running is a no-op in under a second: textures and sounds are skipped when their output
exists; models and the map are re-exported only when their manifest line (or the export version,
`../assets-manifest/version`) changed. A new version exports into a new folder, `~/.cache/dbota/v<N>`;
the client reads the folder of the version it was built with. Each run ends with `verify_assets.py`, which fails the import on any missing or
malformed output.

## Files

| file | role |
| --- | --- |
| `../assets-manifest/manifest.txt` | every exported asset: logical name, VPK source, cache path, clips; format in its header |
| `../assets-manifest/version` | the export version: the cache folder `v<N>`, and the stamps that force a re-export |
| `../assets-manifest/ids.txt` | bota item/ability/hero ids → Dota names |
| `export_tooltips.py` | boils the install's localization and item/ability scripts down to `text/tooltips.json` |
| `import_assets.sh` | the importer |
| `visual_check.sh` | measures the picture of a recorded match (see the main README) |
| `bench.sh` | measures the binary's size, start and frame times |
| `crop_world.py` | crops the exported map to the mid-lane band, lists terrain layers and trees |
| `verify_assets.py` | checks glTF meshes/skins/clips/textures, wearable joints, PNG sizes, WAV/MP3 format |
| `inspect_gltf.py` | prints a model's meshes, skins and animations (`--json` for machines) |

## What the cache holds (556 MB per `v<N>` folder, plus 124 MB of the CLI shared by all)

| path | content | size |
| --- | --- | --- |
| `models/hero/nevermore/` | Shadow Fiend, 17 clips, plus `nevermore_{head,shoulders,arms}/` default wearables | 16 MB |
| `models/hero/pudge/` | Pudge, 18 clips, plus `pudge_{weapon,back,offhand,shoulder,arms,head,belt}/` default wearables | 4 MB |
| `models/creep/` | Radiant/Dire melee, ranged, flagbearer (6 clips each); siege (3 clips) and its `*_destruction` break-apart model | 41 MB |
| `models/tower/` | Radiant (`radiant_tower002` + base) and Dire (`dire_tower002`) tier-1 towers | 4 MB |
| `models/tree/` | the 9 tree models the map uses around mid (standing mesh + stump) | 8 MB |
| `models/building/` | ancients, melee/ranged barracks (static, plus `*_destruction` collapse models), fountains | 73 MB |
| `models/courier/` | default donkey courier, Radiant and Dire, walking and flying | 4 MB |
| `map/mid/` | `mid.gltf` with its geometry in `mid.bin`, the default map from fountain to fountain around mid, its textures, `trees.txt`, `buildings.txt` (the towers' places) | 354 MB |
| `textures/ground/` | terrain layer textures (also used by `map.mid`) | 37 MB |
| `textures/particle/` | 13 effect textures (smoke, glow, sparks, ground cracks, fire, ...) | 1 MB |
| `icons/`, `hud/`, `minimap/` | item and ability icons, hero portraits, minimap hero/creep/tower/building icons, HUD art, minimap | 2 MB |
| `text/tooltips.json` | the names, descriptions, lore, notes and number labels of the items and abilities in `ids.txt`, read out of the install's own localization and scripts by `export_tooltips.py`; the client puts the simulator's numbers into them (without the file, tooltips show only the simulator's plain names and numbers) | 30 KB |
| `sounds/` | 48 WAVs and 50 MP3s (announcer, hero move/attack voice lines, the horns, `ambient/morning.wav` and `night.wav`) resolved through sound events | 17 MB |

Formats: glTF 2.0 binary (`.glb`) with PNG textures next to it (relative URIs), PNG, WAV, MP3. All WAVs
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
  Tower "attack" clips are single-frame poses. A clip lasts until its last keyframe: a run of equal
  values keeps only its ends, so a clip that ends holding a pose stops where the pose is reached
  (Pudge's hook throw has 87 frames at 30 fps but lasts 0.9 s). Root motion is baked into the root
  bones; the client holds roots in place across the ground.
- **Wearables.** Shadow Fiend's default head, shoulders and arms (and Pudge's seven default items) are separate skinned meshes. Spawn
  them alongside the hero and point their skin joints at the hero's joint entities with the same
  name, **ignoring case** (`root0_jnt` vs `Root0_JNT`); `verify_assets.py` checks every wearable joint
  exists in the hero skeleton.
- **Materials.** Source2Viewer 20.0 cannot read Dota's current shader files (VCS v72), so materials
  are mapped by texture name: base colour and normal maps are right; the metallic/roughness,
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

`maps/dota.vpk`'s world is exported whole and cropped by `crop_world.py` to nodes touching the band
`|dota.x − dota.y| / √2 ≤ 13100`, `−11800 ≤ (dota.x + dota.y) / √2 ≤ 11700`: the mid lane from
fountain to fountain with both base plateaus and the ground the band touches either side of it. Towers, trees, barracks, ancients and fountain
buildings are removed (the client spawns them as units; see `EXCLUDE` in `crop_world.py`); shops,
statues, walls, decorations and the fountains' water/glow meshes stay. Nodes are flattened to world
matrices; skins, animations and morph targets are dropped. Each triangle list using less than half of
its mesh set's vertices gets its own vertices: the exporter shares a set's whole vertex buffers among
its pieces, and a glTF loader copies them whole for every piece (2.6 GB of vertices loaded for 70 MB
of geometry before this; 236 MB after, for a 213 MB file instead of 129 MB).

- **Terrain** meshes use Valve's 4-layer `multiblend.vfx`; the glTF material carries only layer 1
  (`g_tColor1`, `g_tNormal1`), the blend weights are in the custom vertex attributes `_TEXCOORD_1.._3`,
  and `extras.vmat.TextureParams` names every layer texture (`g_tColor0..3`, `g_tNormal0..3`, masks),
  exported to `textures/ground/<basename>.png`. The client bakes the four layers, by those weights and each
  layer texture's mean colour, into the vertex colours as it loads the map.
- **Water** (`water_dota.vfx`) has no colour texture; its normal/noise/flow maps are in `textures/ground/`.
- **Trees.** `map/mid/trees.txt` lists every tree on the whole map:
  `model dota_x dota_y height yaw_deg scale` (models under `models/tree/` exist for the 9 kinds used
  around mid; meshes `<tree>_model` and the stump).

## Not exported

- Particle systems (only a few of their textures, first frame of sprite sheets).
- Tower destruction models and non-default (event/crystal/mega) creep variants.
- The full map outside the mid band (export it by raising `half_width` / `v_min` / `v_max` on the
  `world` line (they are 13100 and −11800 / 11700 now); the whole map with materials is ~330 MB).
