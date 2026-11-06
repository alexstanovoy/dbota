"""Tests of tools/test_groups.py: how files are routed to groups, how commands are joined and how baselines are read."""

import importlib.util
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("test_groups", Path(__file__).with_name("test_groups.py"))
test_groups = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(test_groups)


def selector(target=None, filters=(), package="krate", unittest_dir=None, modules=()):
    return test_groups.Selector(None if unittest_dir else package, target, tuple(filters), (), (), unittest_dir, tuple(modules))


def group(name, paths, *selectors, before=()):
    return test_groups.Group(name, list(paths), list(selectors), [list(c) for c in before])


class RoutingTests(unittest.TestCase):
    def setUp(self):
        self.groups = [group("alpha", ["src/a.rs", "src/a/**"]), group("beta_one", ["src/b.rs"]), group("beta_two", ["src/c.rs"])]
        routes = [(["Cargo.toml"], ["*"]), (["**.md"], []), (["src/shared.rs"], ["beta_*"])]
        self.router = test_groups.Router(self.groups, routes)

    def test_a_file_a_group_names_runs_that_group(self):
        self.assertEqual(self.router.route("src/a/deep/x.rs"), {"alpha"})

    def test_a_star_route_runs_every_group(self):
        self.assertEqual(self.router.route("Cargo.toml"), {"alpha", "beta_one", "beta_two"})

    def test_a_prefix_route_runs_the_groups_of_that_prefix(self):
        self.assertEqual(self.router.route("src/shared.rs"), {"beta_one", "beta_two"})

    def test_a_document_runs_no_group(self):
        self.assertEqual(self.router.route("docs/x.md"), set())

    def test_a_file_nothing_names_is_unknown_and_runs_every_group(self):
        self.assertIsNone(self.router.route("src/new.rs"))
        chosen, unnamed = test_groups.affected(self.router, self.groups, ["src/a.rs", "src/new.rs"])
        self.assertEqual([g.name for g in chosen], ["alpha", "beta_one", "beta_two"])
        self.assertEqual(unnamed, ["src/new.rs"])

    def test_single_star_does_not_cross_directories(self):
        self.assertIsNone(test_groups.Router([group("g", ["src/*.rs"])], []).route("src/deep/x.rs"))


class CommandTests(unittest.TestCase):
    config = {"profile": "test-release"}

    def test_groups_sharing_a_target_join_their_filters_into_one_command(self):
        chosen = [group("a", [], selector("bin:k", ["one::"])), group("b", [], selector("bin:k", ["two::", "one::"]))]
        _, commands = test_groups.commands(self.config, chosen)
        self.assertEqual(commands, [["cargo", "test", "--profile", "test-release", "-p", "krate", "--bin", "k", "--", "one::", "two::"]])

    def test_a_group_taking_the_whole_target_drops_the_filters(self):
        chosen = [group("a", [], selector("bin:k", ["one::"])), group("b", [], selector("bin:k"))]
        _, commands = test_groups.commands(self.config, chosen)
        self.assertEqual(commands, [["cargo", "test", "--profile", "test-release", "-p", "krate", "--bin", "k"]])

    def test_whole_test_targets_of_a_package_share_one_command(self):
        chosen = [group("a", [], selector("test:one"), selector("test:two")), group("b", [], selector("test:three"))]
        _, commands = test_groups.commands(self.config, chosen)
        self.assertEqual(commands, [["cargo", "test", "--profile", "test-release", "-p", "krate", "--test", "one", "--test", "two", "--test", "three"]])

    def test_known_baselines_are_skipped_by_name(self):
        _, commands = test_groups.commands(self.config, [group("a", [], selector("lib", ["x::"]))], ["x::slow"])
        self.assertEqual(commands[0][-4:], ["--", "x::", "--skip", "x::slow"])

    def test_python_modules_run_one_command_each_and_their_before_commands_once(self):
        chosen = [group("a", [], selector(unittest_dir="t", modules=["test_x", "test_y"]), before=[["make"]]), group("b", [], selector(unittest_dir="t", modules=["test_x"]), before=[["make"]])]
        before, commands = test_groups.commands(self.config, chosen)
        self.assertEqual(before, [["make"]])
        self.assertEqual([c[-1] for c in commands], ["test_x.py", "test_y.py"])


class FailureTests(unittest.TestCase):
    def test_failed_tests_are_read_from_libtests_report_once_each_and_sorted(self):
        log = "running 2 tests\n---- b::second stdout ----\npanicked\n---- a::first stdout ----\n---- b::second stdout ----\n"
        self.assertEqual(test_groups.failed_tests(log), ["a::first", "b::second"])


class BaselineTests(unittest.TestCase):
    def read(self, text):
        with tempfile.TemporaryDirectory() as directory:
            Path(directory, "b.txt").write_text(text)
            original, test_groups.ROOT = test_groups.ROOT, Path(directory)
            try:
                return test_groups.baselines({"baselines": "b.txt"})
            finally:
                test_groups.ROOT = original

    def test_names_and_reasons_are_read_and_comments_skipped(self):
        found = self.read("# a comment\n\nmod::test_one # fails on main since abc\n")
        self.assertEqual(found, {"mod::test_one": "fails on main since abc"})

    def test_a_baseline_without_a_reason_is_refused(self):
        with self.assertRaises(test_groups.Failure) as raised:
            self.read("mod::test_one\n")
        self.assertIn("expected `exact::test::name # reason`", str(raised.exception))


if __name__ == "__main__":
    unittest.main()
