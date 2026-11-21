"""The radovid configuration the browser checks serve: the tracked example's rulesets, run on this machine.

Kept apart from the page checks so that it can be tested without a browser (test_radovid_service.py)."""

import os
import pathlib
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent


# The rulesets of the example radovid configuration that the page check serves: Demo Hero (the idle reason
# is drawn when the server stops hearing the client) and the 1x1 mid game (a seated match, kept by the
# service for the replay steps). Their game arguments are the example's, so the check plays what the
# example, and so the stand, plays; a missing ruleset fails the check instead of being invented here.
RULESETS = ("demo", "solo_mid")
# What only the check's timing needs, not a rule of the game: a match ends five seconds after its player
# is gone, and the game server closes a link that has said nothing for five seconds, which only the
# relay's deliberate cut can cause on a loaded machine.
EMPTY_AFTER_SECS = "5"
IDLE_TIMEOUT_SECS = "5"


def example_path():
    """The tracked example configuration: in DBOTA_GATE_BOTA or the bota checkout beside this one."""
    bota = pathlib.Path(os.environ.get("DBOTA_GATE_BOTA") or HERE.parent.parent.parent / "bota")
    return bota / "crates/bota_radovid/radovid.example.toml"


def example_rulesets():
    """The example's rulesets, by id (and the whole example)."""
    import tomllib

    path = example_path()
    example = tomllib.loads(path.read_text(encoding="utf-8"))
    found = {ruleset["id"]: ruleset for ruleset in example["ruleset"]}
    for wanted in RULESETS:
        if wanted not in found:
            sys.exit(f"{path} lists no ruleset {wanted!r}: the replays check serves {RULESETS} and takes them from it")
    return {wanted: found[wanted] for wanted in RULESETS}, example


def set_flag(args, flag, value):
    """`args` with the value of `flag` replaced (or the flag added when `add`): the example's arguments must hold it."""
    if flag not in args:
        sys.exit(f"the example's launcher arguments have no {flag}: the replays check replaces its value")
    args = list(args)
    args[args.index(flag) + 1] = value
    return args


def toml_value(value):
    import json

    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return str(value)
    if isinstance(value, str):
        return json.dumps(value)
    if isinstance(value, list):
        return "[" + ", ".join(toml_value(item) for item in value) + "]"
    if isinstance(value, dict):
        return "{ " + ", ".join(f"{key} = {toml_value(item)}" for key, item in value.items()) + " }"
    raise TypeError(f"{value!r} has no TOML form here")


def local_ruleset(ruleset, server_bin, fingerprint, cert, key, directory, public_url, extra_args):
    """The example's ruleset as this machine runs it: only the launcher's kind, binary, URL, ports and
    certificate paths (and the replay's folder) are replaced; the rest is the example's, flag for flag."""
    launcher = dict(ruleset["launcher"])
    for container_key in ("image", "container_port", "mounts"):
        launcher.pop(container_key, None)
    args = list(launcher["args"])
    for flag, value in (
        ("--bind", "127.0.0.1"), ("--ws-port", "{port}"), ("--tls-cert", str(cert)), ("--tls-key", str(key)),
        ("--empty-after-secs", EMPTY_AFTER_SECS),
    ):
        args = set_flag(args, flag, value)
    launcher.update(kind="local", binary=str(server_bin), public_url=public_url, args=[*args, "--port", "0", *extra_args])
    if "artifact" in launcher:
        launcher["artifact"] = f"{directory}/replays/replay-{{match_id}}.brp"
    head = {key: value for key, value in ruleset.items() if key not in ("launcher", "game_fingerprint")}
    lines = ["[[ruleset]]", *(f"{key} = {toml_value(value)}" for key, value in head.items()),
             f"game_fingerprint = {toml_value(fingerprint)}", "[ruleset.launcher]",
             *(f"{key} = {toml_value(value)}" for key, value in launcher.items())]
    return "\n".join(lines)


def radovid_config(directory, page_dir, server_bin, port, game_port, cert, key, relay_port):
    """A radovid with the example's Demo Hero and 1x1 mid rulesets run locally, serving `page_dir` on its
    own origin. Demo Hero's game server is reached through the relay (`relay_port`) that cuts the client's
    replies on purpose; the 1x1 game's directly."""
    fingerprint = subprocess.run([server_bin, "--wire-fingerprint"], capture_output=True, text=True).stdout.strip()
    rulesets, example = example_rulesets()
    served = {
        "demo": (f"wss://127.0.0.1:{relay_port}", ["--idle-timeout-secs", IDLE_TIMEOUT_SECS]),
        "solo_mid": (f"wss://127.0.0.1:{{port}}", []),
    }
    parts = [
        f'''listen = "127.0.0.1:{port}"
database = "{directory}/radovid.db"
tls_cert = "{cert}"
tls_key = "{key}"
game_ports = "{game_port}-{game_port}"
lobby_teams = {toml_value(example["lobby_teams"])}
lobby_ruleset = "solo_mid"
[web]
static_dir = "{page_dir}"
[password]
memory_kib = 1024
iterations = 1
[artifacts]
save_replays = true
[limits]
game_capacity = 1
lobby_games_max = 1''',
    ]
    for ruleset_id, ruleset in rulesets.items():
        url, extra = served[ruleset_id]
        parts.append(local_ruleset(ruleset, server_bin, fingerprint, cert, key, directory, url, extra))
    return "\n".join(parts) + "\n"
