#!/usr/bin/env python3
"""Seeds the corpora of the fuzz targets from a local Dota 2 install and,
if given, the client's asset cache.

    python3 -I fuzz/seeds.py <.../dota 2 beta/game/dota> [<asset cache>]

Reads small files out of pak01_dir.vpk and the small map packages, bundles
the files a model, a world or an import reads into one input each (the
layout fuzz_targets/common/files.rs reads), and copies the cache's smallest
images, sounds and text files. Writes corpus/<target>/seed-* under
$FUZZ_DATA (default ~/.cache/dbota-fuzz), outside the repository: they are
Valve's files.
"""
import os
import struct
import sys
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
DATA = os.environ.get("FUZZ_DATA") or os.path.join(
    os.environ.get("XDG_CACHE_HOME") or os.path.expanduser("~/.cache"), "dbota-fuzz"
)
# Resource kinds worth a seed each, with how many.
KINDS = {
    "vmat_c": 6, "vmdl_c": 6, "vpcf_c": 6, "vsndevts_c": 3, "vdata_c": 3, "vphys_c": 3,
    "vanim_c": 3, "vagrp_c": 3, "vseq_c": 3, "vmorf_c": 3, "vrman_c": 3, "vents_c": 3,
    "vsnap_c": 3, "vpost_c": 3, "vnmgraph_c": 3, "vmix_c": 3, "vjs_c": 2, "vcss_c": 2,
}
MAX_RESOURCE = 64 << 10
MAX_TEXTURE = 128 << 10
MAX_SOUND = 48 << 10
MAX_KV3 = 64 << 10
# The separator of the files of one input (fuzz_targets/common/files.rs).
SEPARATOR = b"\n%%dbota-file%%\n"
MAX_BUNDLE = 1 << 20
TEXT_SEED = 64 << 10


def write(target, name, data):
    folder = os.path.join(DATA, "corpus", target)
    os.makedirs(folder, exist_ok=True)
    with open(os.path.join(folder, "seed-" + name), "wb") as out:
        out.write(data)


def bundle(files):
    """One input of (path, bytes) files."""
    return SEPARATOR.join(path.encode() + b"\n" + data for path, data in files)


def cstr(tree, at):
    end = tree.index(b"\0", at)
    return tree[at:end].decode(), end + 1


class Package:
    """A VPK's files by path, read on demand: pak01_dir.vpk with its parts, or one file."""

    def __init__(self, game, dir_path):
        self.game, self.dir_path = game, dir_path
        with open(os.path.join(game, dir_path), "rb") as vpk:
            _, version, tree_len = struct.unpack("<III", vpk.read(12))
            header = 28 if version == 2 else 12
            vpk.seek(header)
            tree = vpk.read(tree_len)
        self.data_start = header + tree_len
        self.files = {}
        at = 0
        while True:
            ext, at = cstr(tree, at)
            if not ext:
                return
            while True:
                folder, at = cstr(tree, at)
                if not folder:
                    break
                while True:
                    name, at = cstr(tree, at)
                    if not name:
                        break
                    record = struct.unpack_from("<IHHII", tree, at)
                    at += 18
                    preload = tree[at : at + record[1]]
                    at += record[1]
                    path = f"{'' if folder == ' ' else folder + '/'}{name}.{ext}"
                    self.files[path] = (ext, record[0], preload, record[2], record[3], record[4])

    def size(self, path):
        entry = self.files[path]
        return len(entry[2]) + entry[5]

    def read(self, path):
        entry = self.files.get(path)
        if entry is None:
            return None
        _, crc, preload, archive, offset, length = entry
        tail = b""
        if length:
            if archive == 0x7FFF:
                part, offset = self.dir_path, offset + self.data_start
            else:
                part = self.dir_path.replace("_dir.vpk", f"_{archive:03}.vpk")
            with open(os.path.join(self.game, part), "rb") as file:
                file.seek(offset)
                tail = file.read(length)
        data = preload + tail
        return data if zlib.crc32(data) == crc else None


def blocks(data):
    """(kind, bytes, offset) of every block of a compiled resource."""
    if len(data) < 16:
        return
    table, count = struct.unpack_from("<II", data, 8)
    at = 8 + table
    for _ in range(min(count, 64)):
        if at + 12 > len(data):
            return
        kind = data[at : at + 4]
        offset, size = struct.unpack_from("<II", data, at + 4)
        start = at + 4 + offset
        yield kind.decode("latin-1"), data[start : start + size], start
        at += 12


def references(data):
    """The paths a resource's RERL block names, compiled (`x.vmat` is `x.vmat_c`)."""
    for kind, block, start in blocks(data):
        if kind != "RERL" or len(block) < 8:
            continue
        list_at = start + struct.unpack_from("<I", data, start)[0]
        count = struct.unpack_from("<I", data, start + 4)[0]
        for i in range(min(count, 256)):
            field = list_at + i * 16 + 8
            if field + 4 > len(data):
                return
            name_at = field + struct.unpack_from("<I", data, field)[0]
            end = data.find(b"\0", name_at)
            if end > 0:
                name = data[name_at:end].decode("utf-8", "replace")
                yield name if name.endswith("_c") else name + "_c"


def reachable(packages, root, data, limit):
    """`root` and the files its references reach in `packages`, within `limit` bytes."""
    files, seen, total = [(root, data)], {root}, len(data)
    at = 0
    while at < len(files):
        for path in references(files[at][1]):
            if path in seen:
                continue
            seen.add(path)
            found = next((p.read(path) for p in packages if path in p.files), None)
            if found is not None and total + len(found) <= limit:
                files.append((path, found))
                total += len(found)
        at += 1
    return files


def is_kv3(block):
    magic = block[:4]
    return magic == b"VKV\x03" or (magic[1:] == b"3VK" and 1 <= magic[0] <= 5)


def kinds(data):
    return {kind for kind, _, _ in blocks(data)}


def spread(paths, want):
    step = max(1, len(paths) // want)
    return paths[::step][:want]


def pick(pak):
    """Small files of every kind, spread over sizes: the smallest are often empty shells."""
    chosen = {}
    for path, entry in pak.files.items():
        ext = entry[0]
        size = pak.size(path)
        limit = {"vtex_c": MAX_TEXTURE, "vsnd_c": MAX_SOUND}.get(ext, MAX_RESOURCE)
        if 256 <= size <= limit and (ext in KINDS or ext in ("vtex_c", "vsnd_c")):
            chosen.setdefault(ext, []).append(path)
    for ext, group in chosen.items():
        group.sort(key=pak.size)
        yield from spread(group, KINDS.get(ext, 40))


def texture_format(data):
    for kind, block, _ in blocks(data):
        if kind == "DATA" and len(block) > 26:
            return block[26]
    return None


def formats(pak):
    """Seeds of the single-format targets: resources, KV3 blocks, textures, sounds."""
    seen_formats = {}
    kv3_seen = set()
    for path in pick(pak):
        data = pak.read(path)
        if data is None:
            continue
        ext, name = pak.files[path][0], path.replace("/", "_")
        if ext == "vtex_c":
            fmt = texture_format(data)
            if seen_formats.get(fmt, 0) >= 3:
                continue
            seen_formats[fmt] = seen_formats.get(fmt, 0) + 1
            write("vtex", name, data)
        elif ext == "vsnd_c":
            write("vsnd", name, data)
        else:
            write("resource", name, data)
        for kind, block, _ in blocks(data):
            key = (ext, kind, block[:1])
            if is_kv3(block) and len(block) <= MAX_KV3 and key not in kv3_seen:
                kv3_seen.add(key)
                write("kv3", f"{name}-{kind}", block)
            if kind == "NTRO":
                write("resource", f"ntro-{name}", data)


def models(pak):
    """Models whole (meshes, materials, textures), their animations, and materials."""
    paths = sorted((p for p in pak.files if p.endswith(".vmdl_c")), key=pak.size)
    small = [p for p in paths if 8 << 10 <= pak.size(p) <= 256 << 10]
    animated = 0
    for path in spread(small, 400):
        data = pak.read(path)
        if data is None:
            continue
        name = path.replace("/", "_")
        has = kinds(data)
        if {"ANIM", "AGRP"} & has and animated < 16:
            animated += 1
            write("anim", name, data)
        for kind, block, _ in blocks(data):
            if kind in ("VBIB", "MBUF") and len(block) <= 128 << 10:
                write("vbib", f"{name}-{kind}", block + b"\x03")
    for path in spread(small, 12):
        data = pak.read(path)
        if data is not None:
            write("model", path.replace("/", "_"), bundle(reachable([pak], path, data, MAX_BUNDLE)))
    materials = sorted((p for p in pak.files if p.endswith(".vmat_c")), key=pak.size)
    for path in spread([p for p in materials if pak.size(p) <= 128 << 10], 24):
        data = pak.read(path)
        if data is not None:
            write("material", path.replace("/", "_"), data)


def worlds(game, pak):
    """The small maps' worlds with what they place, and their entity lumps."""
    maps = os.path.join(game, "maps")
    found = []
    for folder, _, names in os.walk(maps):
        for file in names:
            if file.endswith(".vpk"):
                full = os.path.join(folder, file)
                found.append((os.path.getsize(full), os.path.relpath(full, game)))
    found.sort()
    for _, rel in found[:12]:
        write("vpk", os.path.basename(rel), open(os.path.join(game, rel), "rb").read())
    worlds_written = 0
    for _, rel in found[:40]:
        try:
            package = Package(game, rel)
        except (OSError, struct.error, ValueError):
            continue
        for path in package.files:
            if path.endswith(".vents_c") and package.size(path) <= 256 << 10:
                data = package.read(path)
                if data is not None:
                    write("entities", rel.replace("/", "_") + "-" + os.path.basename(path), data)
        world = next((p for p in package.files if p.endswith("world.vwrld_c")), None)
        if world is None or worlds_written >= 8:
            continue
        files = [(world, package.read(world))]
        files += [
            (p, package.read(p))
            for p in package.files
            if p.endswith((".vwnod_c", ".vents_c")) and package.size(p) <= 256 << 10
        ]
        files = [(p, d) for p, d in files if d is not None]
        extra = []
        for path, data in files:
            extra += reachable([package, pak], path, data, MAX_BUNDLE // 2)[1:]
        all_files, total = [], 0
        for path, data in files + extra:
            if total + len(data) <= MAX_BUNDLE and path not in dict(all_files):
                all_files.append((path, data))
                total += len(data)
        write("world", rel.replace("/", "_"), bundle(all_files))
        worlds_written += 1


def excerpt(text, keys, limit):
    """The lines of a KeyValues text from each line naming one of `keys` to the end of its
    block, within `limit` bytes."""
    lines = text.splitlines(keepends=True)
    out, total, i = [], 0, 0
    while i < len(lines) and total < limit:
        if any(f'"{k}"' in lines[i] for k in keys):
            depth, j = 0, i
            while j < len(lines):
                depth += lines[j].count("{") - lines[j].count("}")
                j += 1
                if depth <= 0 and j > i + 1:
                    break
            chunk = "".join(lines[i:j])
            out.append(chunk)
            total += len(chunk)
            i = j
        else:
            i += 1
    return "".join(out)


def texts(pak):
    """Tooltip sources and portrait framings, cut to what the client asks for."""
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
    files = [("", loc[: 64 << 10].encode())] + [("", s.encode()) for s in scripts]
    write("tooltips", "abilities", bundle(files))
    portraits = read("scripts/npc/portraits.txt")
    write("portraits", "excerpt", portraits[: 200 << 10].encode())
    return loc, scripts, portraits


def imports(pak, game, loc, scripts, portraits):
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
    formats(pak)
    models(pak)
    worlds(game, pak)
    loc, scripts, portraits = texts(pak)
    imports(pak, game, loc, scripts, portraits)
    own_texts()


if __name__ == "__main__":
    main(sys.argv[1])
    if len(sys.argv) > 2:
        cache_seeds(sys.argv[2])
