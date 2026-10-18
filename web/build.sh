#!/usr/bin/env bash
# Builds the page into web/out: the client (dbota.wasm), the importer module and the page's
# scripts. Needs wasm32-unknown-unknown and wasm-bindgen-cli at the version of Cargo.lock.
#   DBOTA_WEB_REPLAY=match.brp web/build.sh     # a page that plays the replay built into it
#   PROFILE=release web/build.sh                # a quicker, bigger build for tests (default: web)
set -euo pipefail
export PATH="$PATH:$HOME/.cargo/bin"
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$here"
profile=${PROFILE:-web}
out=web/out
features=()
[ -n "${DBOTA_WEB_REPLAY:-}" ] && features=(--features web-replay)
export DBOTA_WEB_ARGS=${DBOTA_WEB_ARGS:---replay built-in --follow 0}
cargo build --profile "$profile" --target wasm32-unknown-unknown "${features[@]}" -p dbota
cargo build --profile "$profile" --target wasm32-unknown-unknown -p dbota_import
mkdir -p "$out/import"
wasm-bindgen --target web --no-typescript --out-dir "$out" "target/wasm32-unknown-unknown/$profile/dbota.wasm"
wasm-bindgen --target web --no-typescript --out-dir "$out/import" "target/wasm32-unknown-unknown/$profile/dbota_import.wasm"
cp web/index.html web/page.css web/main.js web/grant.js web/cache.js web/import_worker.js web/report.js "$out/"
ln -sfn "$here/assets" "$out/assets"
echo "built $out"
