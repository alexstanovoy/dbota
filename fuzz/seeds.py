#!/usr/bin/env python3
"""Seeds the corpora of dbota's fuzz targets from a local Dota 2 install and,
if given, the client's asset cache.

    python3 -I fuzz/seeds.py <.../dota 2 beta/game/dota> [<asset cache>]

Bundles the files a whole import reads into one input each (the layout
`heatel::fuzzing::files` reads), writes the manifest's plan, and copies the
cache's smallest images, sounds and text files. Writes corpus/<target>/seed-*
under $FUZZ_DATA (default ~/.cache/dbota-fuzz), outside the repository: they
are Valve's files. Reading the install is heatel's fuzz/seeds.py (the heatel
checkout beside dbota's, or $HEATEL_FUZZ); its own targets' seeds come from it.
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
# Nothing is written into heatel's checkout.
sys.dont_write_bytecode = True
sys.path.insert(0, os.environ.get("HEATEL_FUZZ") or os.path.join(HERE, "..", "..", "heatel", "fuzz"))
os.environ.setdefault(
    "FUZZ_DATA",
    os.path.join(os.environ.get("XDG_CACHE_HOME") or os.path.expanduser("~/.cache"), "dbota-fuzz"),
)
from seeds import MAX_BUNDLE, Package, bundle, excerpt, reachable, spread, write  # noqa: E402

TEXT_SEED = 64 << 10


def texts(pak):
    """Tooltip sources, portrait framings and the shop, cut to what the client asks for."""
    ids = open(os.path.join(HERE, "..", "assets-manifest", "ids.txt")).read()
    names, heroes = [], []
    for line in ids.splitlines():
        w = line.split("#")[0].split()
        if len(w) >= 3 and w[0] == "item":
            names.append("item_" + w[2])
        elif len(w) >= 3 and w[0] == "ability":
            names.append(w[2])
        elif len(w) >= 3 and w[0] == "hero":
            heroes.append(w[2])
    read = lambda p: (pak.read(p) or b"").decode("utf-8", "replace")
    localization = read("resource/localization/abilities_english.txt")
    loc = "".join(l for l in localization.splitlines(keepends=True) if any(n in l for n in names))
    scripts = [
        excerpt(read(p), names, 96 << 10)
        for p in ["scripts/npc/items.txt", "scripts/npc/npc_abilities.txt"]
        + [f"scripts/npc/heroes/npc_dota_hero_{h}.txt" for h in heroes[:2]]
    ]
    portraits = read("scripts/npc/portraits.txt")
    shops = read("scripts/shops.txt")
    words = "".join(
        l for l in read("resource/localization/dota_english.txt").splitlines(keepends=True)
        if "DOTA_SubShop_" in l or "DOTA_Shop_" in l or "DOTA_Inventory" in l
    )
    return loc, scripts, portraits, (shops, words)


def imports(pak, game, loc, scripts, portraits, shop):
    """Whole imports: one file of every manifest kind, then the files they name."""
    models_ = sorted((p for p in pak.files if p.endswith(".vmdl_c")), key=pak.size)
    models_ = [p for p in models_ if 16 << 10 <= pak.size(p) <= 96 << 10]
    textures = [p for p in pak.files if p.endswith(".vtex_c") and pak.size(p) <= 32 << 10]
    sounds = [p for p in pak.files if p.endswith(".vsnd_c") and pak.size(p) <= 32 << 10]
    world = next(
        (f for f in sorted(os.listdir(os.path.join(game, "maps", "scenes"))) if f.endswith(".vpk")),
        None,
    )
    framing = excerpt(portraits, ["models/heroes/nevermore/nevermore.vmdl"], 8 << 10)
    for n, (m, n_) in enumerate(zip(spread(models_, 4), spread(models_[1:], 4))):
        roots = [
            ("models/m.vmdl_c", pak.read(m)),
            ("models/n.vmdl_c", pak.read(n_)),
            ("materials/t.vtex_c", pak.read(textures[n * 7 % len(textures)])),
            ("sounds/s.vsnd_c", pak.read(sounds[n * 5 % len(sounds)])),
            ("maps/w/world.vwrld_c", b""),
            (
                "scripts/npc/portraits.txt",
                ('"Portraits" {\n' + framing.replace("models/heroes/nevermore/nevermore.vmdl", "models/m.vmdl") + "}\n").encode(),
            ),
            ("resource/localization/abilities_english.txt", loc[: 16 << 10].encode()),
        ] + [(f"script{i}", s[: 16 << 10].encode()) for i, s in enumerate(scripts[:3])]
        roots += [("scripts/shops.txt", shop[0].encode()), ("strings", shop[1].encode())]
        if any(d is None for _, d in roots):
            continue
        extra = []
        for path, data in roots[:2]:
            extra += reachable([pak], path, data, MAX_BUNDLE // 2)[1:]
        if world:
            package = Package(game, os.path.join("maps", "scenes", world))
            w = next((p for p in package.files if p.endswith("world.vwrld_c")), None)
            if w:
                roots[4] = ("maps/w/world.vwrld_c", package.read(w) or b"")
        files, total = [], 0
        for path, data in roots + extra:
            if total + len(data) <= MAX_BUNDLE:
                files.append((path, data))
                total += len(data)
        write("import", f"import{n}", bundle(files))


def own_texts():
    """The importer's plan from the client's own manifest and ids (not Valve's)."""
    root = os.path.join(HERE, "..", "assets-manifest")
    manifest = open(os.path.join(root, "manifest.txt"), "rb").read()
    ids = open(os.path.join(root, "ids.txt"), "rb").read()
    write("plan", "manifest", manifest[:48 << 10] + b"\0" + ids[:8 << 10] + b"\0v7/rev3/0/1/2\n")


def cache_seeds(cache):
    """The smallest images and sounds of the cache, a few of each kind, and its text files."""
    kinds_ = {".png": "cache_image", ".ktx2": "cache_image", ".wav": "cache_sound", ".mp3": "cache_sound"}
    found = {}
    for folder, _, files in os.walk(cache):
        for file in files:
            ext = os.path.splitext(file)[1]
            if ext in kinds_:
                path = os.path.join(folder, file)
                found.setdefault(ext, []).append((os.path.getsize(path), path))
    for ext, group in found.items():
        for size, path in sorted(group)[:: max(1, len(group) // 12)][:12]:
            if size <= 256 << 10:
                with open(path, "rb") as file:
                    write(kinds_[ext], os.path.basename(path), file.read())
    for rel in ["map/mid/trees.txt", "map/mid/buildings.txt", "map/mid/lighting.txt", "models/portraits.txt"]:
        path = os.path.join(cache, rel)
        if os.path.isfile(path):
            with open(path, "rb") as file:
                write("cache_text", rel.replace("/", "_"), file.read(TEXT_SEED))


def main(game):
    pak = Package(game, "pak01_dir.vpk")
    loc, scripts, portraits, shop = texts(pak)
    imports(pak, game, loc, scripts, portraits, shop)
    own_texts()


if __name__ == "__main__":
    main(sys.argv[1])
    if len(sys.argv) > 2:
        cache_seeds(sys.argv[2])
