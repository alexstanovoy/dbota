#!/usr/bin/env bash
# Runs a command that opens dbota windows in a private headless Wayland compositor of its own, so no
# window shows on the desktop while the client still draws on the real GPU (its screenshots are read
# back from its own frames, which no compositor touches).
#
#   tools/headless.sh COMMAND [ARGS...]
#
# The compositor is Weston's headless backend with its GL renderer: one virtual output, started for
# this command and gone with it; nothing of the desktop session is used or changed. Everything the
# command starts (cargo tests that spawn dbota, a server and its clients) shares it.
#
# Environment:
#   DBOTA_SHOW=1               run COMMAND on the desktop instead, windows shown (interactive debugging)
#   DBOTA_HEADLESS_SIZE=WxH    the virtual output's size (default 1920x1080)
#   DBOTA_HEADLESS_SCALE=N     its integer scale (default 1)
#   DBOTA_WESTON=PATH          the weston to run (default: `weston` on PATH; packaged as `weston`)
#   DBOTA_HEADLESS_LOG=FILE    weston's log and what its helpers print (default: discarded)
# Exits with COMMAND's status; 125 when the compositor cannot start.
set -uo pipefail

# Inside the compositor: run the command, keep its status for the outer half.
if [ "${1:-}" = --dbota-headless-inner ]; then
    status_file=$2
    shift 2
    # The command's own output goes where the caller's went (fds 3 and 4), not into weston's log.
    exec 1>&3 2>&4 3>&- 4>&-
    "$@"
    status=$?
    echo "$status" > "$status_file"
    exit "$status"
fi

if [ $# -eq 0 ]; then
    echo "usage: tools/headless.sh COMMAND [ARGS...]" >&2
    exit 2
fi
# Shown on request, and not nested: a command already inside a headless compositor stays in it.
if [ "${DBOTA_SHOW:-0}" = 1 ] || [ -n "${DBOTA_HEADLESS:-}" ]; then
    exec "$@"
fi

weston=${DBOTA_WESTON:-weston}
if ! command -v "$weston" > /dev/null; then
    echo "tools/headless.sh: no weston (install the 'weston' package, or set DBOTA_WESTON); DBOTA_SHOW=1 shows the windows instead" >&2
    exit 125
fi
size=${DBOTA_HEADLESS_SIZE:-1920x1080}
if ! [[ $size =~ ^[1-9][0-9]{1,4}x[1-9][0-9]{1,4}$ ]]; then
    echo "tools/headless.sh: DBOTA_HEADLESS_SIZE '$size' is not WIDTHxHEIGHT" >&2
    exit 2
fi
scale=${DBOTA_HEADLESS_SCALE:-1}
if ! [[ $scale =~ ^[1-4]$ ]]; then
    echo "tools/headless.sh: DBOTA_HEADLESS_SCALE '$scale' is not 1 to 4" >&2
    exit 2
fi

dir=$(mktemp -d "${TMPDIR:-/tmp}/dbota-headless.XXXXXX") || exit 125
trap 'rm -rf -- "$dir"' EXIT
# No panel, no on-screen keyboard, never idle: the output shows the windows and nothing else.
cat > "$dir/weston.ini" <<'EOF'
[core]
idle-time=0
[shell]
panel-position=none
locking=false
[input-method]
path=
EOF
# The compositor ends with the command; if this script is killed, the compositor goes too (and the
# client with it, its display gone), so a killed run leaves nothing behind.
pdeath=()
command -v setpriv > /dev/null && pdeath=(setpriv --pdeathsig TERM --)
socket=dbota-headless-$$
log=${DBOTA_HEADLESS_LOG:-/dev/null}
env -u DISPLAY DBOTA_HEADLESS="$socket" "${pdeath[@]}" "$weston" --backend=headless --renderer=gl \
    --width="${size%x*}" --height="${size#*x}" --scale="$scale" --socket="$socket" \
    --config="$dir/weston.ini" --log="$log" \
    -- "$(realpath -- "${BASH_SOURCE[0]}")" --dbota-headless-inner "$dir/status" "$@" \
    3>&1 4>&2 >> "$log" 2>&1
weston_status=$?
if [ ! -s "$dir/status" ]; then
    echo "tools/headless.sh: the compositor ended (status $weston_status) before the command did" >&2
    exit 125
fi
exit "$(cat "$dir/status")"
