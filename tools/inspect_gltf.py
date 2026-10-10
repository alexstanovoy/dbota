#!/usr/bin/env python3
"""Summarise a .glb/.gltf: meshes, skins, materials, images, animations (name, duration)."""
import json
import struct
import sys
from pathlib import Path


def load_json(path: Path) -> dict:
    data = path.read_bytes()
    if path.suffix == ".gltf":
        return json.loads(data)
    magic, _version, _length = struct.unpack_from("<III", data, 0)
    if magic != 0x46546C67:
        raise ValueError(f"{path}: not a glb")
    chunk_len, chunk_type = struct.unpack_from("<II", data, 12)
    if chunk_type != 0x4E4F534A:
        raise ValueError(f"{path}: first chunk is not JSON")
    return json.loads(data[20 : 20 + chunk_len])


def durations(doc: dict) -> list[tuple[str, float]]:
    accessors = doc.get("accessors", [])
    out = []
    for anim in doc.get("animations", []):
        end = 0.0
        for sampler in anim.get("samplers", []):
            acc = accessors[sampler["input"]]
            end = max(end, (acc.get("max") or [0.0])[0])
        out.append((anim.get("name", "?"), end))
    return out


def main() -> None:
    as_json = "--json" in sys.argv
    for arg in (a for a in sys.argv[1:] if a != "--json"):
        doc = load_json(Path(arg))
        summary = {
            "file": arg,
            "meshes": len(doc.get("meshes", [])),
            "skins": len(doc.get("skins", [])),
            "joints": sum(len(s.get("joints", [])) for s in doc.get("skins", [])),
            "materials": len(doc.get("materials", [])),
            "images": [i.get("uri", "<embedded>") for i in doc.get("images", [])],
            "animations": durations(doc),
        }
        if as_json:
            print(json.dumps(summary))
            continue
        print(f"{arg}: meshes={summary['meshes']} skins={summary['skins']} "
              f"joints={summary['joints']} materials={summary['materials']} "
              f"images={len(summary['images'])}")
        for name, dur in summary["animations"]:
            print(f"  anim {name} {dur:.3f}s")


if __name__ == "__main__":
    main()
