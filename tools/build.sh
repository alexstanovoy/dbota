#!/usr/bin/env bash
# Builds the desktop client and the game server it starts, side by side, so that LOCAL LOBBY
# finds a server of the same bota and the same wire: target/release/dbota and
# target/release/bota_server (`PROFILE=dist` builds both in `target/dist`).
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
profile=${PROFILE:-release}
bota=${DBOTA_GATE_BOTA:-$here/../bota}
server_args=()
if [ "$profile" = dev_release ]; then
    server_args=(--config 'profile.dev_release.inherits="release"')
fi
(cd -- "$here" && cargo build --locked --profile "$profile" --manifest-path Cargo.toml)
# The server is bota's own binary, built into this target folder with the WebSocket listener a
# service launches it with (a local lobby needs only the plain one).
(cd -- "$bota" && cargo build --locked --profile "$profile" --manifest-path Cargo.toml \
    -p bota_server --features websocket --target-dir "$here/target" "${server_args[@]}")
echo "built $here/target/$profile/dbota and $here/target/$profile/bota_server"
