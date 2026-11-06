#!/usr/bin/env python3
"""The test groups of this repository (tools/test_groups.toml): which tests a change needs, and the proof that none is left out.

    tools/test_groups.py check [--base REV | --all]       every test in exactly one group, every file routed
    tools/test_groups.py select [--all | --base REV | A..B | FILE...]
                                                          the groups the change affects and the commands that run them
    tools/test_groups.py run [--record FILE] [selection]  the same commands, run one after the other, stopping at the first failure

A change is the files differing from the merge-base of HEAD with REV (committed, uncommitted and new files), the files between
two revisions `A..B`, or the files named. Which groups a file runs is what the toml says (group `paths`, `[[route]]`) plus what
the code says: a change to a Rust file runs the groups of the test files that import it directly (`crate::`, `super::`,
`self::` paths), and a change to a script a test file names runs that test's group. A file nothing names runs every group, and
a change to the toml runs the groups it edited (every group when a global key moved).

`check` builds the tests (every feature set the groups use when the whole repository is checked; with `--base`, the others
only when the change can matter to them), asks each test binary for its tests (libtest's `--list`, doc tests and Python
unittest ids too) and fails on a test in no group or in two, a group without a test, a filter or a path pattern that matches
nothing, a file nothing routes, and a test file that is not in its group's `paths` or whose imports or scripts would not run
its group when changed. `run` checks that every command ran exactly the tests its groups select.

Known failures of main (the file `baselines` names: exact test names, each with its reason) are skipped by `run` and listed as
waived, so one of them does not hide the rest of the suite; `--no-waive` runs them, and a changed file that holds a waived
test runs it too. Any other failure stops the run. Tests marked `#[ignore]` are grouped like the others and reported as
ignored: the gate's own steps run them.
"""

import argparse
import json
import re
import shlex
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True).stdout.strip())
CONFIG = Path(__file__).with_suffix(".toml")
LIBRARY_KINDS = ("lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro")
GLOBAL_TABLES = ("group", "route", "outside")


class Failure(Exception):
    """A problem to report to the user, one per line."""


@dataclass(frozen=True)
class Selector:
    package: str | None  # None: a Python suite
    target: str | None  # "lib", "bin:NAME", "test:NAME", "doc"; None: every target of the package
    filters: tuple[str, ...]  # libtest's rule: a test runs when its name contains any of them; empty: all
    features: tuple[str, ...]
    env: tuple[tuple[str, str], ...]
    unittest: str | None  # a directory whose test_*.py files are the suite
    modules: tuple[str, ...]  # the suite's files (without .py)


@dataclass
class Group:
    name: str
    paths: list[str]
    selectors: list[Selector]
    before: list[list[str]]


@dataclass
class Listed:
    """The tests of one test target: its names, the ones marked ignored, and the file it starts at."""

    tests: list[str]
    ignored: set[str]
    root: Path


@dataclass
class Planned:
    """One command of a run, and what it is meant to run."""

    argv: list[str]
    package: str | None = None
    features: tuple[str, ...] = ()
    targets: tuple[str, ...] = ()  # "" is every target of the package
    filters: tuple[str, ...] | None = None  # None: the whole target
    unit: tuple[str, str] | None = None  # a Python suite: its directory and module


def git(*args: str) -> str:
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True, check=True).stdout


def glob_regex(pattern: str) -> re.Pattern[str]:
    """`**` crosses directories, `*` and `?` do not."""
    out = ""
    for part in re.split(r"(\*\*|\*|\?)", pattern):
        out += {"**": ".*", "*": "[^/]*", "?": "[^/]"}.get(part, re.escape(part))
    return re.compile(out + r"\Z")


def parse_config(config: dict) -> tuple[list[Group], list[tuple[list[str], list[str]]]]:
    groups = []
    for entry in config.get("group", []):
        selectors = []
        for test in entry["tests"]:
            selector = Selector(
                package=test.get("package"),
                target=test.get("target"),
                filters=tuple(test.get("filters", [])),
                features=tuple(test.get("features", config.get("features", []))),
                env=tuple(sorted(test.get("env", {}).items())),
                unittest=test.get("unittest"),
                modules=tuple(test.get("modules", [])),
            )
            if (selector.package is None) == (selector.unittest is None):
                raise Failure(f"{CONFIG.name}: group {entry['name']}: a test names either a package or a unittest directory")
            selectors.append(selector)
        groups.append(Group(entry["name"], entry.get("paths", []), selectors, entry.get("before", [])))
    if len({g.name for g in groups}) != len(groups):
        raise Failure(f"{CONFIG.name}: group names must be unique")
    routes = [(route["paths"], route["groups"]) for route in config.get("route", [])]
    return groups, routes


def load() -> tuple[dict, list[Group], list[tuple[list[str], list[str]]]]:
    config = tomllib.loads(CONFIG.read_text())
    groups, routes = parse_config(config)
    return config, groups, routes


def baselines(config: dict) -> dict[str, str]:
    """Known failures of main, `test name # reason` per line (blank lines and lines starting with # are comments)."""
    path = config.get("baselines")
    if not path:
        return {}
    found: dict[str, str] = {}
    for number, line in enumerate((ROOT / path).read_text().splitlines(), 1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        name, separator, reason = line.partition(" # ")
        if not separator or not reason.strip() or not name.strip() or " " in name.strip():
            raise Failure(f"{path}:{number}: expected `exact::test::name # reason`")
        found[name.strip()] = reason.strip()
    return found


# ---------------------------------------------------------------------------------------------------------------- imports


def split_top(text: str) -> list[str]:
    """`a, b::{c, d}, e` -> the items at the top level."""
    items, depth, current = [], 0, ""
    for character in text:
        if character == "{":
            depth += 1
        elif character == "}":
            depth -= 1
        if character == "," and depth == 0:
            items.append(current)
            current = ""
        else:
            current += character
    return [*items, current]


def paths_in(text: str) -> list[tuple[str, list[str]]]:
    """Every `crate::`, `self::` and `super::` path of a source: its kind (`crate`, `self` or the count of `super` as digits)
    and its segments; `use crate::{a, b::c}` gives one path per item."""
    found = []
    for match in re.finditer(r"\b(crate::|self::|(?:super::)+)(\{[^;]*?\}\s*;|[A-Za-z_]\w*(?:::[A-Za-z_]\w*)*)", text):
        head, rest = match.groups()
        kind = "crate" if head == "crate::" else "self" if head == "self::" else str(head.count("super"))
        if rest.startswith("{"):
            inner = rest[: rest.rindex("}")][1:]
            for item in split_top(inner):
                item = item.strip().split("{")[0].rstrip(":").strip()
                segments = [s for s in item.split("::") if re.fullmatch(r"[A-Za-z_]\w*", s)]
                if segments:
                    found.append((kind, segments))
        else:
            found.append((kind, rest.split("::")))
    return found


def import_edges(files: list[str], read) -> dict[str, set[str]]:
    """file -> the Rust files it imports directly, per crate (a directory holding src/main.rs or src/lib.rs)."""
    roots = sorted({f[: -len("main.rs")].rstrip("/") for f in files if f.endswith("src/main.rs")}
                   | {f[: -len("lib.rs")].rstrip("/") for f in files if f.endswith("src/lib.rs")})
    available = set(files)
    edges: dict[str, set[str]] = {}
    for root in roots:
        crate = [f for f in files if f.endswith(".rs") and f.startswith(root + "/")]

        def module_of(path: str) -> list[str]:
            parts = path[len(root) + 1 : -3].split("/")
            if parts[-1] == "mod":
                parts = parts[:-1]
            return [] if parts in (["main"], ["lib"]) else parts

        def file_for(parts: list[str]) -> str | None:
            for length in range(len(parts), 0, -1):
                base = "/".join([root, *parts[:length]])
                for candidate in (base + ".rs", base + "/mod.rs"):
                    if candidate in available:
                        return candidate
            return None

        for path in crate:
            here = module_of(path)
            found = set()
            for kind, segments in paths_in(read(path)):
                if kind == "crate":
                    absolute = segments
                elif kind == "self":
                    absolute = here + segments
                else:
                    absolute = here[: max(0, len(here) - int(kind))] + segments
                target = file_for(absolute)
                if target and target != path:
                    found.add(target)
            edges[path] = found
    return edges


# ---------------------------------------------------------------------------------------------------------------- routing


class Router:
    """Which groups a change to a file runs."""

    def __init__(self, groups: list[Group], routes: list[tuple[list[str], list[str]]], derived: dict[str, set[str]] | None = None):
        self.names = [group.name for group in groups]
        self.owned = [([glob_regex(p) for p in group.paths], group.name) for group in groups]
        self.routes = [([glob_regex(p) for p in paths], tokens) for paths, tokens in routes]
        self.derived = derived or {}

    def expand(self, token: str) -> set[str]:
        if token == "*":
            return set(self.names)
        if token.endswith("*"):
            return {name for name in self.names if name.startswith(token[:-1])}
        return {token} if token in self.names else set()

    def owners(self, path: str) -> set[str]:
        """The groups whose `paths` name the file."""
        return {name for patterns, name in self.owned if any(p.match(path) for p in patterns)}

    def route(self, path: str) -> set[str] | None:
        """The groups a change to `path` runs (empty: none, for documents); None when nothing names the file."""
        groups = self.owners(path)
        named = bool(groups)
        for patterns, tokens in self.routes:
            if any(p.match(path) for p in patterns):
                named = True
                for token in tokens:
                    groups |= self.expand(token)
        if not named:
            return None
        return groups | self.derived.get(path, set())


def derive(router: Router, files: list[str], read, baseline_names: dict[str, str], baselines_path: str | None) -> dict[str, set[str]]:
    """The routes the code implies: a Rust file runs the groups of the test files that import it, a script a test file names runs
    that file's group, and the baselines file runs the groups of the tests it names."""
    derived: dict[str, set[str]] = {}
    edges = import_edges(files, read)
    sources = {f: read(f) for f in files if f.endswith((".rs", ".py"))}
    for path, imported in edges.items():
        if "#[test]" in sources[path] or "#[cfg(test)]" in sources[path]:
            for dependency in imported:
                derived.setdefault(dependency, set()).update(router.owners(path))
    for script in (f for f in files if f.endswith((".sh", ".py")) and not f.startswith("fuzz/")):
        token = re.compile(r"(?<![\w.-])" + re.escape(Path(script).name) + r"(?![\w-])")
        for path, text in sources.items():
            if path != script and token.search(text) and (router.owners(path) or path.startswith("tests/") or "test" in Path(path).name):
                owners = router.owners(path) or (router.route(path) or set())
                derived.setdefault(script, set()).update(owners)
    if baselines_path:
        for name in baseline_names:
            function = re.compile(r"\bfn\s+" + re.escape(name.split("::")[-1]) + r"\b")
            for path, text in sources.items():
                if path.endswith(".rs") and function.search(text):
                    derived.setdefault(baselines_path, set()).update(router.owners(path))
    return derived


def load_router(config: dict, groups: list[Group], routes: list[tuple[list[str], list[str]]]) -> Router:
    router = Router(groups, routes)
    files = every_file()
    cache: dict[str, str] = {}

    def read(path: str) -> str:
        if path not in cache:
            cache[path] = (ROOT / path).read_text(errors="replace")
        return cache[path]

    router.derived = derive(router, files, read, baselines(config), config.get("baselines"))
    return router


def every_file() -> list[str]:
    return sorted(set(git("ls-files").splitlines()) | set(git("ls-files", "--others", "--exclude-standard").splitlines()))


def default_base(config: dict) -> str:
    base = config.get("base", "main")
    found = subprocess.run(["git", "rev-parse", "-q", "--verify", f"origin/{base}"], cwd=ROOT, capture_output=True)
    return f"origin/{base}" if found.returncode == 0 else base


def merge_base(base: str) -> str:
    found = subprocess.run(["git", "merge-base", "HEAD", base], cwd=ROOT, capture_output=True, text=True)
    if found.returncode != 0:
        raise Failure(f"no merge-base of HEAD and {base}: {found.stderr.strip()}")
    return found.stdout.strip()


def selection(arguments: argparse.Namespace, config: dict) -> tuple[list[str] | None, str | None]:
    """The changed files the arguments name (None: everything) and the revision they are compared with."""
    if arguments.all:
        return None, None
    if arguments.change:
        ranges = [c for c in arguments.change if ".." in c and not (ROOT / c).exists()]
        if ranges:
            if len(arguments.change) != 1 or "..." in ranges[0]:
                raise Failure("name one range `A..B` or a list of files")
            first, second = ranges[0].split("..")
            return sorted(git("diff", "--name-only", "--no-renames", first, second).splitlines()), first
        return sorted(arguments.change), merge_base(default_base(config))
    mb = merge_base(arguments.base or default_base(config))
    diff = git("diff", "--name-only", "--no-renames", mb).splitlines()
    return sorted(set(diff) | set(git("ls-files", "--others", "--exclude-standard").splitlines())), mb


def config_delta(groups: list[Group], base_revision: str | None) -> set[str]:
    """What an edit of the toml runs: every group when a global key moved, else the groups whose definition changed (all of
    them when the toml did not exist at the revision)."""
    shown = subprocess.run(["git", "show", f"{base_revision}:{CONFIG.relative_to(ROOT)}"], cwd=ROOT, capture_output=True, text=True) if base_revision else None
    if shown is None or shown.returncode != 0:
        return {"tooling"} & {g.name for g in groups}
    old = tomllib.loads(shown.stdout)
    new = tomllib.loads(CONFIG.read_text())
    if {k: v for k, v in old.items() if k not in GLOBAL_TABLES} != {k: v for k, v in new.items() if k not in GLOBAL_TABLES}:
        return {g.name for g in groups}
    before = {e["name"]: e for e in old.get("group", [])}
    changed = {e["name"] for e in new.get("group", []) if before.get(e["name"]) != e}
    return (changed | {"tooling"}) & {g.name for g in groups}


def affected(router: Router, groups: list[Group], files: list[str] | None, base_revision: str | None = None) -> tuple[list[Group], list[str]]:
    """The groups to run, in the toml's order, and the files nothing names (they run every group)."""
    if files is None:
        return groups, []
    chosen: set[str] = set()
    unnamed = []
    for path in files:
        routed = router.route(path)
        if routed is None:
            unnamed.append(path)
            continue
        chosen |= routed
        if (ROOT / path) == CONFIG:
            chosen |= config_delta(groups, base_revision)
    if unnamed:
        return groups, unnamed
    return [g for g in groups if g.name in chosen], []


# ---------------------------------------------------------------------------------------------------------------- commands


def target_flags(target: str) -> list[str]:
    if target in ("", ):
        return []
    if target == "lib":
        return ["--lib"]
    if target == "doc":
        return ["--doc"]
    kind, name = target.split(":")
    return [f"--{kind}", name]


def cargo_prefix(config: dict, package: str, features: tuple[str, ...]) -> list[str]:
    flags = ["cargo", "test", "--profile", config.get("profile", "test-release"), *config.get("cargo_flags", [])]
    if features:
        flags += ["--features", ",".join(features)]
    return [*flags, "-p", package]


def arguments_of(filters: list[str], skipped: list[str]) -> list[str]:
    arguments = [*filters, *[a for name in skipped for a in ("--skip", name)]]
    return ["--", *arguments] if arguments else []


def with_env(env: tuple[tuple[str, str], ...], command: list[str]) -> list[str]:
    return ["env", *[f"{k}={v}" for k, v in env], *command] if env else command


def commands(config: dict, chosen: list[Group], skipped: list[str] = ()) -> tuple[list[list[str]], list[Planned]]:
    """The `before` commands and the test commands of the groups. Groups that select the same target share one command with
    the filters of all of them joined (libtest runs a test that any filter matches, so the union is exactly the groups' tests);
    targets taken whole by the same package share one command too. `skipped`: test names every command leaves out."""
    before: list[list[str]] = []
    keyed: dict[tuple, dict[str, set[str] | None]] = {}  # (package, features, env) -> target -> filters (None: the whole target)
    modules: list[Planned] = []
    for group in chosen:
        for command in group.before:
            if command not in before:
                before.append(command)
        for selector in group.selectors:
            if selector.unittest:
                for module in selector.modules:
                    argv = [sys.executable, "-B", "-m", "unittest", "discover", "-s", selector.unittest, "-t", selector.unittest, "-p", module + ".py"]
                    if not any(m.argv == argv for m in modules):
                        modules.append(Planned(argv, unit=(selector.unittest, module)))
                continue
            targets = keyed.setdefault((selector.package, selector.features, selector.env), {})
            target = selector.target or ""
            if target not in targets:
                targets[target] = set(selector.filters) or None
            elif targets[target] is not None:
                targets[target] = (targets[target] | set(selector.filters)) if selector.filters else None
    planned: list[Planned] = []
    for (package, features, env), targets in keyed.items():
        prefix = cargo_prefix(config, package, features)
        if "" in targets and targets[""] is None:
            planned.append(Planned(with_env(env, [*prefix, *arguments_of([], skipped)]), package, features, ("",), None))
            continue
        whole = [t for t, filters in targets.items() if filters is None]
        for target, filters in targets.items():
            if filters is not None:
                argv = [*prefix, *target_flags(target), *arguments_of(sorted(filters), skipped)]
                planned.append(Planned(with_env(env, argv), package, features, (target,), tuple(sorted(filters))))
        if whole:
            argv = [*prefix, *[f for t in whole for f in target_flags(t)], *arguments_of([], skipped)]
            planned.append(Planned(with_env(env, argv), package, features, tuple(whole), None))
    return before, [*planned, *modules]


def cmd_select(arguments: argparse.Namespace) -> int:
    config, groups, routes = load()
    router = load_router(config, groups, routes)
    files, base_revision = selection(arguments, config)
    chosen, unnamed = affected(router, groups, files, base_revision)
    for path in unnamed[:5]:
        print(f"# no group or route names {path}: every group runs", file=sys.stderr)
    print("# groups: " + (" ".join(g.name for g in chosen) or "none"))
    before, tests = commands(config, chosen)
    waived = [] if arguments.no_waive else baselines(config)
    if waived:
        print(f"# run adds --skip for the {len(waived)} known baselines of {config['baselines']} (--no-waive runs them)")
    for command in [*before, *(t.argv for t in tests)]:
        print(shlex.join(command))
    return 0


# ---------------------------------------------------------------------------------------------------------------- listing


def package_names() -> dict[str, str]:
    """Manifest path -> package name, for the packages of the workspace."""
    meta = json.loads(subprocess.run(["cargo", "metadata", "--format-version", "1", "--no-deps", "--offline"], cwd=ROOT, capture_output=True, text=True, check=True).stdout)
    return {p["manifest_path"]: p["name"] for p in meta["packages"]}


def tests_of(listing: str) -> list[str]:
    return [line[: -len(": test")] for line in listing.splitlines() if line.endswith(": test")]


def listing_of(executable: str, directory: Path) -> tuple[list[str], set[str]]:
    """The tests of a test binary, and the ones marked ignored."""
    done = subprocess.run([executable, "--list", "--format", "terse"], cwd=directory, capture_output=True, text=True)
    if done.returncode != 0:
        raise Failure(f"{executable} --list failed (a test target with `harness = false` cannot be grouped):\n{done.stderr[-1000:]}")
    odd = [line for line in done.stdout.splitlines() if line and not line.endswith((": test", ": benchmark"))]
    if odd:
        raise Failure(f"{executable} --list printed lines that are no tests (`harness = false`?): {odd[:3]}")
    ignored = subprocess.run([executable, "--list", "--ignored", "--format", "terse"], cwd=directory, capture_output=True, text=True)
    return tests_of(done.stdout), set(tests_of(ignored.stdout))


def feature_sets(config: dict, groups: list[Group]) -> list[tuple[str, ...]]:
    sets = {tuple(config.get("features", []))}
    for group in groups:
        sets |= {s.features for s in group.selectors if s.package}
    return sorted(sets)


def list_cargo(config: dict, sets: list[tuple[str, ...]]) -> dict[tuple[tuple[str, ...], str, str], Listed]:
    """(features, package, target) -> its tests, for the test targets of each build."""
    profile = config.get("profile", "test-release")
    names = package_names()
    found: dict[tuple[tuple[str, ...], str, str], Listed] = {}
    for features in sets:
        command = ["cargo", "test", "--profile", profile, "--no-run", "--message-format=json-render-diagnostics"]
        if features:
            command += ["--features", ",".join(features)]
        built = subprocess.run(command, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        if built.returncode != 0:
            raise Failure(f"building the tests failed: {shlex.join(command)}\n{built.stderr[-3000:]}")
        for line in built.stdout.splitlines():
            message = json.loads(line)
            if message.get("reason") != "compiler-artifact" or not message["profile"]["test"] or not message.get("executable"):
                continue
            package = names.get(message["manifest_path"])
            if package is None:
                continue
            target = message["target"]
            kind = "lib" if target["kind"][0] in LIBRARY_KINDS else f"{target['kind'][0]}:{target['name']}"
            directory = Path(message["manifest_path"]).parent
            tests, ignored = listing_of(message["executable"], directory)
            found[(features, package, kind)] = Listed(tests, ignored, Path(target["src_path"]))
            if kind == "lib" and target.get("doctest", False):
                doc = cargo_prefix(config, package, features) + ["--doc", "--", "--list", "--format", "terse"]
                done = subprocess.run(doc, cwd=ROOT, capture_output=True, text=True)
                if done.returncode != 0:
                    raise Failure(f"listing the doc tests of {package} failed:\n{done.stderr[-1000:]}")
                if tests_of(done.stdout):
                    found[(features, package, "doc")] = Listed(tests_of(done.stdout), set(), directory)
    return found


def list_unittest(directory: str, modules: list[str]) -> list[str]:
    walk = (
        "import sys, unittest\n"
        "def walk(suite):\n"
        "    for test in suite:\n"
        "        yield from walk(test) if isinstance(test, unittest.TestSuite) else [test]\n"
        "sys.path.insert(0, sys.argv[1])\n"
        "for test in walk(unittest.defaultTestLoader.loadTestsFromNames(sys.argv[2:])):\n"
        "    print(test.id())\n"
    )
    found = subprocess.run([sys.executable, "-B", "-c", walk, directory, *modules], cwd=ROOT, capture_output=True, text=True)
    names = found.stdout.split()
    broken = [n for n in names if n.startswith("unittest.loader.")]
    if found.returncode != 0 or broken:
        raise Failure(f"loading the Python tests {modules} of {directory} failed: {broken or found.stderr[-1000:]}")
    return names


def source_file(listed: Listed, kind: str, name: str) -> Path:
    """The file a test is written in: its module path walked down from the target's root, as rustc finds module files."""
    if kind == "doc":
        return ROOT / name.split(" - ", 1)[0]
    file = listed.root
    directory = file.parent
    for segment in name.split("::")[:-1]:
        found = next((c for c in (directory / f"{segment}.rs", directory / segment / "mod.rs") if c.is_file()), None)
        if found is None:
            break
        file, directory = found, directory / segment
    return file


def selects(selector: Selector, package: str, kind: str, name: str) -> bool:
    if selector.package != package or selector.target not in (None, kind):
        return False
    return not selector.filters or any(f in name for f in selector.filters)


# ---------------------------------------------------------------------------------------------------------------- run


def failed_tests(log: str) -> list[str]:
    return sorted(set(re.findall(r"^---- (.*) stdout ----$", log, re.MULTILINE)))


def execute(command: list[str]) -> tuple[int, str]:
    """Runs a command with its output on ours too; returns its status and the output."""
    lines = []
    with subprocess.Popen(command, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, errors="replace") as process:
        assert process.stdout is not None
        for line in process.stdout:
            sys.stdout.write(line)
            lines.append(line)
    return process.returncode, "".join(lines)


def counts_of(log: str) -> tuple[int, int, int, int]:
    """Passed, failed, ignored and measured tests the libtest reports of a log add up to."""
    total = [0, 0, 0, 0]
    for found in re.finditer(r"test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured", log):
        for index, number in enumerate(found.groups()):
            total[index] += int(number)
    return total[0], total[1], total[2], total[3]


def expected_of(plan: Planned, listings: dict, skipped: list[str]) -> tuple[int, int]:
    """How many tests the command runs and how many it reports ignored, from the listing."""
    ran = ignored = 0
    for (features, package, kind), listed in listings.items():
        if features != plan.features or package != plan.package:
            continue
        if "" not in plan.targets and kind not in plan.targets:
            continue
        for name in listed.tests:
            if plan.filters and not any(f in name for f in plan.filters):
                continue
            if any(s in name for s in skipped):
                continue
            if name in listed.ignored:
                ignored += 1
            else:
                ran += 1
    return ran, ignored


def problem_of(plan: Planned, status: int, log: str, listings: dict, skipped: list[str], units: dict[str, int]) -> str | None:
    """Why a finished command failed, None when it passed: its exit code, or a count of tests other than the selection's."""
    if status != 0:
        return f"failed: {shlex.join(plan.argv)}"
    if plan.unit:
        if re.search(r"skipped=\d", log):
            return f"Python tests skipped themselves, and a skipped test did not run: {shlex.join(plan.argv)}"
        ran = sum(int(n) for n in re.findall(r"^Ran (\d+) tests? in ", log, re.MULTILINE))
        wanted = units[f"{plan.unit[0]}/{plan.unit[1]}"]
        return None if ran == wanted else f"{plan.unit[1]} ran {ran} tests where its group selects {wanted}"
    passed, failed, ignored, measured = counts_of(log)
    ran, ignoring = expected_of(plan, listings, skipped)
    if measured:
        return f"{measured} measured tests: {shlex.join(plan.argv)}"
    if (passed, ignored) != (ran, ignoring):
        return (f"it ran {passed} tests and reported {ignored} ignored where its groups select {ran} and {ignoring} ignored "
                f"(the selection and libtest's disagree): {shlex.join(plan.argv)}")
    return None


def unwaived(waived: dict[str, str], files: list[str] | None) -> dict[str, str]:
    """The baselines whose test is written in a changed file: they run, so a change to them is never green by skipping."""
    if not files:
        return {}
    texts = [(ROOT / f).read_text(errors="replace") for f in files if f.endswith(".rs") and (ROOT / f).is_file()]
    return {n: r for n, r in waived.items() if any(re.search(r"\bfn\s+" + re.escape(n.split("::")[-1]) + r"\b", t) for t in texts)}


def check_skips(waived: dict[str, str], names: list[str]) -> list[str]:
    """`--skip NAME` skips every test whose name contains NAME: a baseline must be a test and only that one."""
    errors = []
    for name in waived:
        if name not in names:
            errors.append(f"baseline {name} is no test (a fixed or renamed test leaves the baselines file)")
        elif sum(name in other for other in names) != 1:
            errors.append(f"baseline {name} is a part of other tests' names, so skipping it would skip them too: name it fully")
    return errors


def cmd_run(arguments: argparse.Namespace) -> int:
    config, groups, routes = load()
    router = load_router(config, groups, routes)
    files, base_revision = selection(arguments, config)
    chosen, unnamed = affected(router, groups, files, base_revision)
    if unnamed:
        print(f"test groups: no group or route names {unnamed[0]}: every group runs")
    waived = {} if arguments.no_waive else baselines(config)
    forced = unwaived(waived, files)
    effective = {n: r for n, r in waived.items() if n not in forced}
    before, tests = commands(config, chosen, sorted(effective))
    record = Path(arguments.record) if arguments.record else None
    print("test groups: " + (" ".join(g.name for g in chosen) or "none"))
    if forced:
        print(f"   a changed file holds {len(forced)} waived baseline(s), so they run: {', '.join(forced)}")
    cargo = [t for t in tests if t.package]
    listings = list_cargo(config, sorted({t.features for t in cargo})) if cargo else {}
    names = [n for (_, _, _), listed in listings.items() for n in listed.tests]
    errors = check_skips(effective, names) if cargo else []
    units = {f"{t.unit[0]}/{t.unit[1]}": len(list_unittest(t.unit[0], [t.unit[1]])) for t in tests if t.unit}
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    for command in before:
        if execute(command)[0] != 0:
            print(f"TEST-GROUPS-FAIL {shlex.join(command)}")
            return 1
    previous = [name for name in (record.read_text().split("\n") if record and record.exists() else []) if name]
    if previous:
        print(f"   first, the tests that failed last time ({len(previous)})")
        skips = [a for name in effective for a in ("--skip", name)]
        for plan in cargo:
            head = plan.argv[: plan.argv.index("--")] if "--" in plan.argv else plan.argv
            if execute([*head, "--", *previous, *skips])[0] != 0:
                return 1
    failed: list[str] = []
    for plan in tests:
        status, log = execute(plan.argv)
        problem = problem_of(plan, status, log, listings, sorted(effective), units)
        if problem:
            failed += failed_tests(log)
            if record:
                record.write_text("\n".join(sorted(set(failed))) + "\n")
            print(f"TEST-GROUPS-FAIL {problem}")
            return 1
    if record:
        record.unlink(missing_ok=True)
    if effective and cargo:
        print(f"test groups: green; {len(effective)} waived baseline(s) were not run:")
        for name, reason in effective.items():
            print(f"   waived baseline: {name} -- {reason}")
    return 0


# ---------------------------------------------------------------------------------------------------------------- check


def wanted_builds(config: dict, groups: list[Group], router: Router, files: list[str] | None) -> list[tuple[str, ...]]:
    """The feature sets `check` builds: all of them for the whole repository, else the plain one, the ones a group the change
    runs uses, and the ones a changed file can matter to (a feature named in its text, or the build's own files)."""
    sets = feature_sets(config, groups)
    if files is None:
        return sets
    plain = tuple(config.get("features", []))
    chosen, unnamed = affected(router, groups, files)
    used = {s.features for g in chosen for s in g.selectors if s.package}
    texts = " ".join((ROOT / f).read_text(errors="replace") for f in files if f.endswith(".rs") and (ROOT / f).is_file())
    builds = {f for f in files if f in ("Cargo.toml", "Cargo.lock", "build.rs") or f.endswith("/Cargo.toml") or f == str(CONFIG.relative_to(ROOT))}
    wanted = []
    for features in sets:
        extra = set(features) - set(plain)
        needed = features == plain or features in used or bool(unnamed) or bool(builds) or any(f'feature = "{e}"' in texts for e in extra)
        if needed:
            wanted.append(features)
    return wanted


def cmd_check(arguments: argparse.Namespace) -> int:
    config, groups, routes = load()
    router = load_router(config, groups, routes)
    files = every_file()
    changed = None
    if arguments.base or arguments.change:
        changed, _ = selection(arguments, config)
    errors: list[str] = check_routing(router, groups, routes, files)
    owners: dict[tuple[str, str, str, str], list[str]] = {}  # (package, target, features, test) -> its groups
    counts = {g.name: 0 for g in groups}
    hits: dict[tuple[str, int], int] = {}
    routed_files: list[tuple[Path, str, str]] = []
    listings = list_cargo(config, wanted_builds(config, groups, router, changed))
    for (features, package, kind), listed in sorted(listings.items()):
        for name in listed.tests:
            who = []
            for group in groups:
                for index, selector in enumerate(group.selectors):
                    if selector.features == features and selects(selector, package, kind, name):
                        hits[(group.name, index)] = hits.get((group.name, index), 0) + 1
                        if group.name not in who:
                            who.append(group.name)
            owners[(package, kind, ",".join(features), name)] = who
            for g in who:
                counts[g] += 1
            if who:
                routed_files.append((source_file(listed, kind, name), who[0], f"{package} {kind} {name}"))
    errors += check_python(config, groups, files, owners, counts, routed_files)
    covered = {(package, kind, name) for (package, kind, features, name), who in owners.items() if who}
    for (package, kind, features, name), who in owners.items():
        label = f"{package} {kind}{' with ' + features if features else ''} {name}"
        if not who and (package, kind, name) not in covered:  # a test of two builds is in a group of either
            errors.append(f"in no group: {label}")
        elif len(set(who)) > 1:
            errors.append(f"in {len(set(who))} groups ({', '.join(sorted(set(who)))}) of one build: {label}")
    names = [key[3] for key in owners]
    errors += check_skips(baselines(config), names) if changed is None else []
    for group in groups:
        if counts[group.name] == 0:
            errors.append(f"group {group.name} holds no test")
        for index, selector in enumerate(group.selectors):
            if selector.package and changed is None and not hits.get((group.name, index)):
                errors.append(f"group {group.name}: {selector.package} {selector.target or '*'} {list(selector.filters)} selects no test")
            for filter_ in selector.filters:
                single = Selector(selector.package, selector.target, (filter_,), selector.features, (), None, ())
                if changed is None and not any(selects(single, p, k, n) for (p, k, f, n) in owners if f == ",".join(selector.features)):
                    errors.append(f"group {group.name}: filter {filter_!r} selects no test")
    errors += check_files(router, routed_files, import_edges(files, lambda path: (ROOT / path).read_text(errors="replace")))
    errors += check_scripts(router, groups, files)
    if errors:
        print("\n".join(errors[:60]), file=sys.stderr)
        if len(errors) > 60:
            print(f"... and {len(errors) - 60} more", file=sys.stderr)
        return 1
    unique = {(p, k, n) for (p, k, _, n) in owners}
    print(f"test groups: {len(unique)} tests in {len(groups)} groups, every file routed")
    return 0


def check_python(config: dict, groups: list[Group], files: list[str], owners: dict, counts: dict, routed_files: list) -> list[str]:
    errors = []
    outside = {entry["path"] for entry in config.get("outside", [])}
    for directory in sorted({s.unittest for g in groups for s in g.selectors if s.unittest}):
        grouped = sorted({m for g in groups for s in g.selectors if s.unittest == directory for m in s.modules})
        for file in files:
            if re.fullmatch(re.escape(directory) + r"/test_[^/]*\.py", file) and file not in outside and Path(file).stem not in grouped:
                errors.append(f"in no group: Python tests {file} (a group names its module, or [[outside]] says which gate step runs it)")
        for module in grouped:
            if f"{directory}/{module}.py" not in files:
                errors.append(f"group names Python tests {directory}/{module}.py, which is no file")
        for name in list_unittest(directory, [m for m in grouped if f"{directory}/{m}.py" in files]):
            module = name.split(".")[0]
            who = sorted({g.name for g in groups for s in g.selectors if s.unittest == directory and module in s.modules})
            owners[("python", directory, "", name)] = who
            for g in who:
                counts[g] += 1
            routed_files.append((ROOT / directory / f"{module}.py", who[0], f"python {name}"))
    return errors


def check_files(router: Router, routed_files: list[tuple[Path, str, str]], edges: dict[str, set[str]]) -> list[str]:
    """A test's file is in its group's `paths`, so a change to it runs the group; the files it imports directly run that group
    too (the derived routes make that so, and this proves the derivation reached them)."""
    errors = []
    seen = set()
    for file, group, label in routed_files:
        if not file.is_relative_to(ROOT) or (file, group) in seen:
            continue
        seen.add((file, group))
        path = str(file.relative_to(ROOT))
        if group not in router.owners(path):
            errors.append(f"{label} is written in {path}, which is not in the `paths` of its group {group}")
            continue
        for dependency in sorted(edges.get(path, ())):
            if group not in (router.route(dependency) or set()):
                errors.append(f"{path} imports {dependency}, which would not run its group {group} when changed")
    return errors


def check_scripts(router: Router, groups: list[Group], files: list[str]) -> list[str]:
    """A script a test file names runs that file's group when changed (derived), unless a route sent it nowhere on purpose."""
    errors = []
    for script, owners in sorted(router.derived.items()):
        if script not in files:
            continue
        routed = router.route(script)
        if routed is not None and not owners <= routed:
            errors.append(f"{script} is named by tests of {sorted(owners - routed)}, which it would not run")
    return errors


def check_routing(router: Router, groups: list[Group], routes: list[tuple[list[str], list[str]]], files: list[str]) -> list[str]:
    errors = []
    for path in files:
        if router.route(path) is None:
            errors.append(f"no group or route names {path}")
    for group in groups:
        for pattern in group.paths:
            if not any(glob_regex(pattern).match(f) for f in files):
                errors.append(f"group {group.name}: path pattern {pattern!r} matches no file")
    for paths, tokens in routes:
        for pattern in paths:
            if not any(glob_regex(pattern).match(f) for f in files):
                errors.append(f"route pattern {pattern!r} matches no file")
        for token in tokens:
            if token != "*" and not router.expand(token):
                errors.append(f"route to {token!r} names no group")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands_ = parser.add_subparsers(dest="command", required=True)
    for name in ("check", "select", "run"):
        sub = commands_.add_parser(name)
        if name == "check":
            sub.add_argument("--all", action="store_true", help="everything (the default)")
            sub.add_argument("--base", help="check as much as the change since the merge-base with this revision can matter to")
            sub.set_defaults(change=[], no_waive=False)
            continue
        sub.add_argument("--all", action="store_true", help="every group")
        sub.add_argument("--base", help="the revision the work is compared with")
        sub.add_argument("--no-waive", action="store_true", help="run the known baseline failures too")
        if name == "run":
            sub.add_argument("--record", help="file the failed tests' names are kept in, and run first next time")
        sub.add_argument("change", nargs="*", help="A..B, or the changed files")
    arguments = parser.parse_args()
    try:
        return {"check": cmd_check, "select": cmd_select, "run": cmd_run}[arguments.command](arguments)
    except Failure as failure:
        print(f"test groups: {failure}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
