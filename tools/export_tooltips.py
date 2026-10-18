#!/usr/bin/env python3
"""Builds the tooltip text file of the dbota asset cache from a local Dota 2 install's own files.

Reads the abilities localization, the English interface strings, the item script and the
ability scripts that tools/import_assets.sh extracted from the install, and writes
<out>/text/tooltips.json: for each Dota item and ability that assets-manifest/ids.txt names,
its name, description, lore and notes, the labels of its numbers, its cost, and Dota's own
numbers (the client prints bota's instead). Valve's text lives only in the cache, never in a
repository. Usage: export_tooltips.py <raw dir> <ids.txt> <out file>
"""
import json
import re
import sys
from pathlib import Path

TOKEN = re.compile(r'"((?:[^"\\]|\\.)*)"|([{}])|//[^\n]*|/\*.*?\*/|\s+', re.S)


def parse_kv(text):
    """Valve KV1 text to nested dicts. A repeated key keeps its first value."""
    root, stack, key = {}, [], None
    stack = [root]
    for m in TOKEN.finditer(text):
        s, brace = m.group(1), m.group(2)
        if s is None and brace is None:
            continue
        top = stack[-1]
        if brace == "{":
            child = {}
            if key is not None:
                top.setdefault(key, child)
                child = top[key] if isinstance(top[key], dict) else child
            stack.append(child)
            key = None
        elif brace == "}":
            stack.pop()
        elif key is None:
            key = s
        else:
            top.setdefault(key, s)
            key = None
    return root


def blocks(node, wanted, found):
    """Every block anywhere under node whose key is in wanted, first one wins."""
    for k, v in node.items():
        if not isinstance(v, dict):
            continue
        if k in wanted and k not in found and ("AbilityValues" in v or "ItemCost" in v
                                              or "AbilityBehavior" in v):
            found[k] = v
        blocks(v, wanted, found)


def loc_table(path):
    """The localization file's key/value pairs, keys lower-cased."""
    text = path.read_text(encoding="utf-8-sig")
    table = {}
    for m in re.finditer(r'^\s*"([^"]+)"\s+"((?:[^"\\]|\\.)*)"', text, re.M):
        table.setdefault(m.group(1).lower(), m.group(2).replace('\\"', '"').replace("\\n", "\n"))
    return table


def level_values(v):
    """A script value as the space-separated string Dota prints (special bonuses left out)."""
    if isinstance(v, dict):
        v = v.get("value")
    return v.strip() if isinstance(v, str) else None


def main():
    raw, ids_path, out = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
    loc = loc_table(raw / "resource/localization/abilities_english.txt")
    variables = {k[len("dota_ability_variable_"):]: v for k, v in loc.items()
                 if k.startswith("dota_ability_variable_")}
    scripts = {}
    for f in [raw / "scripts/npc/items.txt", raw / "scripts/npc/npc_abilities.txt",
              *sorted((raw / "scripts/npc/heroes").glob("*.txt"))]:
        scripts[f] = parse_kv(f.read_text(encoding="utf-8-sig"))
    wanted = {}
    for line in ids_path.read_text().splitlines():
        words = line.split("#")[0].split()
        if len(words) >= 3 and words[0] in ("item", "ability") and words[2] != "recipe":
            wanted[words[2] if words[0] == "ability" else "item_" + words[2]] = words[0]
        elif len(words) >= 4 and words[0] == "text":
            wanted[words[3]] = words[1]
    found = {}
    for tree in scripts.values():
        blocks(tree, set(wanted), found)
    result = {}
    for name in sorted(wanted):
        block = found.get(name, {})
        entry = {}
        title = loc.get("dota_tooltip_ability_" + name)
        if title is None:
            continue
        entry["name"] = title
        for field, suffix in (("description", "_description"), ("lore", "_lore")):
            if "dota_tooltip_ability_" + name + suffix in loc:
                entry[field] = loc["dota_tooltip_ability_" + name + suffix]
        notes = []
        while "dota_tooltip_ability_%s_note%d" % (name, len(notes)) in loc:
            notes.append(loc["dota_tooltip_ability_%s_note%d" % (name, len(notes))])
        if notes:
            entry["notes"] = notes
        values = {}
        for key, v in (block.get("AbilityValues") or {}).items():
            s = level_values(v)
            if s is not None:
                values[key] = s
        entry["values"] = values
        for field, key in (("cost", "ItemCost"), ("cooldown", "AbilityCooldown"),
                           ("mana", "AbilityManaCost"), ("range", "AbilityCastRange")):
            s = level_values(block.get(key)) or values.get(key)
            if s is not None:
                entry[field] = s
        labels = []
        for key in values:
            label = loc.get("dota_tooltip_ability_%s_%s" % (name, key.lower()))
            if label:
                label = re.sub(r"\$(\w+)", lambda m: variables.get(m.group(1), m.group(0)), label)
                labels.append([key, label])
        if labels:
            entry["labels"] = labels
        result[name] = entry
    out.parent.mkdir(parents=True, exist_ok=True)
    tmp = out.with_suffix(".tmp")
    tmp.write_text(json.dumps(result, ensure_ascii=False, indent=0, separators=(",", ":")),
                   encoding="utf-8")
    tmp.replace(out)
    missing = sorted(set(wanted) - set(result))
    if missing:
        print("export_tooltips: no text for", ", ".join(missing), file=sys.stderr)


main()
