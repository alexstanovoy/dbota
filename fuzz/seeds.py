#!/usr/bin/env python3
"""Seeds the corpora of the fuzz targets from a local Dota 2 install.

    python3 -I fuzz/seeds.py <.../dota 2 beta/game/dota>

Reads small files out of pak01_dir.vpk and copies a few small map packages.
Writes corpus/<target>/seed-*, which stay out of the repository: they are
Valve's files.
"""
import os
import struct
import sys
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
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


def write(target, name, data):
    folder = os.path.join(HERE, "corpus", target)
    os.makedirs(folder, exist_ok=True)
    with open(os.path.join(folder, "seed-" + name), "wb") as out:
        out.write(data)


def cstr(tree, at):
    end = tree.index(b"\0", at)
    return tree[at:end].decode(), end + 1


def entries(game):
    """(path, crc, preload, archive, offset, length) of every file of pak01."""
    with open(os.path.join(game, "pak01_dir.vpk"), "rb") as vpk:
        _, version, tree_len = struct.unpack("<III", vpk.read(12))
        header = 28 if version == 2 else 12
        vpk.seek(header)
        tree = vpk.read(tree_len)
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
                crc, preload, archive, offset, length, _ = struct.unpack_from("<IHHIIH", tree, at)
                at += 18
                data = tree[at : at + preload]
                at += preload
                path = f"{'' if folder == ' ' else folder + '/'}{name}.{ext}"
                yield path, ext, crc, data, archive, offset, length


def read(game, entry):
    _, _, crc, preload, archive, offset, length = entry
    tail = b""
    if length:
        with open(os.path.join(game, f"pak01_{archive:03}.vpk"), "rb") as part:
            part.seek(offset)
            tail = part.read(length)
    data = preload + tail
    return data if zlib.crc32(data) == crc else None


def blocks(data):
    """(kind, bytes) of every block of a compiled resource."""
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
        yield kind.decode("latin-1"), data[start : start + size]
        at += 12


def is_kv3(block):
    magic = block[:4]
    return magic == b"VKV\x03" or (magic[1:] == b"3VK" and 1 <= magic[0] <= 5)


def pick(game, files):
    """Small files of every kind, spread over sizes: the smallest are often empty shells."""
    chosen = {}
    for entry in files:
        ext = entry[1]
        size = len(entry[3]) + entry[6]
        limit = {"vtex_c": MAX_TEXTURE, "vsnd_c": MAX_SOUND}.get(ext, MAX_RESOURCE)
        if 256 <= size <= limit and (ext in KINDS or ext in ("vtex_c", "vsnd_c")):
            chosen.setdefault(ext, []).append(entry)
    for ext, group in chosen.items():
        group.sort(key=lambda e: len(e[3]) + e[6])
        want = KINDS.get(ext, 40)
        step = max(1, len(group) // want)
        yield from (e for e in group[::step][:want])


def texture_format(data):
    for kind, block in blocks(data):
        if kind == "DATA" and len(block) > 26:
            return block[26]
    return None


def main(game):
    formats = {}
    kv3_seen = set()
    for entry in pick(game, entries(game)):
        data = read(game, entry)
        if data is None:
            continue
        ext, name = entry[1], entry[0].replace("/", "_")
        if ext == "vtex_c":
            fmt = texture_format(data)
            if formats.get(fmt, 0) >= 3:
                continue
            formats[fmt] = formats.get(fmt, 0) + 1
            write("vtex", name, data)
        elif ext == "vsnd_c":
            write("vsnd", name, data)
        else:
            write("resource", name, data)
        for kind, block in blocks(data):
            key = (ext, kind, block[:1])
            if is_kv3(block) and len(block) <= MAX_KV3 and key not in kv3_seen:
                kv3_seen.add(key)
                write("kv3", f"{name}-{kind}", block)
            if kind == "NTRO":
                write("resource", f"ntro-{name}", data)
    maps = os.path.join(game, "maps", "scenes")
    small = sorted(
        (os.path.getsize(os.path.join(maps, f)), f) for f in os.listdir(maps) if f.endswith(".vpk")
    )
    for size, file in small[:12]:
        with open(os.path.join(maps, file), "rb") as vpk:
            write("vpk", file, vpk.read())


if __name__ == "__main__":
    main(sys.argv[1])
