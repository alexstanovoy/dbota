"""Tests of tools/test_groups.py: how files are routed, how commands are joined, what `check` and `run` guarantee.

The guarantees are tested on a toy repository (a git repository holding a tiny cargo package and a groups file) that the tests
build once and edit between cases."""

import argparse
import contextlib
import importlib.util
import io
import os
import shutil
import subprocess
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


def argv_of(commands):
    return [c.argv for c in commands]


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

    def test_also_paths_run_a_group_without_naming_the_file(self):
        reader = group("reader", [], selector("lib"))
        reader.also_paths = ["**.rs"]
        router = test_groups.Router([*self.groups, reader], [])
        self.assertIsNone(router.route("src/new.rs"))
        self.assertEqual(router.route("src/a.rs"), {"alpha", "reader"})

    def test_single_star_does_not_cross_directories(self):
        self.assertIsNone(test_groups.Router([group("g", ["src/*.rs"])], []).route("src/deep/x.rs"))

    def test_a_derived_route_adds_groups_to_a_named_file_only(self):
        router = test_groups.Router(self.groups, [(["docs/**"], [])], {"src/a.rs": {"beta_one"}, "src/unnamed.rs": {"beta_one"}})
        self.assertEqual(router.route("src/a.rs"), {"alpha", "beta_one"})
        self.assertIsNone(router.route("src/unnamed.rs"))


class ImportTests(unittest.TestCase):
    def edges(self, sources):
        files = sorted(sources)
        return test_groups.import_edges(test_groups.crate_trees(files, sources.__getitem__), sources.__getitem__)

    def test_crate_paths_super_paths_and_brace_lists_are_edges(self):
        edges = self.edges({
            "src/main.rs": "mod a; mod b; mod c;",
            "src/a.rs": "pub fn x() {}",
            "src/b/mod.rs": "mod inner; pub fn y() {}",
            "src/c.rs": "use crate::a::x; use crate::{b::y, a};",
            "src/b/inner.rs": "use super::y; use super::super::a::x;",
        })
        self.assertEqual(edges["src/c.rs"], {"src/a.rs", "src/b/mod.rs"})
        self.assertEqual(edges["src/b/inner.rs"], {"src/b/mod.rs", "src/a.rs"})

    def test_a_path_into_an_item_of_a_module_reaches_the_modules_file(self):
        edges = self.edges({"src/lib.rs": "mod a; mod b;", "src/a.rs": "", "src/b.rs": "fn f() { crate::a::Thing::new(); }"})
        self.assertEqual(edges["src/b.rs"], {"src/a.rs"})

    def test_a_path_attribute_names_the_file_of_a_module_wherever_it_is(self):
        edges = self.edges({
            "src/lib.rs": "mod a;",
            "src/a.rs": '#[path = "../tests/a_tests.rs"]\nmod a_tests;\n// #[path = "../nowhere.rs"]\n// mod ghost;',
            "tests/a_tests.rs": "use super::helper;",
            "src/b.rs": "",
        })
        self.assertEqual(edges["tests/a_tests.rs"], {"src/a.rs"})

    def test_a_module_of_a_path_file_is_found_below_that_files_directory(self):
        files = {"src/lib.rs": '#[path = "x/y.rs"]\nmod y;', "src/x/y.rs": "mod z;", "src/x/z.rs": ""}
        tree = test_groups.crate_trees(sorted(files), files.__getitem__)["src/lib.rs"]
        self.assertEqual(tree.walk("src/lib.rs", ["y", "z"]), "src/x/z.rs")

    def test_a_glob_import_is_an_edge_to_the_module_it_globs(self):
        edges = self.edges({
            "src/lib.rs": "mod a; mod m;",
            "src/a.rs": '#[path = "t.rs"]\nmod t;',
            "src/t.rs": "use super::*;",
            "src/m.rs": "use crate::a::*;",
        })
        self.assertEqual(edges["src/t.rs"], {"src/a.rs"})
        self.assertEqual(edges["src/m.rs"], {"src/a.rs"})

    def test_two_crates_do_not_see_each_others_modules(self):
        edges = self.edges({"src/main.rs": "mod a;", "src/a.rs": "use crate::z::x;", "sub/src/lib.rs": "mod z;", "sub/src/z.rs": ""})
        self.assertEqual(edges["src/a.rs"], {"src/main.rs"})  # the item `z` is not a module here: the path reaches the crate root


class ModuleTreeLimitTests(unittest.TestCase):
    """What the module tree cannot read is not guessed: rustc's file list shows it, and check fails on the difference."""

    def listed(self, sources):
        return test_groups.Listed([], set(), test_groups.ROOT / "src/lib.rs", set(sources))

    def tree(self, files):
        return test_groups.crate_trees(sorted(files), files.__getitem__)

    def test_a_module_declared_inside_an_inline_module_is_reported_when_rustc_built_it(self):
        files = {"src/lib.rs": "mod outer { mod inner; }", "src/outer/inner.rs": ""}
        errors = test_groups.check_tree(self.tree(files), self.listed({"src/lib.rs", "src/outer/inner.rs"}), "k lib")
        self.assertEqual(len(errors), 1)
        self.assertIn("rustc built it from src/outer/inner.rs, which the module tree of src/lib.rs does not reach (an inline module", errors[0])

    def test_an_included_file_is_reported_when_rustc_built_it(self):
        files = {"src/lib.rs": 'include!("generated.rs");', "src/generated.rs": ""}
        errors = test_groups.check_tree(self.tree(files), self.listed({"src/lib.rs", "src/generated.rs"}), "k lib")
        self.assertIn("an include!", errors[0])

    def test_a_tree_that_reaches_every_built_file_has_nothing_to_report(self):
        files = {"src/lib.rs": "mod a;", "src/a.rs": ""}
        self.assertEqual(test_groups.check_tree(self.tree(files), self.listed({"src/lib.rs", "src/a.rs"}), "k lib"), [])


class CommandTests(unittest.TestCase):
    config = {"profile": "test-release"}

    def test_groups_sharing_a_target_join_their_filters_into_one_command(self):
        chosen = [group("a", [], selector("bin:k", ["one::"])), group("b", [], selector("bin:k", ["two::", "one::"]))]
        _, commands = test_groups.commands(self.config, chosen)
        self.assertEqual(argv_of(commands), [["cargo", "test", "--profile", "test-release", "-p", "krate", "--bin", "k", "--", "one::", "two::"]])

    def test_a_group_taking_the_whole_target_drops_the_filters(self):
        chosen = [group("a", [], selector("bin:k", ["one::"])), group("b", [], selector("bin:k"))]
        _, commands = test_groups.commands(self.config, chosen)
        self.assertEqual(argv_of(commands), [["cargo", "test", "--profile", "test-release", "-p", "krate", "--bin", "k"]])

    def test_whole_test_targets_of_a_package_share_one_command(self):
        chosen = [group("a", [], selector("test:one"), selector("test:two")), group("b", [], selector("test:three"))]
        _, commands = test_groups.commands(self.config, chosen)
        self.assertEqual(argv_of(commands), [["cargo", "test", "--profile", "test-release", "-p", "krate", "--test", "one", "--test", "two", "--test", "three"]])

    def test_known_baselines_are_skipped_by_name(self):
        _, commands = test_groups.commands(self.config, [group("a", [], selector("lib", ["x::"]))], ["x::slow"])
        self.assertEqual(commands[0].argv[-4:], ["--", "x::", "--skip", "x::slow"])

    def test_two_feature_sets_of_a_package_are_two_commands(self):
        plain = test_groups.Selector("krate", "lib", ("a",), (), (), None, ())
        gpu = test_groups.Selector("krate", "lib", ("b",), ("cuda",), (), None, ())
        _, commands = test_groups.commands(self.config, [group("a", [], plain, gpu)])
        self.assertEqual([c.features for c in commands], [(), ("cuda",)])
        self.assertIn("--features", commands[1].argv)

    def test_python_modules_run_one_command_each_and_their_before_commands_once(self):
        chosen = [group("a", [], selector(unittest_dir="t", modules=["test_x", "test_y"]), before=[["make"]]), group("b", [], selector(unittest_dir="t", modules=["test_x"]), before=[["make"]])]
        before, commands = test_groups.commands(self.config, chosen)
        self.assertEqual(before, [["make"]])
        self.assertEqual([c.argv[-1] for c in commands], ["test_x.py", "test_y.py"])


class BuildTests(unittest.TestCase):
    config = {"features": ["builtin"]}
    groups = [group("a", ["src/a.rs"], test_groups.Selector("k", "lib", (), ("builtin",), (), None, ())),
              group("gpu", ["src/gpu.rs"], test_groups.Selector("k", "lib", (), ("builtin", "cuda"), (), None, ()))]

    def router(self):
        return test_groups.Router(self.groups, [(["docs/**"], [])])

    def wanted(self, files):
        return test_groups.wanted_builds(self.config, self.groups, self.router(), files)

    def test_the_whole_repository_is_checked_in_every_build_the_groups_use(self):
        self.assertEqual(self.wanted(None), [("builtin",), ("builtin", "cuda")])

    def test_a_change_no_cuda_group_or_feature_text_reaches_checks_the_plain_build_only(self):
        self.assertEqual(self.wanted(["docs/x.md"]), [("builtin",)])

    def test_a_change_that_runs_the_cuda_group_checks_the_cuda_build(self):
        self.assertEqual(self.wanted(["src/gpu.rs"]), [("builtin",), ("builtin", "cuda")])


class OutcomeTests(unittest.TestCase):
    def plan(self, filters=None):
        return test_groups.Planned(["cargo", "test"], "krate", (), ("lib",), filters)

    def listings(self):
        listed = test_groups.Listed(["a::one", "a::two", "b::three"], {"b::three"}, Path("src/lib.rs"))
        return {((), "krate", "lib"): listed}

    def log(self, passed, failed=0, ignored=0, measured=0):
        return f"test result: ok. {passed} passed; {failed} failed; {ignored} ignored; {measured} measured; 0 filtered out\n"

    def test_a_run_of_exactly_the_selected_tests_passes(self):
        self.assertIsNone(test_groups.problem_of(self.plan(), 0, self.log(2, ignored=1), self.listings(), [], {}))

    def test_a_run_of_fewer_tests_than_selected_is_a_problem(self):
        problem = test_groups.problem_of(self.plan(), 0, self.log(1, ignored=1), self.listings(), [], {})
        self.assertIn("it ran 1 tests and reported 1 ignored where its groups select 2 and 1 ignored", problem)

    def test_a_skipped_waived_test_is_not_counted_as_selected(self):
        self.assertIsNone(test_groups.problem_of(self.plan(), 0, self.log(1, ignored=1), self.listings(), ["a::two"], {}))

    def test_filters_narrow_the_selection(self):
        self.assertIsNone(test_groups.problem_of(self.plan(("a::",)), 0, self.log(2), self.listings(), [], {}))

    def test_measured_tests_are_a_problem(self):
        self.assertIn("measured", test_groups.problem_of(self.plan(), 0, self.log(2, ignored=1, measured=1), self.listings(), [], {}))

    def test_a_failing_exit_status_is_a_problem(self):
        self.assertIn("failed: cargo test", test_groups.problem_of(self.plan(), 101, "", self.listings(), [], {}))

    def test_python_tests_that_skip_themselves_are_a_problem(self):
        plan = test_groups.Planned(["python3"], unit=("t", "test_x"))
        problem = test_groups.problem_of(plan, 0, "Ran 2 tests in 0.1s\n\nOK (skipped=1)\n", {}, [], {"t/test_x": 2})
        self.assertIn("skipped themselves", problem)

    def test_python_tests_are_counted_like_the_listing(self):
        plan = test_groups.Planned(["python3"], unit=("t", "test_x"))
        self.assertIsNone(test_groups.problem_of(plan, 0, "Ran 2 tests in 0.1s\n\nOK\n", {}, [], {"t/test_x": 2}))
        self.assertIn("ran 1 tests where its group selects 2", test_groups.problem_of(plan, 0, "Ran 1 test in 0.1s\n\nOK\n", {}, [], {"t/test_x": 2}))

    def test_failed_tests_are_read_from_libtests_report_once_each_and_sorted(self):
        log = "running 2 tests\n---- b::second stdout ----\npanicked\n---- a::first stdout ----\n---- b::second stdout ----\n"
        self.assertEqual(test_groups.failed_tests(log), ["a::first", "b::second"])

    def test_a_baseline_that_is_part_of_another_tests_name_is_refused(self):
        errors = test_groups.check_skips({"a::one": "r"}, ["a::one", "a::one_more"])
        self.assertEqual(errors, ["baseline a::one is a part of other tests' names, so skipping it would skip them too: name it fully"])

    def test_a_baseline_that_is_no_test_is_refused(self):
        self.assertIn("is no test", test_groups.check_skips({"a::gone": "r"}, ["a::one"])[0])


class BaselineFileTests(unittest.TestCase):
    def read(self, text):
        with tempfile.TemporaryDirectory() as directory:
            Path(directory, "b.txt").write_text(text)
            original, test_groups.ROOT = test_groups.ROOT, Path(directory)
            try:
                return test_groups.baselines({"baselines": "b.txt"})
            finally:
                test_groups.ROOT = original

    def test_names_and_reasons_are_read_and_comments_skipped(self):
        found = self.read("# a comment\n\nmod::test_one # fails since abc (owner: me; remove when: fixed)\n")
        self.assertEqual(found, {"mod::test_one": "fails since abc (owner: me; remove when: fixed)"})

    def test_a_baseline_without_a_reason_is_refused(self):
        with self.assertRaises(test_groups.Failure) as raised:
            self.read("mod::test_one\n")
        self.assertIn("expected `exact::test::name # reason (owner: WHO; remove when: CONDITION)`", str(raised.exception))

    def test_a_baseline_without_an_owner_and_a_removal_condition_is_refused(self):
        for line in ("mod::test_one # it fails\n", "mod::test_one # it fails (owner: me)\n", "mod::test_one # it fails (remove when: fixed)\n"):
            with self.assertRaises(test_groups.Failure, msg=line):
                self.read(line)


TOY_BASELINE = "c::failing # fails on main (owner: nobody; remove when: the toy is fixed)\n"

TOY_TOML = """
profile = "test-release"
baselines = "known_baselines.txt"

[[route]]
paths = ["Cargo.toml", "Cargo.lock", ".gitignore", "src/lib.rs"]
groups = ["*"]

[[route]]
paths = ["known_baselines.txt"]
groups = []

[[group]]
name = "first"
paths = ["src/a.rs", "tools/test_groups.toml"]
tests = [{ package = "toy", target = "lib", filters = ["a::"] }]

[[group]]
name = "second"
paths = ["src/b.rs"]
tests = [{ package = "toy", target = "lib", filters = ["b::"] }]

[[group]]
name = "third"
paths = ["src/c.rs", "src/other/e_tests.rs"]
tests = [{ package = "toy", target = "lib", filters = ["c::", "e::"] }]

[[group]]
name = "fourth"
paths = ["src/d.rs"]
tests = [{ package = "toy", target = "lib", features = ["extra"], filters = ["d::"] }]
"""

TOY_FILES = {
    "Cargo.toml": '[package]\nname = "toy"\nversion = "0.1.0"\nedition = "2021"\n[workspace]\n[features]\nextra = []\n[profile.test-release]\ninherits = "release"\n',
    ".gitignore": "target/\n",
    "src/lib.rs": "#![allow(dead_code, unused_imports)]\nmod a;\nmod b;\nmod c;\n#[cfg(feature = \"extra\")]\nmod d;\n#[path = \"other/e_tests.rs\"]\nmod e;\n",
    "src/d.rs": "#[test]\nfn only_with_the_feature() {}\n",
    "src/other/e_tests.rs": "#[test]\nfn pathed() {}\n",
    "src/a.rs": "pub fn helper() {}\n#[test]\nfn one() {}\n#[test]\n#[ignore]\nfn two() {}\n",
    "src/b.rs": "use crate::a::helper;\n#[test]\nfn uses_a() { helper(); }\n",
    "src/c.rs": "#[test]\nfn failing() { assert_eq!(1, 2, \"toy failure\"); }\n",
    "known_baselines.txt": "",
}


class ToyRepositoryTests(unittest.TestCase):
    """`check` and `run` on a git repository of a toy cargo package, built once."""

    @classmethod
    def setUpClass(cls):
        scratch = Path(os.environ.get("CARGO_TARGET_DIR") or test_groups.ROOT / "target") / "test-groups-toy"
        scratch.mkdir(parents=True, exist_ok=True)
        cls.directory = Path(tempfile.mkdtemp(prefix="repo-", dir=scratch))
        cls.original = (test_groups.ROOT, test_groups.CONFIG)
        cls.target = os.environ.get("CARGO_TARGET_DIR")
        os.environ["CARGO_TARGET_DIR"] = str(cls.directory / "target")
        test_groups.ROOT = cls.directory
        test_groups.CONFIG = cls.directory / "tools/test_groups.toml"
        for name, text in TOY_FILES.items():
            cls.write(name, text)
        cls.write("tools/test_groups.toml", TOY_TOML)
        subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=cls.directory, check=True, capture_output=True)
        for command in (["git", "init", "-q", "-b", "main"], ["git", "add", "-A"], ["git", "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "toy"]):
            subprocess.run(command, cwd=cls.directory, check=True, capture_output=True)

    @classmethod
    def tearDownClass(cls):
        test_groups.ROOT, test_groups.CONFIG = cls.original
        if cls.target is None:
            del os.environ["CARGO_TARGET_DIR"]
        else:
            os.environ["CARGO_TARGET_DIR"] = cls.target
        shutil.rmtree(cls.directory, ignore_errors=True)

    @classmethod
    def write(cls, name, text):
        path = cls.directory / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def setUp(self):
        subprocess.run(["git", "checkout", "-q", "--", "."], cwd=self.directory, check=True)
        subprocess.run(["git", "clean", "-qfd"], cwd=self.directory, check=True)

    def arguments(self, *change, base=None, record=None, no_waive=False, all_=False, skip_feature=()):
        return argparse.Namespace(all=all_, base=base, change=list(change), no_waive=no_waive, record=record, skip_feature=list(skip_feature))

    def check(self, toml=None, base=None, skip_feature=()):
        if toml is not None:
            self.write("tools/test_groups.toml", toml)
        errors, output = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
            status = test_groups.cmd_check(self.arguments(base=base, skip_feature=skip_feature))
        return status, errors.getvalue(), output.getvalue()

    def run_groups(self, *change, record=None, no_waive=False, all_=False):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            status = test_groups.cmd_run(self.arguments(*change, record=record, no_waive=no_waive, all_=all_))
        return status, output.getvalue()

    def select(self, *change):
        output = io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(io.StringIO()):
            test_groups.cmd_select(self.arguments(*change))
        return output.getvalue().splitlines()[0]

    def test_check_passes_when_every_test_is_in_one_group_and_every_file_is_routed(self):
        status, errors, _ = self.check()
        self.assertEqual((status, errors), (0, ""))

    def test_check_names_a_test_in_no_group(self):
        status, errors, _ = self.check(TOY_TOML.replace('filters = ["b::"]', 'filters = ["b::uses_aa"]'))
        self.assertEqual(status, 1)
        self.assertIn("in no group: toy lib b::uses_a", errors)

    def test_check_names_a_test_in_two_groups(self):
        status, errors, _ = self.check(TOY_TOML.replace('filters = ["b::"]', 'filters = ["b::", "a::one"]'))
        self.assertEqual(status, 1)
        self.assertIn("in 2 groups (first, second) of one build: toy lib a::one", errors)

    def test_check_names_a_filter_that_selects_no_test(self):
        status, errors, _ = self.check(TOY_TOML.replace('"e::"]', '"e::", "gone::"]'))
        self.assertEqual(status, 1)
        self.assertIn("group third: filter 'gone::' selects no test", errors)

    def test_check_names_a_file_nothing_routes(self):
        self.write("docs/notes.txt", "x")
        status, errors, _ = self.check()
        self.assertEqual(status, 1)
        self.assertIn("no group or route names docs/notes.txt", errors)

    def test_check_names_a_path_pattern_that_matches_no_file(self):
        status, errors, _ = self.check(TOY_TOML.replace('"src/other/e_tests.rs"]\ntests = [{ package = "toy", target = "lib", filters = ["c::"', '"src/other/e_tests.rs", "src/gone.rs"]\ntests = [{ package = "toy", target = "lib", filters = ["c::"'))
        self.assertEqual(status, 1)
        self.assertIn("group third: path pattern 'src/gone.rs' matches no file", errors)

    def test_check_names_a_test_file_missing_from_its_groups_paths(self):
        status, errors, _ = self.check(TOY_TOML.replace('paths = ["src/c.rs", "src/other/e_tests.rs"]', 'paths = ["src/other/e_tests.rs"]').replace('"src/lib.rs"]', '"src/lib.rs", "src/c.rs"]'))
        self.assertEqual(status, 1)
        self.assertIn("src/c.rs, which is not in the `paths` of its group third", errors)

    def test_check_names_a_baseline_that_is_no_test(self):
        self.write("known_baselines.txt", "a::gone # fixed long ago (owner: nobody; remove when: never)\n")
        status, errors, _ = self.check()
        self.assertEqual(status, 1)
        self.assertIn("baseline a::gone is no test", errors)

    def test_check_skips_a_feature_build_it_is_told_to_and_says_so(self):
        status, errors, output = self.check(skip_feature=["extra"])
        self.assertEqual((status, errors), (0, ""))
        self.assertIn("SKIPPED the builds with feature(s) extra (extra)", output)

    def test_check_names_a_test_of_the_feature_build_in_no_group(self):
        status, errors, _ = self.check(TOY_TOML.replace('filters = ["d::"]', 'filters = ["none::"]'))
        self.assertEqual(status, 1)
        self.assertIn("in no group: toy lib with extra d::only_with_the_feature", errors)

    def test_a_test_in_a_path_module_is_written_in_the_file_the_path_names(self):
        status, errors, _ = self.check(TOY_TOML.replace('paths = ["src/c.rs", "src/other/e_tests.rs"]', 'paths = ["src/c.rs"]').replace('"src/lib.rs"]', '"src/lib.rs", "src/other/e_tests.rs"]'))
        self.assertEqual(status, 1)
        self.assertIn("src/other/e_tests.rs, which is not in the `paths` of its group third", errors)

    def test_the_module_tree_must_reach_every_file_rustc_built_a_binary_from(self):
        files = sorted(test_groups.every_file())
        trees = test_groups.crate_trees(files, lambda path: (self.directory / path).read_text())
        listed = test_groups.Listed([], set(), self.directory / "src/lib.rs", {"src/lib.rs", "src/other/e_tests.rs", "src/stray.rs"})
        errors = test_groups.check_tree(trees, listed, "toy lib")
        self.assertEqual(len(errors), 1)
        self.assertIn("rustc built it from src/stray.rs, which the module tree of src/lib.rs does not reach", errors[0])

    def test_a_change_to_a_file_runs_the_groups_of_the_tests_importing_it(self):
        self.assertEqual(self.select("src/a.rs"), "# groups: first second")

    def test_a_change_to_a_file_nothing_imports_runs_its_own_group_only(self):
        self.assertEqual(self.select("src/b.rs"), "# groups: second")

    def test_a_script_a_test_file_names_runs_that_files_group(self):
        self.write("src/b.rs", TOY_FILES["src/b.rs"] + '#[test]\nfn runs_the_script() { let _ = "tools/seed.sh"; }\n')
        self.write("tools/seed.sh", "#!/bin/sh\n")
        self.write("tools/test_groups.toml", TOY_TOML + '\n[[route]]\npaths = ["tools/seed.sh"]\ngroups = []\n')
        self.assertEqual(self.select("tools/seed.sh"), "# groups: second")

    def test_an_edit_of_a_global_key_of_the_toml_runs_every_group(self):
        self.write("tools/test_groups.toml", TOY_TOML.replace('profile = "test-release"', 'profile = "test-release"\ncargo_flags = ["--quiet"]'))
        groups = test_groups.parse_config(__import__("tomllib").loads(TOY_TOML))[0]
        self.assertEqual(test_groups.config_delta(groups, "HEAD"), {"first", "second", "third", "fourth"})

    def test_an_edit_of_one_group_of_the_toml_runs_that_group(self):
        self.write("tools/test_groups.toml", TOY_TOML.replace('filters = ["a::"]', 'filters = ["a::one"]'))
        groups = test_groups.parse_config(__import__("tomllib").loads(TOY_TOML))[0]
        self.assertEqual(test_groups.config_delta(groups, "HEAD"), {"first"})

    def test_run_stops_at_a_failing_test_and_keeps_its_name_for_next_time(self):
        record = self.directory / "failed.txt"
        status, output = self.run_groups("src/c.rs", record=record)
        self.assertEqual(status, 1)
        self.assertIn("TEST-GROUPS-FAIL failed: cargo test", output)
        self.assertEqual(record.read_text(), "c::failing\n")

    def test_run_goes_green_over_a_waived_baseline_and_reports_it(self):
        self.write("known_baselines.txt", TOY_BASELINE)
        status, output = self.run_groups(all_=True)
        self.assertEqual(status, 0, output[-1500:])
        self.assertIn("waived baseline: c::failing -- fails on main (owner: nobody; remove when: the toy is fixed)", output)

    def test_run_skips_a_waived_baseline_when_the_change_does_not_reach_its_group(self):
        self.write("known_baselines.txt", TOY_BASELINE)
        status, output = self.run_groups("src/b.rs")
        self.assertEqual(status, 0, output)

    def test_run_runs_a_waived_baseline_when_a_changed_file_is_routed_to_its_group(self):
        self.write("known_baselines.txt", TOY_BASELINE)
        status, output = self.run_groups("src/lib.rs")
        self.assertEqual(status, 1)
        self.assertIn("so they run: c::failing", output)

    def test_editing_the_baselines_file_alone_does_not_unwaive_what_it_waives(self):
        self.write("known_baselines.txt", TOY_BASELINE)
        status, output = self.run_groups("known_baselines.txt")
        self.assertEqual(status, 0, output)

    def test_run_runs_a_waived_baseline_when_asked_to(self):
        self.write("known_baselines.txt", TOY_BASELINE)
        self.assertEqual(self.run_groups(all_=True, no_waive=True)[0], 1)

    def test_run_of_the_passing_groups_counts_the_ignored_test_and_succeeds(self):
        status, output = self.run_groups("src/a.rs")
        self.assertEqual(status, 0, output)
        self.assertIn("1 ignored", output)


if __name__ == "__main__":
    unittest.main()
