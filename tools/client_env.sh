# Sourced by the tools that start the real client or run the tests that do (test_client.sh,
# bench.sh, headless.sh, precheck.sh, gate.sh). The client reads and writes its settings, replays
# (the replay library prunes the oldest files beyond its limits) and asset cache under the user's
# HOME and XDG folders, so none of these tools may run it, or the tests that start it, as they are.
#
#   private_home_environment HOME_DIR GPU_CACHE_DIR
#       HOME and every XDG/Windows data folder under HOME_DIR, the shader cache in GPU_CACHE_DIR
#       (the compiled shaders are worth keeping between runs). What the next commands still need
#       from the real home is named before it is hidden: the Rust toolchain (CARGO_HOME,
#       RUSTUP_HOME), the compiler cache (SCCACHE_DIR), the browsers of Playwright and the Dota
#       install (DBOTA_DOTA_PATH, found in the usual Steam places, read only). The caller names and
#       removes HOME_DIR.
#   private_client_environment HOME_DIR GPU_CACHE_DIR
#       The above, and none of the variables in client_environment.txt: for the tools that start
#       the client themselves.
#   in_private_environment COMMAND...
#       COMMAND with the private home (only that: a test below it may set a steering variable on
#       purpose, which is the harness's business) made under the target folder and removed after;
#       its status is the command's. A test that starts the client without the harness's wrappers
#       still cannot reach the user's files.
#   refuse_managed_path VARIABLE / refuse_managed_asset_cache
#       Fails when the folder VARIABLE names (DBOTA_ASSETS) is, or leads through a link or dots
#       into, the managed ~/.cache/dbota, whose maintenance prunes the caches it owns and where
#       a run writes use stamps.
#   require_test_asset_cache
#       That, and fails unless DBOTA_ASSETS names an asset cache with an index.
#   import_assets
#       `dbota import` into DBOTA_ASSETS (refused when that is managed), on a private home, the
#       Dota install given by DBOTA_DOTA_PATH.
#   run_scrubbed COMMAND...
#       COMMAND without the variables in client_environment.txt: for the game servers the tools
#       start (a seat list or referee key in the caller's shell must not decide who sits). To
#       start one in the background, whose pid `$!` must be the server's, use
#       `steering_unset_arguments; env "${STEERING_UNSET[@]}" COMMAND... &`: env execs the command.
#   client_binary [PROFILE]
#       The built client: $DBOTA_BIN, else <target dir>/<PROFILE, default release>/dbota, where the
#       target dir is $CARGO_TARGET_DIR, else this checkout's target.
_client_env_here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)

private_home_environment() {
    local home=$1 gpu_cache=$2 library real_home=$HOME
    export CARGO_HOME="${CARGO_HOME:-$real_home/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$real_home/.rustup}"
    export SCCACHE_DIR="${SCCACHE_DIR:-${XDG_CACHE_HOME:-$real_home/.cache}/sccache}"
    export PLAYWRIGHT_BROWSERS_PATH="${PLAYWRIGHT_BROWSERS_PATH:-${XDG_CACHE_HOME:-$real_home/.cache}/ms-playwright}"
    if [ -z "${DBOTA_DOTA_PATH:-}" ]; then
        for library in .var/app/com.valvesoftware.Steam/.local/share/Steam .steam/steam .local/share/Steam; do
            if [ -f "$real_home/$library/steamapps/common/dota 2 beta/game/dota/pak01_dir.vpk" ]; then
                export DBOTA_DOTA_PATH="$real_home/$library/steamapps/common/dota 2 beta"
                break
            fi
        done
    fi
    export HOME="$home" XDG_CONFIG_HOME="$home/config" XDG_DATA_HOME="$home/data" \
        XDG_STATE_HOME="$home/state" LOCALAPPDATA="$home/data" APPDATA="$home/data" \
        XDG_CACHE_HOME="$gpu_cache"
}

_scrub_steering_variables() {
    local name
    while read -r name; do
        case $name in '' | '#'*) ;; *) unset "$name" ;; esac
    done < "$_client_env_here/tools/client_environment.txt"
}

private_client_environment() {
    _scrub_steering_variables
    private_home_environment "$@"
}

in_private_environment() {
    (
        target=${CARGO_TARGET_DIR:-$_client_env_here/target}
        mkdir -p "$target"
        scratch=$(mktemp -d "$target/private-environment.XXXXXX") || exit 1
        trap 'rm -r -- "$scratch"' EXIT
        private_home_environment "$scratch" "$target/tmp/gpu-cache"
        "$@"
    )
}

refuse_managed_path() {
    local variable=$1 path
    for path in "${!variable:-}" "$(realpath -m -- "${!variable:-/}")"; do
        case "/$path/" in
            */.cache/dbota/*)
                echo "$variable names the managed asset cache ${!variable}: use a private folder" >&2
                return 1 ;;
        esac
    done
}

refuse_managed_asset_cache() {
    refuse_managed_path DBOTA_ASSETS
}

require_test_asset_cache() {
    : "${DBOTA_ASSETS:?DBOTA_ASSETS must name a test asset cache}"
    refuse_managed_asset_cache || return 1
    [ -f "$DBOTA_ASSETS/index.txt" ] || { echo "test asset cache has no index: $DBOTA_ASSETS" >&2; return 1; }
}

import_assets() {
    : "${DBOTA_ASSETS:?DBOTA_ASSETS must name where to import}"
    refuse_managed_asset_cache || return 1
    in_private_environment bash -c 'exec cargo run --release -q -- import ${DBOTA_DOTA_PATH:+--dota-path "$DBOTA_DOTA_PATH"}'
}

steering_unset_arguments() {
    local name
    STEERING_UNSET=()
    while read -r name; do
        case $name in '' | '#'*) ;; *) STEERING_UNSET+=(-u "$name") ;; esac
    done < "$_client_env_here/tools/client_environment.txt"
}

run_scrubbed() {
    steering_unset_arguments
    env "${STEERING_UNSET[@]}" "$@"
}

client_binary() {
    echo "${DBOTA_BIN:-${CARGO_TARGET_DIR:-$_client_env_here/target}/${1:-release}/dbota}"
}
