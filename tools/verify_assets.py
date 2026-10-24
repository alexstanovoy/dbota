#!/usr/bin/env python3
"""Check every manifest output in the cache: glTF structure and clips, PNG size, WAV/MP3 format.

Labels are checked in text/labels.json.

Usage: verify_assets.py <manifest.txt> <cache dir>. Exits non-zero when anything is missing or wrong.
"""
import json
import struct
import sys
from pathlib import Path

from inspect_gltf import durations, load_json

DURATION_TOLERANCE_S = 0.01


def parse_manifest(path: Path) -> list[tuple[str, list[str], dict[str, str]]]:
    entries = []
    for raw in path.read_text().splitlines():
        line = raw.split("#", 1)[0].split()
        if not line:
            continue
        fields = [f for f in line if "=" not in f]
        keys = dict(f.split("=", 1) for f in line if "=" in f)
        entries.append((fields[0], fields[1:], keys))
    return entries


def png_size(path: Path) -> tuple[int, int]:
    head = path.read_bytes()[:24]
    if head[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    return struct.unpack(">II", head[16:24])


def wav_info(path: Path) -> str:
    data = path.read_bytes()
    if data[:4] != b"RIFF" or data[8:12] != b"WAVE":
        raise ValueError("not a RIFF/WAVE file")
    pos, fmt, size = 12, None, None
    while pos + 8 <= len(data):
        cid, clen = data[pos : pos + 4], struct.unpack_from("<I", data, pos + 4)[0]
        if cid == b"fmt ":
            fmt = struct.unpack_from("<HHIIHH", data, pos + 8)
        elif cid == b"data":
            size = clen
        pos += 8 + clen + (clen & 1)
    if fmt is None or size is None:
        raise ValueError("missing fmt/data chunk")
    tag, channels, rate, byte_rate, _align, bits = fmt
    if tag != 1 or bits not in (8, 16):
        raise ValueError(f"not 8/16-bit PCM (format {tag}, {bits} bits)")
    return f"pcm{bits} {channels}ch {rate}Hz {size / byte_rate:.2f}s"


def mp3_info(path: Path) -> str:
    data = path.read_bytes()
    pos = 0
    if data[:3] == b"ID3":  # skip an ID3v2 tag: 10-byte header, syncsafe size
        pos = 10 + sum((b & 0x7F) << (7 * (3 - i)) for i, b in enumerate(data[6:10]))
    if len(data) < pos + 4 or data[pos] != 0xFF or data[pos + 1] & 0xE0 != 0xE0:
        raise ValueError("not an MPEG audio stream")
    rate = {0: 44100, 1: 48000, 2: 32000}.get((data[pos + 2] >> 2) & 3)
    if rate is None or (data[pos + 1] >> 3) & 3 != 3 or (data[pos + 1] >> 1) & 3 != 1:
        raise ValueError("not MPEG-1 layer III")
    return f"mp3 {rate}Hz {len(data)} bytes"


def joint_names(doc: dict) -> set[str]:
    """Lower-cased: Source bone-merges by case-insensitive name (wearables say root0_jnt, SF Root0_JNT)."""
    return {doc["nodes"][j].get("name", "").lower() for skin in doc.get("skins", []) for j in skin["joints"]}


def check_model(path: Path, keys: dict[str, str], clips: list[list[str]], parent: Path | None) -> str:
    doc = load_json(path)
    if not doc.get("meshes"):
        raise ValueError("no meshes")
    anims = dict(durations(doc))
    if ("anims" in keys or parent) and not doc.get("skins"):
        raise ValueError("animated or attached model without a skin")
    if parent:
        foreign = joint_names(doc) - joint_names(load_json(parent))
        if foreign:
            raise ValueError(f"joints missing from {keys['attach']}: {sorted(foreign)}")
    for _model, logical, name, expected, *_ in clips:
        if name not in anims:
            raise ValueError(f"clip {logical}: animation {name} missing")
        if abs(anims[name] - float(expected)) > DURATION_TOLERANCE_S:
            raise ValueError(f"clip {logical}: {name} lasts {anims[name]:.3f}s, manifest says {expected}")
    images = [i.get("uri") for i in doc.get("images", []) if "uri" in i]
    missing = [u for u in images if not (path.parent / u).is_file()]
    if missing:
        raise ValueError(f"missing textures {missing}")
    buffers = [b["uri"] for b in doc.get("buffers", []) if "uri" in b]
    missing = [u for u in buffers if not (path.parent / u).is_file()]
    if missing:
        raise ValueError(f"missing buffers {missing}")
    return (f"meshes={len(doc['meshes'])} skins={len(doc.get('skins', []))} "
            f"materials={len(doc.get('materials', []))} images={len(images)} anims={len(anims)}")


def check_particle(path: Path, cache: Path) -> str:
    doc = json.loads(path.read_text())
    if doc.get("version") != 1 or not doc.get("systems"):
        raise ValueError("not a version 1 particle file with systems")
    textures = doc.get("textures", [])
    missing = [t["file"] for t in textures if not (cache / t["file"]).is_file()]
    if missing:
        raise ValueError(f"textures missing: {missing}")
    return f"systems={len(doc['systems'])} textures={len(textures)}"


def check(kind: str, fields: list[str], keys: dict[str, str], clips, outputs: dict[str, Path], cache: Path) -> str:
    path = cache / fields[2]  # its own output: sound variants share one logical name
    if not path.is_file():
        raise ValueError("missing")
    if kind in ("model", "world"):
        parent = outputs[keys["attach"]] if "attach" in keys else None
        return check_model(path, keys, [c for c in clips if c[0] == fields[0]], parent)
    if kind == "texture":
        return "%dx%d" % png_size(path)
    if kind in ("portraits", "shop"):  # text the client parses itself
        return f"{path.stat().st_size} bytes"
    if kind == "particle":
        return check_particle(path, cache)
    return mp3_info(path) if path.suffix == ".mp3" else wav_info(path)


def main() -> None:
    manifest, cache = Path(sys.argv[1]), Path(sys.argv[2])
    entries = parse_manifest(manifest)
    clips = [fields for kind, fields, _ in entries if kind == "clip"]
    outputs = {fields[0]: cache / fields[2] for kind, fields, _ in entries if kind not in ("clip", "unit", "label")}
    labels_path = cache / "text/labels.json"
    labels = json.loads(labels_path.read_text()) if labels_path.is_file() else {}
    failures = 0
    for kind, fields, keys in entries:
        if kind == "clip":
            continue
        if kind == "label":  # all labels go to one file, under their key lower-cased
            if fields[1].lower() not in labels:
                failures += 1
                print(f"FAIL {fields[1]:44} not in text/labels.json")
            continue
        if kind == "unit":
            if fields[1] != "none" and fields[1] not in outputs:
                failures += 1
                print(f"FAIL {fields[0]:44} no model {fields[1]}")
            continue
        try:
            detail = check(kind, fields, keys, clips, outputs, cache)
            print(f"ok   {fields[0]:44} {fields[2]:60} {detail}")
        except (OSError, ValueError, KeyError, struct.error) as err:
            failures += 1
            print(f"FAIL {fields[0]:44} {fields[2]:60} {err}")
    print(f"{failures} failure(s)")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
