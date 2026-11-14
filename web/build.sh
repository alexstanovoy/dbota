#!/usr/bin/env bash
# Builds the page into web/out: the client (dbota.wasm), the importer module and the page's
# scripts. Needs wasm32-unknown-unknown and wasm-bindgen-cli at the version of Cargo.lock.
#   WASM_OPT=0 DBOTA_WEB_REPLAY=match.brp web/build.sh     # a page that plays the replay built into it
#   PROFILE=release web/build.sh                # a quicker, bigger build for tests (default: web)
#   WASM_OPT=0 OUT=temp/page web/build.sh                  # somewhere else than web/out
#   WASM_OPT=0 APIS=webgpu web/build.sh                    # only that graphics API's client (default: "webgpu webgl2")
#   WASM_OPT=/path/to/wasm-opt web/build.sh      # Binaryen 125 (optional; see web/README.md)
#   WASM_OPT=0 web/build.sh                     # skip Binaryen
set -euo pipefail
export PATH="$PATH:$HOME/.cargo/bin"
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$here"
profile=${PROFILE:-web}
# Validate the explicit shipping policy before spending time compiling.
if [ "$profile" = web ] || [ -n "${WASM_OPT:-}" ]; then
    python3 web/optimize.py
fi
out=$(python3 web/output.py prepare "${OUT:-web/out}")
target=${CARGO_TARGET_DIR:-$here/target}
features=()
[ -n "${DBOTA_WEB_REPLAY:-}" ] && features=(--features web-replay)
export DBOTA_WEB_ARGS=${DBOTA_WEB_ARGS:---replay built-in --follow 0}
# The paths compiled into the modules (the places panics and log lines name) say nothing of the
# machine they were built on: the registry, the toolchain and the checkouts' folder get short names.
remap=("$HOME=~" "${CARGO_HOME:-$HOME/.cargo}/registry/src=crates" "$(rustc --print sysroot)=rust"
    "$(dirname -- "$here")=.")
CARGO_ENCODED_RUSTFLAGS=$(printf -- '--remap-path-prefix=%s\x1f' "${remap[@]}")
export CARGO_ENCODED_RUSTFLAGS=${CARGO_ENCODED_RUSTFLAGS%$'\x1f'}
mkdir -p "$out/import"
optimize() {
    # Other profiles are for quick checks; an explicit WASM_OPT also enables their optimization.
    if [ "$profile" = web ] || [ -n "${WASM_OPT:-}" ]; then
        python3 web/optimize.py "$1"
    fi
}
# One client per graphics API (Bevy fixes its render paths when it is compiled); the page loads the
# one the browser can draw with: dbota.js (WebGPU) or dbota_webgl2.js.
for api in ${APIS:-webgpu webgl2}; do
    cargo build --locked --profile "$profile" --target-dir "$target" --target wasm32-unknown-unknown --features "$api" "${features[@]}" -p dbota
    name=dbota; [ "$api" = webgpu ] || name=dbota_$api
    wasm-bindgen --target web --no-typescript --out-dir "$out" --out-name "$name" \
        "$target/wasm32-unknown-unknown/$profile/dbota.wasm"
    optimize "$out/${name}_bg.wasm"
done
cargo build --locked --profile "$profile" --target-dir "$target" --target wasm32-unknown-unknown -p dbota_import_web
wasm-bindgen --target web --no-typescript --out-dir "$out/import" --out-name dbota_import \
    "$target/wasm32-unknown-unknown/$profile/dbota_import_web.wasm"
optimize "$out/import/dbota_import_bg.wasm"
cp web/index.html web/page.css web/boot.js web/busy.js web/audio.js web/main.js web/native.js web/grant.js web/keys.js web/pointer.js web/cache.js web/import_worker.js web/replays.js web/report.js web/voice.js web/voice_worker.js "$out/"
# The icons' licence asks for the credit to travel with them: the page links this file.
cp THIRD-PARTY-NOTICES.md "$out/"
# A copy, not a link: a service serving the folder does not follow a link out of it, and the
# page would have no font to draw its words in.
python3 web/output.py assets "$out"
echo "built $out"
