#!/usr/bin/env bash
# What a Linux machine can prove about the Windows client before a person runs it on Windows:
#
#   1. it compiles for Windows (x86_64-pc-windows-msvc) and clippy has nothing to say about the
#      Windows-only code (the registry, the folder dialog, the message box, the console);
#   2. with cargo-xwin (which downloads Microsoft's C runtime and Windows SDK libraries and
#      accepts their license for you), it links the release binary (`--profile dist`);
#   3. that binary is a windowed program (no console window opens with it) and imports no C
#      runtime DLL (vcruntime, msvcp, ucrtbase, api-ms-win-crt): it needs no Visual C++
#      redistributable, and no file beside it;
#   4. with Wine, it starts in a fresh Wine prefix (a fresh profile, no cache, no settings) and
#      draws its first frame: the import screen.
#
# Each step needs a tool this script does not install; a missing one ends the script with exit
# code 125 and the name of what to install, after the steps that could run. What stays unproven
# without a real Windows machine: DirectX 12 on a real driver, the system's folder dialog and
# message box as a person sees them, the registry of a real Steam install.
#
# Usage: tools/windows_check.sh [OUT_DIR]   (default: temp/windows-check)
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=tools/client_env.sh
. "$here/tools/client_env.sh"
out=${1:-$here/temp/windows-check}
mkdir -p "$out"
target=x86_64-pc-windows-msvc
cd "$here"

missing() {
    echo "windows check: STOP $1" >&2
    exit 125
}

if ! rustup target list --installed 2> /dev/null | grep -qx "$target"; then
    missing "no Rust standard library for $target: rustup target add $target"
fi

# 1. Compiles. `cargo clippy` runs build scripts but links nothing, so the C code some crates
# carry (blake3, ring) is stood in for by empty objects: no Windows C toolchain is needed here.
stand_in=$out/cc.sh
cat > "$stand_in" << 'EOF'
#!/bin/bash
out=
prev=
for arg in "$@"; do
    # The compiler's family is asked with a preprocessor run on a file of macros only.
    [ "$arg" = "-E" ] && exec clang --target=x86_64-pc-windows-msvc "$@"
    case "$arg" in
        /Fo*|-Fo*) out=${arg:3} ;;
    esac
    [ "$prev" = "-o" ] && out=$arg
    prev=$arg
done
[ -n "$out" ] || { echo "cc stand-in: no output in: $*" >&2; exit 1; }
exec clang --target=x86_64-pc-windows-msvc -c -x c /dev/null -o "$out"
EOF
chmod +x "$stand_in"
command -v clang > /dev/null || missing "no clang (Debian: clang) for the check's stand-in objects"
lib=$(command -v llvm-lib || ls /usr/bin/llvm-lib-* 2> /dev/null | sort -V | tail -1 || true)
[ -n "$lib" ] || missing "no llvm-lib (Debian: llvm) for the check's stand-in archives"
# No compiler cache: it does not know the stand-in.
RUSTC_WRAPPER= CC_x86_64_pc_windows_msvc=$stand_in AR_x86_64_pc_windows_msvc=$lib \
    CARGO_TARGET_DIR=$out/check-target \
    cargo clippy --release --target "$target" -p dbota -- -D warnings
echo "windows check: PASS the client compiles for Windows, clippy clean"

# 2. Links, as a release would.
command -v cargo-xwin > /dev/null ||
    missing "no cargo-xwin to link the Windows binary (cargo install cargo-xwin; it downloads the MSVC CRT and the Windows SDK and accepts Microsoft's license)"
CARGO_TARGET_DIR=$out/target cargo xwin build --profile dist --target "$target" -p dbota
exe=$out/target/$target/dist/dbota.exe
echo "windows check: PASS $exe links"

# 3. A windowed program that needs no C runtime beside it.
readobj=$(command -v llvm-readobj || ls /usr/bin/llvm-readobj-* 2> /dev/null | sort -V | tail -1 || true)
[ -n "$readobj" ] || missing "no llvm-readobj (Debian: llvm)"
headers=$("$readobj" --file-headers "$exe")
grep -q "Subsystem: IMAGE_SUBSYSTEM_WINDOWS_GUI" <<< "$headers" ||
    { echo "windows check: FAIL $exe is not a windowed program (a console opens with it)" >&2; exit 1; }
imports=$("$readobj" --coff-imports "$exe" | sed -n 's/^ *Name: //p' | sort -u)
echo "$imports" > "$out/imports.txt"
if grep -Eiq '^(vcruntime|msvcp|ucrtbase|api-ms-win-crt)' <<< "$imports"; then
    echo "windows check: FAIL $exe needs the Visual C++ runtime:" >&2
    grep -Ei '^(vcruntime|msvcp|ucrtbase|api-ms-win-crt)' <<< "$imports" >&2
    exit 1
fi
echo "windows check: PASS windowed, no C runtime DLL (imports in $out/imports.txt)"

# 4. Starts in a fresh profile and draws the import screen.
command -v wine > /dev/null || missing "no Wine to start the binary (Debian: wine wine64)"
prefix=$out/wine-prefix
rm -rf -- "$prefix"
shots=$out/shots
mkdir -p "$shots"
WINEPREFIX=$prefix WINEDEBUG=-all in_private_environment timeout 600 wine "$exe" --display-mode windowed --mute \
    --screenshot-dir "$(winepath -w "$shots" 2> /dev/null || echo "$shots")" \
    --script "await:text:Welcome to dbota;shot:first-frame.png;click:quit dbota" \
    > "$out/wine.log" 2>&1 || { echo "windows check: FAIL the client did not start under Wine (see $out/wine.log)" >&2; exit 1; }
[ -s "$shots/first-frame.png" ] ||
    { echo "windows check: FAIL no first frame of the import screen (see $out/wine.log)" >&2; exit 1; }
echo "windows check: PASS a fresh profile starts at the import screen ($shots/first-frame.png)"
