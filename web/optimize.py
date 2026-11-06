#!/usr/bin/env python3
"""Optionally optimize generated Wasm with the supported Binaryen release."""
import hashlib
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tempfile

VERSION = 125
# Official version_125 x86_64 Linux executable, verified before it is run.
SHA256 = "23c1f4106e34eeaf44e888f947671f04d0fec12c232cca19cc5c4b3cf7c96f08"
FEATURES = [
    "bulk-memory", "nontrapping-float-to-int", "sign-ext", "multivalue",
    "reference-types", "mutable-globals",
]


def optimizer():
    requested = os.environ.get("WASM_OPT")
    if requested == "0":
        return None
    if not requested:
        raise ValueError("Set WASM_OPT=0 or WASM_OPT=/path/to/the/verified/wasm-opt")
    return str(Path(requested).resolve(strict=True))


def verify(command):
    with Path(command).open("rb") as executable:
        digest = hashlib.file_digest(executable, "sha256").hexdigest()
    if digest != SHA256:
        raise ValueError("wasm-opt checksum mismatch; see web/README.md for the supported executable")


def optimize(path, command):
    verify(command)
    version = subprocess.run(
        [command, "--version"], check=True, capture_output=True, text=True, timeout=10,
    ).stdout
    match = re.match(r"wasm-opt version (\d+)\b", version)
    if match is None or int(match[1]) != VERSION:
        raise ValueError(f"web optimization requires Binaryen {VERSION}; found {version.strip()!r}")
    descriptor, temporary = tempfile.mkstemp(prefix=".wasm-opt-", suffix=".wasm", dir=path.parent)
    os.close(descriptor)
    staged = Path(temporary)
    try:
        subprocess.run(
            [command, str(path), "-Oz", "--strip-debug", "--strip-producers",
             *(f"--enable-{feature}" for feature in FEATURES), "-o", str(staged)],
            check=True, capture_output=True, text=True, timeout=900,
        )
        with staged.open("rb") as output:
            if output.read(8) != b"\0asm\1\0\0\0":
                raise ValueError("wasm-opt produced invalid WebAssembly")
        staged.chmod(stat.S_IMODE(path.stat().st_mode))
        staged.replace(path)
    finally:
        staged.unlink(missing_ok=True)


def main():
    command = optimizer()
    if command is not None:
        verify(command)
        for filename in sys.argv[1:]:
            optimize(Path(filename), command)


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        sys.exit(f"web optimization failed: {error.stderr or error}")
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        sys.exit(f"web optimization failed: {error}")
