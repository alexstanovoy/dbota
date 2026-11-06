#!/usr/bin/env bash
# The fast check to run while working, before tools/gate.sh: it stops at the first thing that is wrong.
#
#   tools/precheck.sh [--full] [BASE]   BASE: the branch the work is compared with (default: main); --full: every test group
#
# Formatting is no part of it: it changes no code, and is checked last, by the gate.
# Steps, each only for what the work touches (the files changed since the merge-base with BASE, committed or not):
#   clippy  the crates with a changed file and every crate of the repository that depends on them
#   test-groups  every test in exactly one group and every file routed (tools/test_groups.py check; the gate's check too)
#   test    the test groups the change affects (tools/test_groups.py select shows the commands); the tests that failed in
#           the last run go first, and the run stops at the first failure. --full or PRECHECK_ALL=1 runs every group
#   wasm    the browser build checks, only when a crate they cover changed
# A warm checkout takes a few minutes at most for a typical change. The gate stays the confirmation: a failure there that
# this would have caught is a process bug. PRECHECK_ALL=1 takes every crate. Failed test names are kept in
# $(git rev-parse --git-path precheck-failed) and cleared by a green run. A green run stamps the tree (tools/stamp.sh), and
# tools/gate.sh starts only on a stamped tree.
set -uo pipefail
cd -- "$(git rev-parse --show-toplevel)" || exit 2
# shellcheck source=tools/stamp.sh
. "$(dirname -- "${BASH_SOURCE[0]}")/stamp.sh"
# shellcheck source=tools/client_env.sh
. "$(dirname -- "${BASH_SOURCE[0]}")/client_env.sh"
# Windows on the desktop, or a compositor of an outer run, would take the tests off their private home.
unset DBOTA_SHOW DBOTA_HEADLESS
full=${PRECHECK_ALL:-0}
if [ "${1:-}" = --full ]; then full=1; shift; fi
base=${1:-${PRECHECK_BASE:-main}}
if [ "$full" = 1 ]; then selection=(--all); check_selection=(); else selection=(--base "$base"); check_selection=(--base "$base"); fi
# the browser check (the one client of tools/gate.sh that is quickest), when a source of the client changed
wasm_crates="dbota"
wasm_checks=(
    "DBOTA_WEB_REPLAY=$PWD/Cargo.toml cargo check --target wasm32-unknown-unknown --profile web -p dbota --features web-replay,webgpu"
)
extra_clippy() { :; }
extra_test() { :; }
# the tests that start the client read an asset cache of this build; the gate makes it once per stamp, and so does this
key=$( cat assets-manifest/manifest.txt assets-manifest/ids.txt
       grep -h 'pub const IMPORT_REV' crates/dbota_import/src/import.rs
       grep -h 'pub const OUTPUT_REV' ../heatel/src/lib.rs 2>/dev/null || true )
key=$(printf '%s' "$key" | sha256sum | cut -c1-16)
export DBOTA_ASSETS=${DBOTA_ASSETS:-${DBOTA_GATE_ASSETS:-${XDG_CACHE_HOME:-$HOME/.cache}/dbota-gate}/$key}

refuse_managed_asset_cache || exit 1
record=$(git rev-parse --git-path precheck-failed)
started=$SECONDS
step() { local name=$1 t=$SECONDS; shift; echo "== $name"; "$@" || { echo "PRECHECK-FAIL $name ($((SECONDS - t)) s)"; return 1; }; echo "   $name $((SECONDS - t)) s"; }

mb=$(git merge-base HEAD "$base" 2>/dev/null || git rev-parse HEAD)
changed=$( { git diff --name-only "$mb"; git ls-files --others --exclude-standard; } | sort -u)
plan_out=$(PRECHECK_ALL=$full CHANGED="$changed" python3 - <<'PY'
import json, os, subprocess
meta = json.loads(subprocess.run(["cargo", "metadata", "--format-version", "1", "--offline"], capture_output=True, text=True, check=True).stdout)
root = meta["workspace_root"].rstrip("/") + "/"
ids = set(meta["workspace_members"])
pkgs = {p["id"]: p for p in meta["packages"] if p["id"] in ids}
dirs = {i: os.path.relpath(os.path.dirname(p["manifest_path"]), meta["workspace_root"]).replace(".", "", 1) if os.path.dirname(p["manifest_path"]) == meta["workspace_root"] else os.path.relpath(os.path.dirname(p["manifest_path"]), meta["workspace_root"]) for i, p in pkgs.items()}
name = {i: p["name"] for i, p in pkgs.items()}
hit = set()
files = [f for f in os.environ["CHANGED"].split("\n") if f]
everything = os.environ["PRECHECK_ALL"] == "1" or any(f in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "deny.toml") or f.startswith(".cargo/") for f in files)
if everything:
    hit = set(pkgs)
else:
    for f in files:
        if f.endswith(".md") or f.startswith(("tools/", "docs/", "fuzz/")):
            continue
        best = max((i for i in pkgs if dirs[i] == "" or f == dirs[i] or f.startswith(dirs[i] + "/")), key=lambda i: len(dirs[i]), default=None)
        if best: hit.add(best)
# reverse dependents among the crates of the repository
deps = {i: set() for i in pkgs}
for n in meta["resolve"]["nodes"]:
    if n["id"] in pkgs:
        deps[n["id"]] = {d["pkg"] for d in n["deps"] if d["pkg"] in pkgs}
todo = list(hit)
while todo:
    cur = todo.pop()
    for i, ds in deps.items():
        if cur in ds and i not in hit:
            hit.add(i); todo.append(i)
print(" ".join(sorted(name[i] for i in hit)))
PY
)
plan=$plan_out
groups=$(python3 tools/test_groups.py select "${selection[@]}" | sed -n 's/^# groups: //p') || { echo "PRECHECK-FAIL select" >&2; exit 2; }
if [ -z "$plan" ] && [ "$groups" = none ]; then
    echo "precheck: no crate and no test group touched"
    stamp_put "$(tree_key)" precheck
    echo "PRECHECK-OK ($((SECONDS - started)) s)"
    exit 0
fi
echo "precheck: crates ${plan:-none}; test groups $groups"
pflags=(); for c in $plan; do pflags+=(-p "$c"); done

[ -z "$plan" ] || step clippy cargo clippy "${pflags[@]}" --all-targets -- -D warnings || exit 1
for c in $plan; do
    e=$(extra_clippy "$c")
    [ -z "$e" ] || step "clippy ($c)" bash -c "$e" || exit 1
done

run_tests() { # the failed ones first, then the groups the change affects
    python3 tools/test_groups.py run --record "$record" "${selection[@]}"
}
assets() {
    mkdir -p "$(dirname -- "$DBOTA_ASSETS")"
    (flock 9 && import_assets) 9> "$DBOTA_ASSETS.lock" > /dev/null 2>&1 || echo "   no asset cache (no usable install): the tests that need it fail"
    return 0
}
[ -d "$DBOTA_ASSETS" ] || step assets assets
step test-groups python3 tools/test_groups.py check "${check_selection[@]}" || exit 1
step test in_private_environment run_tests || exit 1
for c in $plan; do
    e=$(extra_test "$c"); [ -z "$e" ] || step "test ($c)" in_private_environment bash -c "$e" || exit 1
done

for c in $plan; do
    case " $wasm_crates " in *" $c "*)
        for w in "${wasm_checks[@]}"; do step wasm bash -c "$w" || exit 1; done
        break ;;
    esac
done
stamp_put "$(tree_key)" precheck
echo "PRECHECK-OK ($((SECONDS - started)) s)"
