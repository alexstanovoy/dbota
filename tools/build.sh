#!/usr/bin/env bash
# Builds the desktop client and the game server it starts, side by side, so that LOCAL LOBBY
# finds a server of the same bota and the same wire: target/release/dbota and
# target/release/bota_server (`PROFILE=dist` builds both in `target/dist`).
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
profile=${PROFILE:-release}
cargo build --profile "$profile" --manifest-path "$here/Cargo.toml"
# The server is bota's own binary, built into this target folder with the WebSocket listener a
# service launches it with (a local lobby needs only the plain one).
cargo build --profile "$profile" --manifest-path "$here/../bota/Cargo.toml" \
    -p bota_server --features websocket --target-dir "$here/target"
echo "built $here/target/$profile/dbota and $here/target/$profile/bota_server"
