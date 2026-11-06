#!/usr/bin/env python3
"""The test groups of this repository (tools/test_groups.toml): which tests a change needs, and the proof that none is left out.

    tools/test_groups.py check                            every test in exactly one group, every file routed
    tools/test_groups.py select [--all | --base REV | A..B | FILE...]
                                                          the groups the change affects and the commands that run them
    tools/test_groups.py run [--record FILE] [selection]  the same commands, run one after the other, stopping at the first failure

A change is the files differing from the merge-base of HEAD with REV (default origin/main, main without an origin; committed,
uncommitted and new files), the files between two revisions `A..B`, or the files named. A file that no group or route names
runs every group. `check` builds the repository's tests, asks each test binary for its tests (libtest's `--list`, doc tests and
Python unittest ids too) and fails on a test in no group or in two, a group without a test, a filter or a path pattern that
matches nothing, a file nothing routes, and a test whose own source file would not run its group when changed.
Known failures of main (the file `baselines` names: exact test names, each with its reason) are skipped by `run` and listed as
waived, so one of them does not hide the rest of the suite; `--no-waive` runs them. Any other failure stops the run.
Tests marked `#[ignore]` are listed and grouped like the others but not run by a group's command: the gate's own steps run them.
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
    """The tests of one test target: its names, the file it starts at and its package's directory."""

    tests: list[str]
    root: Path


def git(*args: str) -> str:
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True, check=True).stdout


def glob_regex(pattern: str) -> re.Pattern[str]:
    """`**` crosses directories, `*` and `?` do not."""
    out = ""
    for part in re.split(r"(\*\*|\*|\?)", pattern):
        out += {"**": ".*", "*": "[^/]*", "?": "[^/]"}.get(part, re.escape(part))
    return re.compile(out + r"\Z")


def load() -> tuple[dict, list[Group], list[tuple[list[str], list[str]]]]:
    config = tomllib.loads(CONFIG.read_text())
    groups = []
    for entry in config.get("group", []):
        selectors = []
        for test in entry["tests"]:
            selectors.append(
                Selector(
                    package=test.get("package"),
                    target=test.get("target"),
                    filters=tuple(test.get("filters", [])),
                    features=tuple(test.get("features", config.get("features", []))),
                    env=tuple(sorted(test.get("env", {}).items())),
                    unittest=test.get("unittest"),
                    modules=tuple(test.get("modules", [])),
                )
            )
            if (selectors[-1].package is None) == (selectors[-1].unittest is None):
                raise Failure(f"{CONFIG.name}: group {entry['name']}: a test names either a package or a unittest directory")
        groups.append(Group(entry["name"], entry.get("paths", []), selectors, entry.get("before", [])))
    if len({g.name for g in groups}) != len(groups):
        raise Failure(f"{CONFIG.name}: group names must be unique")
    routes = [(route["paths"], route["groups"]) for route in config.get("route", [])]
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


class Router:
    """Which groups a change to a file runs."""

    def __init__(self, groups: list[Group], routes: list[tuple[list[str], list[str]]]):
        self.names = [group.name for group in groups]
        self.owned = [([glob_regex(p) for p in group.paths], group.name) for group in groups]
        self.routes = [([glob_regex(p) for p in paths], tokens) for paths, tokens in routes]
        self.route_patterns = routes

    def expand(self, token: str) -> set[str]:
        if token == "*":
            return set(self.names)
        if token.endswith("*"):
            return {name for name in self.names if name.startswith(token[:-1])}
        return {token} if token in self.names else set()

    def route(self, path: str) -> set[str] | None:
        """The groups a change to `path` runs (empty: none, for documents); None when nothing names the file."""
        groups: set[str] = set()
        named = False
        for patterns, name in self.owned:
            if any(p.match(path) for p in patterns):
                groups.add(name)
                named = True
        for patterns, tokens in self.routes:
            if any(p.match(path) for p in patterns):
                named = True
                for token in tokens:
                    groups |= self.expand(token)
        return groups if named else None


def every_file() -> list[str]:
    return sorted(set(git("ls-files").splitlines()) | set(git("ls-files", "--others", "--exclude-standard").splitlines()))


def default_base() -> str:
    found = subprocess.run(["git", "rev-parse", "-q", "--verify", "origin/main"], cwd=ROOT, capture_output=True)
    return "origin/main" if found.returncode == 0 else "main"


def changed_since(base: str) -> list[str]:
    """The paths changed since the merge-base of HEAD with `base`, committed or not; a moved file counts at both its paths."""
    found = subprocess.run(["git", "merge-base", "HEAD", base], cwd=ROOT, capture_output=True, text=True)
    if found.returncode != 0:
        raise Failure(f"no merge-base of HEAD and {base}: {found.stderr.strip()}")
    diff = git("diff", "--name-only", "--no-renames", found.stdout.strip()).splitlines()
    return sorted(set(diff) | set(git("ls-files", "--others", "--exclude-standard").splitlines()))


def selection(arguments: argparse.Namespace) -> list[str] | None:
    """The changed files the arguments name; None: everything."""
    if arguments.all:
        return None
    if arguments.change:
        ranges = [c for c in arguments.change if ".." in c and not Path(ROOT / c).exists()]
        if ranges:
            if len(arguments.change) != 1 or "..." in ranges[0]:
                raise Failure("name one range `A..B` or a list of files")
            first, second = ranges[0].split("..")
            return sorted(git("diff", "--name-only", "--no-renames", first, second).splitlines())
        return sorted(arguments.change)
    return changed_since(arguments.base or default_base())


def affected(router: Router, groups: list[Group], files: list[str] | None) -> tuple[list[Group], list[str]]:
    """The groups to run, in the file's order, and the files nothing names (they run everything)."""
    if files is None:
        return groups, []
    chosen: set[str] = set()
    unnamed = []
    for path in files:
        routed = router.route(path)
        if routed is None:
            unnamed.append(path)
        else:
            chosen |= routed
    if unnamed:
        return groups, unnamed
    return [g for g in groups if g.name in chosen], []


def cargo_flags(selector: Selector, config: dict) -> tuple[list[str], list[str]]:
    """The cargo test command's flags before the target, and the flags that name the target."""
    flags = ["cargo", "test", "--profile", config.get("profile", "test-release"), *config.get("cargo_flags", [])]
    if selector.features:
        flags += ["--features", ",".join(selector.features)]
    flags += ["-p", selector.package]
    target = selector.target
    if target == "lib":
        return flags, ["--lib"]
    if target == "doc":
        return flags, ["--doc"]
    if target is not None:
        kind, name = target.split(":")
        return flags, [f"--{kind}", name]
    return flags, []


def commands(config: dict, chosen: list[Group], skipped: list[str] = ()) -> tuple[list[list[str]], list[list[str]]]:
    """The `before` commands and the test commands of the groups. Groups that select the same target share one command with
    the filters of all of them joined (libtest runs a test that any filter matches, so the union is exactly the groups'
    tests); targets taken whole by the same package share one command too. `skipped`: test names every command leaves out."""
    before: list[list[str]] = []
    targets: dict[tuple, set[str] | None] = {}  # (flags before the target, env, target flags) -> filters; None: the whole target
    modules: list[list[str]] = []
    for group in chosen:
        for command in group.before:
            if command not in before:
                before.append(command)
        for selector in group.selectors:
            if selector.unittest:
                for module in selector.modules:
                    command = [sys.executable, "-B", "-m", "unittest", "discover", "-s", selector.unittest, "-t", selector.unittest, "-p", module + ".py"]
                    if command not in modules:
                        modules.append(command)
                continue
            prefix, target = cargo_flags(selector, config)
            key = (tuple(prefix), selector.env, tuple(target))
            if key not in targets:
                targets[key] = set(selector.filters) or None
            elif targets[key] is not None:
                targets[key] = (targets[key] | set(selector.filters)) if selector.filters else None
    out: list[list[str]] = []
    whole: dict[tuple, list[str]] = {}
    for (prefix, env, target), filters in targets.items():
        if filters is None and target:
            whole.setdefault((prefix, env), []).extend(target)
            continue
        out.append(with_env(env, [*prefix, *target, *arguments_of(sorted(filters or ()), skipped)]))
    for (prefix, env), target in whole.items():
        out.append(with_env(env, [*prefix, *target, *arguments_of([], skipped)]))
    return before, [*out, *modules]


def with_env(env: tuple[tuple[str, str], ...], command: list[str]) -> list[str]:
    return ["env", *[f"{k}={v}" for k, v in env], *command] if env else command


def arguments_of(filters: list[str], skipped: list[str]) -> list[str]:
    arguments = [*filters, *[a for name in skipped for a in ("--skip", name)]]
    return ["--", *arguments] if arguments else []


def cmd_select(arguments: argparse.Namespace) -> int:
    config, groups, routes = load()
    router = Router(groups, routes)
    files = selection(arguments)
    chosen, unnamed = affected(router, groups, files)
    for path in unnamed[:5]:
        print(f"# no group or route names {path}: every group runs", file=sys.stderr)
    print("# groups: " + (" ".join(g.name for g in chosen) or "none"))
    before, tests = commands(config, chosen)
    waived = [] if arguments.no_waive else baselines(config)
    if waived:
        print(f"# run adds --skip for the {len(waived)} known baselines of {config['baselines']} (--no-waive runs them)")
    for command in [*before, *tests]:
        print(shlex.join(command))
    return 0


def failed_tests(log: str) -> list[str]:
    return sorted(set(re.findall(r"^---- (.*) stdout ----$", log, re.MULTILINE)))


def execute(command: list[str]) -> tuple[int, str]:
    """Runs a command with its output on ours too; returns its status and the output."""
    process = subprocess.Popen(command, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, errors="replace")
    lines = []
    assert process.stdout is not None
    for line in process.stdout:
        sys.stdout.write(line)
        lines.append(line)
    return process.wait(), "".join(lines)


def cmd_run(arguments: argparse.Namespace) -> int:
    config, groups, routes = load()
    router = Router(groups, routes)
    chosen, unnamed = affected(router, groups, selection(arguments))
    if unnamed:
        print(f"test groups: no group or route names {unnamed[0]}: every group runs")
    waived = {} if arguments.no_waive else baselines(config)
    before, tests = commands(config, chosen, sorted(waived))
    record = Path(arguments.record) if arguments.record else None
    print("test groups: " + (" ".join(g.name for g in chosen) or "none"))
    for command in before:
        if execute(command)[0] != 0:
            print(f"TEST-GROUPS-FAIL {shlex.join(command)}")
            return 1
    previous = record.read_text().split("\n") if record and record.exists() else []
    previous = [name for name in previous if name]
    if previous:
        print(f"   first, the tests that failed last time ({len(previous)})")
        for command in tests:
            skips = [a for name in waived for a in ("--skip", name)]
            if "unittest" not in command and execute(_without_filters(command) + ["--", *previous, *skips])[0] != 0:
                return 1
    failed: list[str] = []
    for command in tests:
        status, log = execute(command)
        problem = None
        if status != 0:
            problem = f"failed: {shlex.join(command)}"
            failed += failed_tests(log)
        elif "unittest" in command and re.search(r"skipped=\d", log):
            problem = f"Python tests skipped themselves, and a skipped test did not run: {shlex.join(command)}"
        if problem:
            if record:
                record.write_text("\n".join(sorted(set(failed))) + "\n")
            print(f"TEST-GROUPS-FAIL {problem}")
            return 1
    if record:
        record.unlink(missing_ok=True)
    if waived and any("unittest" not in command for command in tests):
        print(f"test groups: green; {len(waived)} waived baseline(s) were not run:")
        for name, reason in waived.items():
            print(f"   waived baseline: {name} -- {reason}")
    return 0


def _without_filters(command: list[str]) -> list[str]:
    return command[: command.index("--")] if "--" in command else command


def package_names() -> dict[str, str]:
    """Manifest path -> package name, for the packages of the workspace."""
    meta = json.loads(subprocess.run(["cargo", "metadata", "--format-version", "1", "--no-deps", "--offline"], cwd=ROOT, capture_output=True, text=True, check=True).stdout)
    return {p["manifest_path"]: p["name"] for p in meta["packages"]}


def tests_of(listing: str) -> list[str]:
    return [line[: -len(": test")] for line in listing.splitlines() if line.endswith(": test")]


def listing_of(executable: str, directory: Path) -> list[str]:
    done = subprocess.run([executable, "--list", "--format", "terse"], cwd=directory, capture_output=True, text=True)
    if done.returncode != 0:
        raise Failure(f"{executable} --list failed (a test target with `harness = false` cannot be grouped):\n{done.stderr[-1000:]}")
    odd = [line for line in done.stdout.splitlines() if line and not line.endswith((": test", ": benchmark"))]
    if odd:
        raise Failure(f"{executable} --list printed lines that are no tests (`harness = false`?): {odd[:3]}")
    return tests_of(done.stdout)


def list_cargo(config: dict) -> dict[tuple[str, str], Listed]:
    """(package, target) -> its tests, for every test target the plain build and each feature set a group names holds."""
    profile = config.get("profile", "test-release")
    names = package_names()
    sets = {tuple(config.get("features", []))}
    for entry in config.get("group", []):
        for test in entry["tests"]:
            sets.add(tuple(test.get("features", config.get("features", []))))
    found: dict[tuple[str, str], Listed] = {}
    for features in sorted(sets):
        command = ["cargo", "test", "--profile", profile, "--no-run", "--message-format=json-render-diagnostics"]
        if features:
            command += ["--features", ",".join(features)]
        built = subprocess.run(command, cwd=ROOT, stdout=subprocess.PIPE, text=True)
        if built.returncode != 0:
            raise Failure(f"building the tests failed: {shlex.join(command)}")
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
            found[(package, kind)] = Listed(listing_of(message["executable"], directory), Path(target["src_path"]))
            if kind == "lib" and target.get("doctest", False):
                done = subprocess.run(command[:2] + ["--profile", profile, "-p", package, "--doc", "--", "--list", "--format", "terse"],
                                      cwd=ROOT, capture_output=True, text=True)
                if done.returncode != 0:
                    raise Failure(f"listing the doc tests of {package} failed:\n{done.stderr[-1000:]}")
                if tests_of(done.stdout):
                    found[(package, "doc")] = Listed(tests_of(done.stdout), directory)
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


def cmd_check(arguments: argparse.Namespace) -> int:
    config, groups, routes = load()
    router = Router(groups, routes)
    errors: list[str] = []
    files = every_file()
    errors += check_routing(router, groups, routes, files)
    owners: dict[tuple[str, str, str], list[str]] = {}
    hits: dict[tuple[str, int], int] = {}
    counts = {g.name: 0 for g in groups}
    routed_files: list[tuple[Path, str, str]] = []
    for (package, kind), listed in sorted(list_cargo(config).items()):
        for name in listed.tests:
            who = []
            for group in groups:
                for index, selector in enumerate(group.selectors):
                    if selects(selector, package, kind, name):
                        hits[(group.name, index)] = hits.get((group.name, index), 0) + 1
                        if group.name not in who:
                            who.append(group.name)
            owners[(package, kind, name)] = who
            counts.update({g: counts[g] + 1 for g in who})
            if who:
                routed_files.append((source_file(listed, kind, name), who[0], f"{package} {kind} {name}"))
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
            who = {g.name for g in groups for s in g.selectors if s.unittest == directory and module in s.modules}
            owners[("python", directory, name)] = sorted(who)
            counts.update({g: counts[g] + 1 for g in who})
            routed_files.append((ROOT / directory / f"{module}.py", sorted(who)[0], f"python {name}"))
    for (package, kind, name), who in owners.items():
        if not who:
            errors.append(f"in no group: {package} {kind} {name}")
        elif len(set(who)) > 1:
            errors.append(f"in {len(set(who))} groups ({', '.join(sorted(set(who)))}): {package} {kind} {name}")
    names = [n for (_, _, n) in owners]
    for name in baselines(config):
        if name not in names:
            errors.append(f"baseline {name} is no test (a fixed or renamed test leaves the baselines file)")
        elif sum(name in n for n in names) != 1:
            errors.append(f"baseline {name} is a part of other tests' names, so skipping it would skip them too: name it fully")
    for group in groups:
        if counts[group.name] == 0:
            errors.append(f"group {group.name} holds no test")
        for index, selector in enumerate(group.selectors):
            if selector.package and not hits.get((group.name, index)):
                errors.append(f"group {group.name}: {selector.package} {selector.target or '*'} {list(selector.filters)} selects no test")
            for filter_ in selector.filters:
                if not any(selects(Selector(selector.package, selector.target, (filter_,), (), (), None, ()), p, k, n) for (p, k, n) in owners):
                    errors.append(f"group {group.name}: filter {filter_!r} selects no test")
    for file, group, label in routed_files:
        path = str(file.relative_to(ROOT)) if file.is_relative_to(ROOT) else None
        routed = router.route(path) if path else None
        if routed is not None and group not in routed and path:
            errors.append(f"{label} is written in {path}, which does not run its group {group} when changed")
    if errors:
        print("\n".join(errors[:60]), file=sys.stderr)
        if len(errors) > 60:
            print(f"... and {len(errors) - 60} more", file=sys.stderr)
        return 1
    total = len(owners)
    print(f"test groups: {total} tests in {len(groups)} groups, every file routed")
    return 0


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
    commands_.add_parser("check")
    for name in ("select", "run"):
        sub = commands_.add_parser(name)
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
