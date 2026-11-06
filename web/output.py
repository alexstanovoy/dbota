#!/usr/bin/env python3
"""Own the build output explicitly before replacing any of its assets."""
import os
import pathlib
import shutil
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUTPUT_MARKER = ".dbota-web-output"
ASSETS_MARKER = ".dbota-web-assets"
OWNER = b"dbota web build 1\n"
MAX_ENTRIES = 16384
MAX_DEPTH = 12


def owned(path):
    if path.is_symlink() or not path.is_file():
        return False
    with path.open("rb") as source:
        return source.read(len(OWNER) + 1) == OWNER


def plain_tree(root, exclusive=False):
    pending = [(root, 0)]
    count = 0
    while pending:
        directory, depth = pending.pop()
        with os.scandir(directory) as entries:
            for entry in entries:
                count += 1
                if count > MAX_ENTRIES or depth > MAX_DEPTH:
                    raise ValueError("web output tree exceeds its limits")
                if entry.is_symlink():
                    raise ValueError("unowned web output: symbolic link " + entry.path)
                if entry.is_dir(follow_symlinks=False):
                    pending.append((pathlib.Path(entry.path), depth + 1))
                elif not entry.is_file(follow_symlinks=False):
                    raise ValueError("unowned web output: special file " + entry.path)
                elif exclusive and entry.stat(follow_symlinks=False).st_nlink != 1:
                    raise ValueError("unowned web output: multiply linked file " + entry.path)


def prepare(destination):
    output = pathlib.Path(destination).resolve()
    assets = ROOT / "assets"
    if output == assets or assets in output.parents or output in assets.parents or output == ROOT / "web":
        raise ValueError("unsafe web output: destination overlaps build sources")
    if output.exists():
        if not output.is_dir():
            raise ValueError("unowned web output: destination is not a directory")
        if not owned(output / OUTPUT_MARKER) and any(output.iterdir()):
            raise ValueError("unowned web output: use an empty directory or an owned build output")
        plain_tree(output, exclusive=True)
    else:
        output.mkdir(parents=True)
    marker = output / OUTPUT_MARKER
    if not marker.exists():
        with marker.open("xb") as target:
            target.write(OWNER)
    if not owned(marker):
        raise ValueError("unowned web output: invalid ownership marker")
    return output


def copy_assets(destination):
    output = prepare(destination)
    source = ROOT / "assets"
    plain_tree(source)
    target = output / "assets"
    if target.exists() and (not target.is_dir() or not owned(target / ASSETS_MARKER)):
        raise ValueError("unowned web output: assets have no build ownership marker")
    with tempfile.TemporaryDirectory(prefix=".dbota-assets-", dir=output) as scratch:
        staged = pathlib.Path(scratch) / "assets"
        shutil.copytree(source, staged)
        (staged / ASSETS_MARKER).write_bytes(OWNER)
        if target.exists():
            shutil.rmtree(target)
        staged.rename(target)


def main():
    try:
        operation, destination = sys.argv[1:]
        if operation == "prepare":
            print(prepare(destination))
        elif operation == "assets":
            copy_assets(destination)
        else:
            raise ValueError("unknown output operation")
    except (OSError, ValueError) as error:
        sys.exit(f"web build: {error}")


if __name__ == "__main__":
    main()
