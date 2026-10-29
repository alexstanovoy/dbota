#!/usr/bin/env bash
# Builds the page into web/out: the client (dbota.wasm), the importer module and the page's
# scripts. Needs wasm32-unknown-unknown and wasm-bindgen-cli at the version of Cargo.lock.
#   DBOTA_WEB_REPLAY=match.brp web/build.sh     # a page that plays the replay built into it
#   PROFILE=release web/build.sh                # a quicker, bigger build for tests (default: web)
#   OUT=temp/page web/build.sh                  # somewhere else than web/out
set -euo pipefail
export PATH="$PATH:$HOME/.cargo/bin"
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$here"
profile=${PROFILE:-web}
out=${OUT:-web/out}
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
# One client per graphics API (Bevy fixes its render paths when it is compiled); the page loads the
# one the browser can draw with: dbota.js (WebGPU) or dbota_webgl2.js.
for api in webgpu webgl2; do
    cargo build --profile "$profile" --target wasm32-unknown-unknown --features "$api" "${features[@]}" -p dbota
    name=dbota; [ "$api" = webgpu ] || name=dbota_$api
    wasm-bindgen --target web --no-typescript --out-dir "$out" --out-name "$name" \
        "target/wasm32-unknown-unknown/$profile/dbota.wasm"
done
cargo build --profile "$profile" --target wasm32-unknown-unknown -p dbota_import --features web
wasm-bindgen --target web --no-typescript --out-dir "$out/import" "target/wasm32-unknown-unknown/$profile/dbota_import.wasm"
cp web/index.html web/page.css web/main.js web/grant.js web/keys.js web/pointer.js web/cache.js web/import_worker.js web/report.js "$out/"
ln -sfn "$here/assets" "$out/assets"
echo "built $out"
