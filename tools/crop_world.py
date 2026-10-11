#!/usr/bin/env python3
"""Crop an exported Dota world (.gltf + .bin from Source2Viewer-CLI) to the mid-lane band.

Keeps static scenery whose world-space bounds touch a band around the mid-lane
diagonal, flattens the node tree (each kept mesh node becomes a root with its
world matrix), drops skins/animations/morph targets, removes nodes whose name
matches --exclude (trees, towers, barracks, ancients, fountains: the client
spawns those itself because they can be cut or destroyed), and writes a .glb whose images stay external
files copied next to it.

Coordinates: the exporter writes glTF Y-up metres with
  gltf.x = dota.y * 0.0254, gltf.y = height * 0.0254, gltf.z = dota.x * 0.0254
(Dota map units; bota uses dota + 9216 on both axes).
"""
import argparse
import json
import math
import re
import shutil
import struct
from pathlib import Path

INCH = 0.0254
STRIP_ATTRIBUTES = ("JOINTS_0", "WEIGHTS_0", "JOINTS_1", "WEIGHTS_1")
LAYERED_SHADERS = ("multiblend.vfx", "water_dota.vfx")
SKIP_LAYER = re.compile(r"(refl|specular|height)")
TREE = re.compile(r"^(tree_|dire_tree)")
# Node names left out of the crop: trees, towers, barracks, ancients and fountains (the client spawns
# those as units), the end-game cameras, and the lower-detail world layers (n0_lr1+).
EXCLUDE = (r"^(tree_|dire_tree|radiant_tower|dire_tower|n0_lr[1-9]|radiant_ancient001|dire_ancient_base001"
           r"|radiant_(melee|ranged)_barracks001|dire_barracks_(melee|ranged)001|radiant_fountain002"
           r"|dire_fountain002|bad_ancient_destruction_camera|radiant_endcam)")


def mat_mul(a: list[float], b: list[float]) -> list[float]:
    """Column-major 4x4 product a*b."""
    out = [0.0] * 16
    for col in range(4):
        for row in range(4):
            out[col * 4 + row] = sum(a[k * 4 + row] * b[col * 4 + k] for k in range(4))
    return out


def node_matrix(node: dict) -> list[float]:
    if "matrix" in node:
        return node["matrix"]
    tx, ty, tz = node.get("translation", [0.0, 0.0, 0.0])
    qx, qy, qz, qw = node.get("rotation", [0.0, 0.0, 0.0, 1.0])
    sx, sy, sz = node.get("scale", [1.0, 1.0, 1.0])
    rot = [
        1 - 2 * (qy * qy + qz * qz), 2 * (qx * qy + qz * qw), 2 * (qx * qz - qy * qw),
        2 * (qx * qy - qz * qw), 1 - 2 * (qx * qx + qz * qz), 2 * (qy * qz + qx * qw),
        2 * (qx * qz + qy * qw), 2 * (qy * qz - qx * qw), 1 - 2 * (qx * qx + qy * qy),
    ]
    return [
        rot[0] * sx, rot[1] * sx, rot[2] * sx, 0.0,
        rot[3] * sy, rot[4] * sy, rot[5] * sy, 0.0,
        rot[6] * sz, rot[7] * sz, rot[8] * sz, 0.0,
        tx, ty, tz, 1.0,
    ]


def world_matrices(doc: dict) -> dict[int, list[float]]:
    identity = [1.0, 0, 0, 0, 0, 1.0, 0, 0, 0, 0, 1.0, 0, 0, 0, 0, 1.0]
    out: dict[int, list[float]] = {}
    stack = [(root, identity) for scene in doc["scenes"] for root in scene["nodes"]]
    while stack:
        index, parent = stack.pop()
        world = mat_mul(parent, node_matrix(doc["nodes"][index]))
        out[index] = world
        stack.extend((child, world) for child in doc["nodes"][index].get("children", []))
    return out


def mesh_corners(doc: dict, mesh: dict) -> list[tuple[float, float, float]]:
    lo = [math.inf] * 3
    hi = [-math.inf] * 3
    for prim in mesh["primitives"]:
        acc = doc["accessors"][prim["attributes"]["POSITION"]]
        lo = [min(a, b) for a, b in zip(lo, acc["min"])]
        hi = [max(a, b) for a, b in zip(hi, acc["max"])]
    return [(x, y, z) for x in (lo[0], hi[0]) for y in (lo[1], hi[1]) for z in (lo[2], hi[2])]


def touches_band(world: list[float], corners, args) -> bool:
    us, vs = [], []
    for x, y, z in corners:
        wx = world[0] * x + world[4] * y + world[8] * z + world[12]
        wz = world[2] * x + world[6] * y + world[10] * z + world[14]
        dota_x, dota_y = wz / INCH, wx / INCH
        us.append((dota_x - dota_y) / math.sqrt(2.0))
        vs.append((dota_x + dota_y) / math.sqrt(2.0))
    u_ok = min(us) <= args.half_width and max(us) >= -args.half_width
    return u_ok and min(vs) <= args.v_max and max(vs) >= args.v_min


class Packer:
    """Copies referenced bufferViews/accessors/materials into a fresh document."""

    def __init__(self, doc: dict, blob: bytes):
        self.doc, self.blob = doc, blob
        self.out_blob = bytearray()
        self.views: dict[int, int] = {}
        self.accessors: dict[int, int] = {}
        self.materials: dict[int, int] = {}
        self.textures: dict[int, int] = {}
        self.images: dict[int, int] = {}
        self.samplers: dict[int, int] = {}
        self.new = {k: [] for k in ("bufferViews", "accessors", "materials", "textures", "images", "samplers")}

    def view(self, index: int) -> int:
        if index not in self.views:
            src = self.doc["bufferViews"][index]
            start = src.get("byteOffset", 0)
            data = self.blob[start : start + src["byteLength"]]
            while len(self.out_blob) % 4:
                self.out_blob.append(0)
            dst = {k: v for k, v in src.items() if k not in ("buffer", "byteOffset")}
            dst.update(buffer=0, byteOffset=len(self.out_blob))
            self.out_blob += data
            self.views[index] = len(self.new["bufferViews"])
            self.new["bufferViews"].append(dst)
        return self.views[index]

    def accessor(self, index: int) -> int:
        if index not in self.accessors:
            src = dict(self.doc["accessors"][index])
            if "sparse" in src:
                raise ValueError("sparse accessors are not supported")
            if "bufferView" in src:
                src["bufferView"] = self.view(src["bufferView"])
            self.accessors[index] = len(self.new["accessors"])
            self.new["accessors"].append(src)
        return self.accessors[index]

    def _remap(self, table: dict, kind: str, index: int, fix) -> int:
        if index not in table:
            item = json.loads(json.dumps(self.doc[kind][index]))
            fix(item)
            table[index] = len(self.new[kind])
            self.new[kind].append(item)
        return table[index]

    def texture(self, index: int) -> int:
        def fix(tex: dict) -> None:
            if "source" in tex:
                tex["source"] = self._remap(self.images, "images", tex["source"], lambda _: None)
            if "sampler" in tex:
                tex["sampler"] = self._remap(self.samplers, "samplers", tex["sampler"], lambda _: None)
        return self._remap(self.textures, "textures", index, fix)

    def material(self, index: int) -> int:
        def fix(mat: dict) -> None:
            self._fix_texture_refs(mat)
        return self._remap(self.materials, "materials", index, fix)

    def _fix_texture_refs(self, obj) -> None:
        if isinstance(obj, dict):
            for key, value in obj.items():
                if key == "extras":  # Valve shader params (vmat), kept verbatim for the client
                    continue
                if key == "index" and isinstance(value, int):
                    obj[key] = self.texture(value)
                else:
                    self._fix_texture_refs(value)
        elif isinstance(obj, list):
            for value in obj:
                self._fix_texture_refs(value)

    def mesh(self, mesh: dict) -> dict:
        prims = []
        for prim in mesh["primitives"]:
            attrs = {k: self.accessor(v) for k, v in prim["attributes"].items() if k not in STRIP_ATTRIBUTES}
            new = {"attributes": attrs, "mode": prim.get("mode", 4)}
            if "indices" in prim:
                new["indices"] = self.accessor(prim["indices"])
            if "material" in prim:
                new["material"] = self.material(prim["material"])
            prims.append(new)
        return {"name": mesh.get("name", ""), "primitives": prims}


def write_glb(path: Path, doc: dict, blob: bytes) -> None:
    raw = json.dumps(doc, separators=(",", ":")).encode()
    raw += b" " * (-len(raw) % 4)
    blob = bytes(blob) + b"\0" * (-len(blob) % 4)
    total = 12 + 8 + len(raw) + 8 + len(blob)
    with path.open("wb") as fh:
        fh.write(struct.pack("<III", 0x46546C67, 2, total))
        fh.write(struct.pack("<II", len(raw), 0x4E4F534A) + raw)
        fh.write(struct.pack("<II", len(blob), 0x004E4942) + blob)


def crop(doc: dict, blob: bytes, args) -> tuple[dict, Packer]:
    exclude = re.compile(args.exclude)
    packer = Packer(doc, blob)
    meshes: dict[int, int] = {}
    nodes, out_meshes = [], []
    for index, world in sorted(world_matrices(doc).items()):
        node = doc["nodes"][index]
        if "mesh" not in node or exclude.search(node.get("name", "")):
            continue
        if not touches_band(world, mesh_corners(doc, doc["meshes"][node["mesh"]]), args):
            continue
        if node["mesh"] not in meshes:
            meshes[node["mesh"]] = len(out_meshes)
            out_meshes.append(packer.mesh(doc["meshes"][node["mesh"]]))
        nodes.append({"name": node.get("name", ""), "matrix": world, "mesh": meshes[node["mesh"]]})
    out = {
        "asset": {"version": "2.0", "generator": "dbota crop_world.py over " + doc["asset"].get("generator", "?")},
        "scene": 0,
        "scenes": [{"name": "mid", "nodes": list(range(len(nodes)))}],
        "nodes": nodes,
        "meshes": out_meshes,
        "buffers": [{"byteLength": len(packer.out_blob) + (-len(packer.out_blob) % 4)}],
        **{k: v for k, v in packer.new.items() if v},
    }
    used_ext = sorted({k for m in packer.new["materials"] for k in m.get("extensions", {})})
    if used_ext:
        out["extensionsUsed"] = used_ext
    return out, packer


def layer_textures(materials: list[dict]) -> list[str]:
    """Compiled texture paths behind blended terrain and water (the glTF only carries layer 0)."""
    paths = set()
    for mat in materials:
        vmat = mat.get("extras", {}).get("vmat", {})
        if vmat.get("ShaderName") in LAYERED_SHADERS:
            paths.update(v for v in vmat.get("TextureParams", {}).values() if not SKIP_LAYER.search(v))
    return sorted(p + "_c" for p in paths)


def tree_lines(doc: dict) -> list[str]:
    """Every tree on the whole map as `model dota_x dota_y height yaw_deg scale`."""
    seen, lines = set(), []
    for index, world in sorted(world_matrices(doc).items()):
        name = doc["nodes"][index].get("name", "")
        if "mesh" not in doc["nodes"][index] or not TREE.search(name):
            continue
        model = name.split(".")[0]
        dota = (world[14] / INCH, world[12] / INCH, world[13] / INCH)
        key = (model, round(dota[0]), round(dota[1]))
        if key in seen:
            continue
        seen.add(key)
        yaw = math.degrees(math.atan2(-world[2], world[0]))
        scale = math.sqrt(world[0] ** 2 + world[1] ** 2 + world[2] ** 2)
        lines.append(f"{model} {dota[0]:.1f} {dota[1]:.1f} {dota[2]:.1f} {yaw:.1f} {scale:.3f}")
    return lines


BUILDING = re.compile(r"^(radiant_tower|dire_tower|radiant_ancient001|dire_ancient_base001"
                      r"|radiant_(melee|ranged)_barracks001|dire_barracks_(melee|ranged)001"
                      r"|radiant_fountain002|dire_fountain002)")


def building_lines(doc: dict) -> list[str]:
    """Every building node on the map as `name dota_x dota_y yaw_deg`, the yaw as the
    map places the model (the same convention as the trees')."""
    seen, lines = set(), []
    for index, world in sorted(world_matrices(doc).items()):
        name = doc["nodes"][index].get("name", "")
        if "mesh" not in doc["nodes"][index] or not BUILDING.search(name):
            continue
        model = name.split(".")[0]
        dota = (world[14] / INCH, world[12] / INCH)
        key = (model, round(dota[0] / 32), round(dota[1] / 32))
        if key in seen:
            continue
        seen.add(key)
        yaw = math.degrees(math.atan2(-world[2], world[0]))
        lines.append(f"{model} {dota[0]:.1f} {dota[1]:.1f} {yaw:.1f}")
    return lines


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("gltf", type=Path)
    parser.add_argument("out", type=Path, help="output .glb; images are copied next to it")
    parser.add_argument("--half-width", type=float, default=2600.0, help="band half-width across the lane, Dota units")
    parser.add_argument("--v-min", type=float, default=-7600.0, help="band start along the lane (Radiant side)")
    parser.add_argument("--v-max", type=float, default=7400.0, help="band end along the lane (Dire side)")
    parser.add_argument("--exclude", default=EXCLUDE)
    parser.add_argument("--layers-out", type=Path, help="write the terrain/water layer .vtex_c paths here")
    parser.add_argument("--trees-out", type=Path, help="write every map tree (model, Dota position, yaw, scale) here")
    parser.add_argument("--buildings-out", type=Path, help="write every building node (model, Dota position, yaw) here")
    args = parser.parse_args()

    doc = json.loads(args.gltf.read_text())
    if len(doc.get("buffers", [])) != 1:
        raise ValueError("expected exactly one buffer")
    blob = (args.gltf.parent / doc["buffers"][0]["uri"]).read_bytes()
    out, packer = crop(doc, blob, args)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    for image in packer.new["images"]:
        if "uri" in image:
            shutil.copyfile(args.gltf.parent / image["uri"], args.out.parent / image["uri"])
    write_glb(args.out, out, packer.out_blob)
    if args.layers_out:
        args.layers_out.write_text("".join(p + "\n" for p in layer_textures(packer.new["materials"])))
    if args.trees_out:
        header = "# model dota_x dota_y height yaw_deg scale (bota = dota + 9216 on x and y)\n"
        args.trees_out.write_text(header + "".join(line + "\n" for line in tree_lines(doc)))
    if args.buildings_out:
        header = "# model dota_x dota_y yaw_deg (bota = dota + 9216 on x and y)\n"
        args.buildings_out.write_text(header + "".join(line + "\n" for line in building_lines(doc)))
    print(f"kept {len(out['nodes'])} nodes, {len(out['meshes'])} meshes, {len(packer.new['materials'])} materials, "
          f"{len(packer.new['images'])} images, {len(packer.out_blob) / 1e6:.1f} MB geometry")


if __name__ == "__main__":
    main()
