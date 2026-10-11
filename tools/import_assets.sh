#!/usr/bin/env bash
# Export the Dota 2 assets listed in assets-manifest/manifest.txt into the dbota cache
# ($DBOTA_ASSETS, default ~/.cache/dbota) with a pinned, checksum-verified Source2Viewer-CLI.
# Valve files are for personal use: they only ever land in the cache, never in a repo.
# Re-running skips every output that already exists (models and the map also compare a stamp
# of their manifest line plus EXPORT_REV). Scratch goes to a mktemp dir under $TMPDIR that is removed on exit.
set -euo pipefail

readonly S2V_VERSION="20.0"
readonly S2V_SHA256="3e8af47cd6ce52e8068904f2aa1dda23c56a6b96a8310b25090f0711cda76a8a"
# Bump when the export flags change so existing models/maps are re-exported.
readonly EXPORT_REV="rev3"
readonly S2V_URL="https://github.com/ValveResourceFormat/ValveResourceFormat/releases/download/${S2V_VERSION}/cli-linux-x64.zip"

readonly TOOLS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly MANIFEST="${TOOLS_DIR}/../assets-manifest/manifest.txt"
readonly CACHE="${DBOTA_ASSETS:-${HOME}/.cache/dbota}"
readonly CLI_DIR="${CACHE}/tools/s2v-${S2V_VERSION}"
readonly CLI="${CLI_DIR}/Source2Viewer-CLI"

DOTA_PATH=""
STAGE=""

usage() {
    cat <<EOF
Usage: $(basename "$0") [--dota-path <"dota 2 beta" dir>]

Exports every asset in assets-manifest/manifest.txt into ${CACHE}
(override with \$DBOTA_ASSETS), then verifies them. Needs curl, unzip, sha256sum, python3.
EOF
}

log() { printf '[import] %s\n' "$*" >&2; }
die() { printf '[import] error: %s\n' "$*" >&2; exit 1; }

parse_args() {
    while (($#)); do
        case "$1" in
            --dota-path) [[ $# -ge 2 ]] || die "--dota-path needs a value"; DOTA_PATH="$2"; shift 2 ;;
            -h|--help) usage; exit 0 ;;
            *) usage >&2; die "unknown argument: $1" ;;
        esac
    done
}

find_dota() {
    local candidates=(
        "${DOTA_PATH}"
        "${HOME}/.var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/common/dota 2 beta"
        "${HOME}/.steam/steam/steamapps/common/dota 2 beta"
        "${HOME}/.local/share/Steam/steamapps/common/dota 2 beta"
    )
    local dir
    for dir in "${candidates[@]}"; do
        if [[ -n "${dir}" && -f "${dir}/game/dota/pak01_dir.vpk" ]]; then
            printf '%s/game/dota' "${dir}"
            return
        fi
    done
    die "Dota 2 not found; pass --dota-path <.../steamapps/common/dota 2 beta>"
}

ensure_cli() {
    if [[ -x "${CLI}" && "$(cat "${CLI_DIR}/.sha256" 2>/dev/null)" == "${S2V_SHA256}" ]]; then
        return
    fi
    log "downloading Source2Viewer-CLI ${S2V_VERSION}"
    local dl="${STAGE}/s2v"
    mkdir -p "${dl}/unpacked"
    curl -fsSL --retry 3 -o "${dl}/cli.zip" "${S2V_URL}"
    printf '%s  %s\n' "${S2V_SHA256}" "${dl}/cli.zip" | sha256sum --check --quiet - \
        || die "Source2Viewer-CLI checksum mismatch"
    unzip -q "${dl}/cli.zip" -d "${dl}/unpacked"
    [[ -f "${dl}/unpacked/Source2Viewer-CLI" ]] || die "archive has no Source2Viewer-CLI"
    chmod +x "${dl}/unpacked/Source2Viewer-CLI"
    printf '%s' "${S2V_SHA256}" > "${dl}/unpacked/.sha256"
    rm -rf "${CLI_DIR}"
    mkdir -p "$(dirname "${CLI_DIR}")"
    mv "${dl}/unpacked" "${CLI_DIR}"
}

# Runs the CLI with its chatter in a log; the CLI exits 0 even when nothing matched,
# so callers check their outputs.
cli() {
    "${CLI}" "$@" < /dev/null >> "${STAGE}/cli.log" 2>&1 || { tail -n 20 "${STAGE}/cli.log" >&2; die "Source2Viewer-CLI failed"; }
}

# Prints manifest lines of one kind without comments.
manifest_lines() {
    sed -e 's/#.*//' "${MANIFEST}" | awk -v kind="$1" '$1 == kind'
}

# Moves a finished staging dir's files into the cache dir of <out>, the main file last
# so its presence means the whole export landed.
install_dir() {
    local staged_dir="$1" out="$2" stamp="$3"
    local dest_dir="${CACHE}/$(dirname "${out}")" main
    main="$(basename "${out}")"
    [[ "$(dirname "${out}")" != "." ]] || die "${out}: models and maps need their own cache subdirectory"
    rm -rf "${dest_dir}"
    mkdir -p "${dest_dir}"
    find "${staged_dir}" -maxdepth 1 -type f ! -name "${main}" -exec mv -t "${dest_dir}" {} +
    printf '%s\n' "${stamp}" > "${dest_dir}/.manifest-line"
    mv "${staged_dir}/${main}" "${dest_dir}/${main}"
}

is_current() {
    local out="$1" stamp="$2"
    [[ -s "${CACHE}/${out}" && "$(cat "${CACHE}/$(dirname "${out}")/.manifest-line" 2>/dev/null)" == "${stamp}" ]]
}

# Value of key=<value> among the manifest line's trailing fields.
line_key() {
    tr ' ' '\n' <<< "$1" | sed -n "s/^$2=//p"
}

export_models() {
    local kind name src out rest staged
    while read -r kind name src out rest; do
        local stamp="${EXPORT_REV} ${kind} ${name} ${src} ${out} ${rest}"
        is_current "${out}" "${stamp}" && continue
        log "model ${name}"
        staged="${STAGE}/model/${name}"
        mkdir -p "${staged}"
        local args=(-i "${GAME}/pak01_dir.vpk" -d -f "${src}" -o "${staged}/$(basename "${out}")"
                    --gltf_export_format glb --gltf_export_materials --gltf_textures_adapt)
        local anims meshes
        anims="$(line_key "${rest}" anims)"
        meshes="$(line_key "${rest}" meshes)"
        if [[ -n "${anims}" ]]; then
            args+=(--gltf_export_animations --gltf_animation_list "${anims}")
        elif [[ -n "$(line_key "${rest}" attach)" ]]; then
            args+=(--gltf_export_animations)  # wearables: only this flag exports their skin
        fi
        [[ -z "${meshes}" ]] || args+=(--gltf_mesh_list "${meshes}")
        cli "${args[@]}"
        [[ -s "${staged}/$(basename "${out}")" ]] || die "no glTF for ${name} (${src})"
        rm -f "${staged}"/*_physics.glb  # collision hulls the client has no use for
        install_dir "${staged}" "${out}" "${stamp}"
    done < <(manifest_lines model)
}

# Exports `<vpk path> <cache-relative out>` pairs from stdin in one CLI run (textures, sounds).
export_files() {
    local label="$1" vpk="$2" src out pairs=() filter=""
    while read -r src out; do
        [[ -s "${CACHE}/${out}" ]] && continue
        pairs+=("${src} ${out}")
        filter+="${filter:+,}${src}"
    done
    ((${#pairs[@]})) || return 0
    log "${label}: ${#pairs[@]} file(s)"
    local staged="${STAGE}/files-${label}"
    # The trailing slash keeps the CLI in folder mode even when only one file matches.
    cli -i "${vpk}" -d -f "${filter}" -o "${staged}/" --threads "$(nproc)"
    local pair produced
    for pair in "${pairs[@]}"; do
        read -r src out <<< "${pair}"
        produced="${staged}/${src%.*}.${out##*.}"
        # Sprite sheets decode to one image per frame plus a .mks listing them; keep the first frame.
        if [[ ! -s "${produced}" && -s "${staged}/${src%.*}.mks" ]]; then
            produced="$(dirname "${produced}")/$(awk '$1 == "frame" { print $2; exit }' "${staged}/${src%.*}.mks")"
        fi
        [[ -s "${produced}" ]] || die "${src} did not decode to .${out##*.}: $(ls "${staged}/${src%.*}".* 2>/dev/null || echo nothing)"
        mkdir -p "$(dirname "${CACHE}/${out}")"
        mv "${produced}" "${CACHE}/${out}"
    done
}

export_world() {
    local kind name src out rest
    while read -r kind name src out rest; do
        local stamp="${EXPORT_REV} ${kind} ${name} ${src} ${out} ${rest}"
        is_current "${out}" "${stamp}" && continue
        log "world ${name} (full map export, then crop)"
        declare -A opt=()
        local kv
        for kv in ${rest}; do opt["${kv%%=*}"]="${kv#*=}"; done
        local raw="${STAGE}/world-raw" staged="${STAGE}/world-${name}"
        cli -i "${GAME}/${opt[vpk]}" -d -f "${src}" -o "${raw}/world.gltf" --gltf_export_format gltf \
            --gltf_export_materials --gltf_textures_adapt --game "${GAME}/gameinfo.gi"
        [[ -s "${raw}/world.gltf" ]] || die "map export produced nothing"
        python3 -B "${TOOLS_DIR}/crop_world.py" "${raw}/world.gltf" "${staged}/$(basename "${out}")" \
            --half-width "${opt[half_width]}" --v-min "${opt[v_min]}" --v-max "${opt[v_max]}" \
            --layers-out "${STAGE}/layers.txt" --trees-out "${staged}/$(basename "${opt[trees]}")" \
            --buildings-out "${staged}/buildings.txt"
        rm -rf "${raw}"
        local layer
        while read -r layer; do
            local base="${layer##*/}"
            printf '%s %s/%s.png\n' "${layer}" "${opt[layers]}" "${base%.vtex_c}"
        done < "${STAGE}/layers.txt" | export_files layers "${GAME}/pak01_dir.vpk"
        install_dir "${staged}" "${out}" "${stamp}"
    done < <(manifest_lines world)
}

main() {
    parse_args "$@"
    local cmd
    for cmd in curl unzip sha256sum python3 nproc; do
        command -v "${cmd}" > /dev/null || die "missing ${cmd}"
    done
    GAME="$(find_dota)"
    readonly GAME
    log "Dota: ${GAME}"
    log "cache: ${CACHE}"
    mkdir -p "${CACHE}"
    STAGE="$(mktemp -d "${TMPDIR:-/tmp}/dbota-import.XXXXXX")"
    trap 'rm -rf "${STAGE}"' EXIT
    ensure_cli
    export_models
    manifest_lines texture | awk '{print $3, $4}' | export_files textures "${GAME}/pak01_dir.vpk"
    manifest_lines sound | awk '{print $3, $4}' | export_files sounds "${GAME}/pak01_dir.vpk"
    export_world
    python3 -B "${TOOLS_DIR}/verify_assets.py" "${MANIFEST}" "${CACHE}" > "${STAGE}/verify.txt" \
        || { cat "${STAGE}/verify.txt" >&2; die "verification failed"; }
    log "verified $(grep -c '^ok' "${STAGE}/verify.txt") assets; cache size $(du -sh "${CACHE}" | cut -f1)"
}

main "$@"
