"""The radovid configuration of the browser checks is the tracked example's, run locally. Runs without a browser."""
import os
import pathlib
import stat
import sys
import tempfile
import tomllib
import unittest
from unittest import mock

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import radovid_service  # noqa: E402

REPLACED = {"--bind", "--ws-port", "--tls-cert", "--tls-key", "--empty-after-secs"}


def pairs(args):
    """The flags with their values: `--cheats` stands alone, `--map 3` has one."""
    found, at = [], 0
    while at < len(args):
        if at + 1 < len(args) and not args[at + 1].startswith("--"):
            found.append((args[at], args[at + 1]))
            at += 2
        else:
            found.append((args[at], None))
            at += 1
    return found


class ServedRulesets(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="radovid-service-")
        self.addCleanup(self.scratch.cleanup)
        self.root = pathlib.Path(self.scratch.name)
        self.server = self.root / "bota_server"
        self.server.write_text("#!/bin/sh\necho abcd1234\n")
        self.server.chmod(self.server.stat().st_mode | stat.S_IXUSR)
        self.example = radovid_service.example_rulesets()[1]
        self.config = tomllib.loads(
            radovid_service.radovid_config(self.root, self.root / "page", self.server, 4460, 20000,
                                           self.root / "cert.pem", self.root / "key.pem", 5555)
        )
        self.served = {ruleset["id"]: ruleset for ruleset in self.config["ruleset"]}

    def test_it_serves_the_demo_and_the_mid_ruleset_of_the_example(self):
        self.assertEqual(sorted(self.served), ["demo", "solo_mid"])
        self.assertEqual(self.config["lobby_ruleset"], "solo_mid")

    def test_the_games_are_played_by_the_examples_arguments(self):
        examples = {ruleset["id"]: ruleset for ruleset in self.example["ruleset"]}
        for name, served in self.served.items():
            with self.subTest(ruleset=name):
                kept = [pair for pair in pairs(served["launcher"]["args"]) if pair[0] not in REPLACED | {"--port", "--idle-timeout-secs"}]
                wanted = [pair for pair in pairs(examples[name]["launcher"]["args"]) if pair[0] not in REPLACED]
                self.assertEqual(kept, wanted, "only the flags that name a place or a timing differ from the example's")

    def test_the_demo_hero_starts_the_map_and_cheats_of_the_example(self):
        example_args = next(r for r in self.example["ruleset"] if r["id"] == "demo")["launcher"]["args"]
        args = self.served["demo"]["launcher"]["args"]
        self.assertEqual(args[args.index("--map") + 1], example_args[example_args.index("--map") + 1])
        self.assertIn("--cheats", args)

    def test_only_the_launcher_kind_and_places_are_replaced(self):
        launcher = self.served["demo"]["launcher"]
        self.assertEqual(launcher["kind"], "local")
        self.assertEqual(launcher["binary"], str(self.server))
        self.assertEqual(launcher["public_url"], "wss://127.0.0.1:5555", "Demo Hero's server is reached through the relay")
        self.assertEqual(self.served["solo_mid"]["launcher"]["public_url"], "wss://127.0.0.1:{port}")
        for key in ("image", "container_port", "mounts"):
            self.assertNotIn(key, launcher)
        self.assertEqual(self.served["demo"]["game_fingerprint"], "abcd1234")

    def test_the_idle_close_is_the_relays_cut_not_a_race_with_a_one_second_timer(self):
        args = self.served["demo"]["launcher"]["args"]
        self.assertEqual(args[args.index("--idle-timeout-secs") + 1], radovid_service.IDLE_TIMEOUT_SECS)
        self.assertGreaterEqual(int(radovid_service.IDLE_TIMEOUT_SECS), 3)

    def test_the_replays_of_the_seated_match_are_kept_where_the_check_looks(self):
        self.assertEqual(self.served["solo_mid"]["launcher"]["artifact"], f"{self.root}/replays/replay-{{match_id}}.brp")

    def test_a_ruleset_missing_from_the_example_fails_the_check(self):
        for missing in radovid_service.RULESETS:
            with self.subTest(missing=missing):
                bota = self.root / f"without-{missing}"
                (bota / "crates/bota_radovid").mkdir(parents=True)
                text = (radovid_service.example_path()).read_text()
                (bota / "crates/bota_radovid/radovid.example.toml").write_text(
                    text.replace(f'id = "{missing}"', f'id = "renamed_{missing}"')
                )
                with mock.patch.dict(os.environ, {"DBOTA_GATE_BOTA": str(bota)}):
                    with self.assertRaises(SystemExit) as stopped:
                        radovid_service.example_rulesets()
                self.assertIn(f"lists no ruleset {missing!r}", str(stopped.exception))

    def test_an_example_without_a_flag_the_check_replaces_fails_by_name(self):
        bota = self.root / "without-flag"
        (bota / "crates/bota_radovid").mkdir(parents=True)
        text = radovid_service.example_path().read_text().replace('"--empty-after-secs", "60"', '"--abandon-unjoined"')
        (bota / "crates/bota_radovid/radovid.example.toml").write_text(text)
        with mock.patch.dict(os.environ, {"DBOTA_GATE_BOTA": str(bota)}):
            with self.assertRaises(SystemExit) as stopped:
                radovid_service.radovid_config(self.root, self.root, self.server, 1, 2, "c", "k", 3)
        self.assertIn("--empty-after-secs", str(stopped.exception))


if __name__ == "__main__":
    unittest.main()
