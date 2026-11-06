# Sourced by the tools that start the real client (test_client.sh, bench.sh). The client reads and
# writes its settings, replays (the replay library prunes the oldest files beyond its limits) and
# asset cache under the user's HOME and XDG folders, so none of these tools may run it as it is.
#
#   private_client_environment HOME_DIR GPU_CACHE_DIR
#       HOME and every XDG/Windows data folder under HOME_DIR, the shader cache in GPU_CACHE_DIR
#       (the compiled shaders are worth keeping between runs), and none of the variables in
#       client_environment.txt. The caller names and removes HOME_DIR.
#   client_binary [PROFILE]
#       The built client: $DBOTA_BIN, else <target dir>/<PROFILE, default release>/dbota, where the
#       target dir is $CARGO_TARGET_DIR, else this checkout's target.
_client_env_here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)

private_client_environment() {
    local home=$1 gpu_cache=$2 name
    while read -r name; do
        case $name in '' | '#'*) ;; *) unset "$name" ;; esac
    done < "$_client_env_here/tools/client_environment.txt"
    export HOME="$home" XDG_CONFIG_HOME="$home/config" XDG_DATA_HOME="$home/data" \
        XDG_STATE_HOME="$home/state" LOCALAPPDATA="$home/data" APPDATA="$home/data" \
        XDG_CACHE_HOME="$gpu_cache"
}

client_binary() {
    echo "${DBOTA_BIN:-${CARGO_TARGET_DIR:-$_client_env_here/target}/${1:-release}/dbota}"
}
