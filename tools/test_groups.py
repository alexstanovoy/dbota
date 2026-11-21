#!/usr/bin/env python3
"""The test groups of this repository (tools/test_groups.toml): which tests a change needs, and the proof that none is left out.

    tools/test_groups.py check [--base REV | --all] [--no-build]
                                                          every test in exactly one group, every file routed
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
nothing, a file nothing routes, a test file that is not in its group's `paths`, and a file rustc built a test binary from that
the module tree (which the import routes and the test-to-file mapping rest on) does not reach. `run` checks that every command
ran exactly the tests its groups select.

`check --no-build` builds nothing: it reads the `#[test]` functions of every test target from the source (the module tree from
the target's root file, each test placed in the builds whose features and cfgs it compiles in) and checks all of the above that
needs no binary. Doc tests, tests behind a cfg it cannot evaluate (counted in its summary) and the files rustc actually built are
the full check's: it is the quick check while working, and the full one stays the merge check.

Known failures of main (the file `baselines` names: exact test names, each with its reason) are skipped by `run` and listed as
waived, so one of them does not hide the rest of the suite; `--no-waive` runs them, and a change to any file routed to a waived
test's group runs it too. Any other failure stops the run. Tests marked `#[ignore]` are grouped like the others and reported as
ignored: the gate's own steps run them.
"""

import argparse
import json
import os
import posixpath
import re
import shlex
import subprocess
import sys
import tomllib
from dataclasses import dataclass, field
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
    also_paths: list[str] = field(default_factory=list)  # files the group reads: a change runs it, but they are named elsewhere


@dataclass
class Listed:
    """The tests of one test target: its names, the ones marked ignored, and the file it starts at."""

    tests: list[str]
    ignored: set[str]
    root: Path
    sources: set[str] = field(default_factory=set)  # the repository's Rust files rustc built the binary from (its dep-info)


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
        groups.append(Group(entry["name"], entry.get("paths", []), selectors, entry.get("before", []), entry.get("also_paths", [])))
    if len({g.name for g in groups}) != len(groups):
        raise Failure(f"{CONFIG.name}: group names must be unique")
    routes = [(route["paths"], route["groups"]) for route in config.get("route", [])]
    return groups, routes


def load() -> tuple[dict, list[Group], list[tuple[list[str], list[str]]]]:
    config = tomllib.loads(CONFIG.read_text())
    groups, routes = parse_config(config)
    return config, groups, routes


def baselines(config: dict) -> dict[str, str]:
    """Known failures of main, `test name # reason (owner: WHO; remove when: CONDITION)` per line (blank lines and lines starting
    with # are comments). Only failures that are deterministic on main belong here."""
    path = config.get("baselines")
    if not path:
        return {}
    found: dict[str, str] = {}
    for number, line in enumerate((ROOT / path).read_text().splitlines(), 1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        name, separator, reason = line.partition(" # ")
        if not separator or not name.strip() or " " in name.strip() or "owner:" not in reason or "remove when:" not in reason:
            raise Failure(f"{path}:{number}: expected `exact::test::name # reason (owner: WHO; remove when: CONDITION)`")
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
    and its segments; `use crate::{a, b::c}` gives one path per item, and a glob (`use super::*;`, `crate::a::*`) names its
    module (no segments for `super::*`: the parent module itself)."""
    found = []
    for match in re.finditer(r"\b(crate::|self::|(?:super::)+)(\{[^;]*?\}\s*;|\*|[A-Za-z_]\w*(?:::[A-Za-z_]\w*)*)", text):
        head, rest = match.groups()
        if rest == "*":
            found.append(("crate" if head == "crate::" else "self" if head == "self::" else str(head.count("super")), []))
            continue
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


def strip_comments(text: str) -> str:
    return re.sub(r"//[^\n]*|/\*.*?\*/", "", text, flags=re.DOTALL)


def declared_modules(text: str) -> list[tuple[str, str | None]]:
    """The out-of-line modules a source declares (`mod name;`), with the `#[path = "..."]` attribute they carry; the inline
    ones (`mod name { ... }`) hold no file."""
    found = []
    for match in re.finditer(r"((?:#\[[^\]]*\]\s*)*)(?:pub(?:\([^)]*\))?\s+)?\bmod\s+(\w+)\s*;", strip_comments(text)):
        attributes, name = match.groups()
        path = re.search(r'#\[path\s*=\s*"([^"]+)"\s*\]', attributes)
        found.append((name, path.group(1) if path else None))
    return found


def module_file(file: str, as_mod_rs: bool, inline: tuple[str, ...], name: str, path: str | None, available: set[str]) -> tuple[str, bool] | None:
    """The file of `mod name;` declared in `file` (inside the inline modules `inline`), and whether its own modules sit beside
    it (a mod.rs or a `#[path]` file); None when no such file exists."""
    directory = posixpath.dirname(file)
    base = posixpath.join(directory if as_mod_rs else posixpath.join(directory, Path(file).stem), *inline)
    if path:
        candidates = [(posixpath.normpath(posixpath.join(base if inline else directory, path)), True)]
    else:
        candidates = [(posixpath.join(base, name + ".rs"), False), (posixpath.join(base, name, "mod.rs"), True)]
    return next(((c, mod_rs) for c, mod_rs in candidates if c in available), None)


class ModuleTree:
    """The files of one crate by module, from its root file: `mod name;` finds `name.rs` or `name/mod.rs` beside the file (below
    its stem when the file is not a mod.rs, lib.rs or main.rs), and `#[path = "..."]` names the file from the declaring file's
    directory. `check` compares the files it reaches with the ones rustc says the crate is built from."""

    def __init__(self, root: str, read, available: set[str]):
        self.root = root
        self.children: dict[str, dict[str, str]] = {}
        self.parent: dict[str, str] = {}
        self.files = {root}
        todo = [(root, True)]
        while todo:
            file, as_mod_rs = todo.pop()
            kids: dict[str, str] = {}
            for name, path in declared_modules(read(file)):
                found = module_file(file, as_mod_rs, (), name, path, available)
                if found is None:
                    continue
                kids[name] = found[0]
                if found[0] not in self.files:
                    self.files.add(found[0])
                    self.parent[found[0]] = file
                    todo.append(found)
            self.children[file] = kids

    def walk(self, start: str, segments: list[str]) -> str:
        """The file of the deepest module the path names, from the module of `start`."""
        current = start
        for segment in segments:
            kid = self.children.get(current, {}).get(segment)
            if kid is None:
                break
            current = kid
        return current


def crate_trees(files: list[str], read) -> dict[str, ModuleTree]:
    """A module tree per crate root: the library and binary roots, and each integration test."""
    available = set(files)
    roots = [f for f in files if f.endswith(("src/main.rs", "src/lib.rs")) or re.fullmatch(r"(?:.*/)?tests/[^/]+\.rs", f)]
    return {root: ModuleTree(root, read, available) for root in roots}


def import_edges(trees: dict[str, ModuleTree], read) -> dict[str, set[str]]:
    """file -> the Rust files it imports directly (`crate::`, `self::` and `super::` paths), in each crate it belongs to."""
    edges: dict[str, set[str]] = {}
    for tree in trees.values():
        for path in tree.files:
            found = edges.setdefault(path, set())
            for kind, segments in paths_in(read(path)):
                if kind == "crate":
                    start = tree.root
                elif kind == "self":
                    start = path
                else:
                    start = path
                    for _ in range(int(kind)):
                        start = tree.parent.get(start, tree.root)
                target = tree.walk(start, segments)
                if target != path:
                    found.add(target)
    return edges


# ---------------------------------------------------------------------------------------------------------------- routing


class Router:
    """Which groups a change to a file runs."""

    def __init__(self, groups: list[Group], routes: list[tuple[list[str], list[str]]], derived: dict[str, set[str]] | None = None):
        self.names = [group.name for group in groups]
        self.owned = [([glob_regex(p) for p in group.paths], group.name) for group in groups]
        self.also = [([glob_regex(p) for p in group.also_paths], group.name) for group in groups]
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
        groups |= {name for patterns, name in self.also if any(p.match(path) for p in patterns)}
        return groups | self.derived.get(path, set())


def derive(router: Router, files: list[str], read, baseline_names: dict[str, str], baselines_path: str | None) -> dict[str, set[str]]:
    """The routes the code implies: a Rust file runs the groups of the test files that import it, a script a test file names runs
    that file's group, and the baselines file runs the groups of the tests it names."""
    derived: dict[str, set[str]] = {}
    edges = import_edges(crate_trees(files, read), read)
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
    """What an edit of the toml runs: every group when a global key moved, else the groups whose definition changed, and the
    tooling group; when the toml did not exist at the revision (it is being introduced), the tooling group only."""
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


def workspace_metadata() -> dict:
    """The packages of the workspace and their targets, as the manifests say (nothing resolved, fetched or built)."""
    return json.loads(subprocess.run(["cargo", "metadata", "--format-version", "1", "--no-deps", "--offline"], cwd=ROOT, capture_output=True, text=True, check=True).stdout)


def package_names() -> dict[str, str]:
    """Manifest path -> package name, for the packages of the workspace."""
    return {p["manifest_path"]: p["name"] for p in workspace_metadata()["packages"]}


def dep_info(executable: str) -> set[str]:
    """The repository's Rust files a test binary was built from, as rustc's dep-info (the `.d` beside it) names them: the
    ground truth the module tree is compared with."""
    path = Path(executable).with_suffix(".d")
    try:
        first = path.read_text().splitlines()[0]
    except (OSError, IndexError) as error:
        raise Failure(f"{path}: rustc's dep-info cannot be read ({error!r}), so which files the build holds is unknown") from error
    if ": " not in first:
        raise Failure(f"{path}: not rustc's dep-info (no `target: sources` line)")
    names = first.split(": ", 1)[1].replace("\\ ", "\0").split()
    found = {(Path(name.replace("\0", " "))).resolve() for name in names}
    return {str(p.relative_to(ROOT)) for p in found if p.is_relative_to(ROOT) and p.suffix == ".rs"}


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


def target_kind(target: dict) -> str:
    """A cargo target as the groups name it: "lib", "bin:NAME", "test:NAME"."""
    return "lib" if target["kind"][0] in LIBRARY_KINDS else f"{target['kind'][0]}:{target['name']}"


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
            kind = target_kind(target)
            directory = Path(message["manifest_path"]).parent
            tests, ignored = listing_of(message["executable"], directory)
            found[(features, package, kind)] = Listed(tests, ignored, Path(target["src_path"]), dep_info(message["executable"]))
            if kind == "lib" and target.get("doctest", False):
                doc = cargo_prefix(config, package, features) + ["--doc", "--", "--list", "--format", "terse"]
                done = subprocess.run(doc, cwd=ROOT, capture_output=True, text=True)
                if done.returncode != 0:
                    raise Failure(f"listing the doc tests of {package} failed:\n{done.stderr[-1000:]}")
                if tests_of(done.stdout):
                    found[(features, package, "doc")] = Listed(tests_of(done.stdout), set(), directory)
    return found


def list_unittest(directory: str, modules: list[str]) -> list[tuple[str, str]]:
    """(the module asked for, the test's id) for the tests of the modules; a module that imports another's test classes
    (`from merge_train.test_repin import RepinTests`) holds them under its own name, as `python -m unittest discover` runs them."""
    walk = (
        "import sys, unittest\n"
        "def walk(suite):\n"
        "    for test in suite:\n"
        "        yield from walk(test) if isinstance(test, unittest.TestSuite) else [test]\n"
        "sys.path.insert(0, sys.argv[1])\n"
        "for module in sys.argv[2:]:\n"
        "    for test in walk(unittest.defaultTestLoader.loadTestsFromName(module)):\n"
        "        print(module, test.id())\n"
    )
    found = subprocess.run([sys.executable, "-B", "-c", walk, directory, *modules], cwd=ROOT, capture_output=True, text=True)
    pairs = [tuple(line.split(" ", 1)) for line in found.stdout.splitlines() if line]
    broken = [n for _, n in pairs if n.startswith("unittest.loader.")]
    if found.returncode != 0 or broken:
        raise Failure(f"loading the Python tests {modules} of {directory} failed: {broken or found.stderr[-1000:]}")
    return pairs


def source_file(listed: Listed, kind: str, name: str, trees: dict[str, ModuleTree]) -> Path:
    """The file a test is written in: its module path walked down the module tree of the target's root file (`#[path]` and
    out-of-line modules included; an inline module stays in its file)."""
    if kind == "doc":
        return ROOT / name.split(" - ", 1)[0]
    root = str(listed.root.relative_to(ROOT)) if listed.root.is_relative_to(ROOT) else None
    tree = trees.get(root) if root else None
    if tree is None:
        return listed.root
    return ROOT / tree.walk(tree.root, name.split("::")[:-1])


def selects(selector: Selector, package: str, kind: str, name: str) -> bool:
    if selector.package != package or selector.target not in (None, kind):
        return False
    return not selector.filters or any(f in name for f in selector.filters)


# ---------------------------------------------------------------------------------------------------------------- source

TOKEN = re.compile(r"""
      \s+ | //[^\n]*
    | (?P<block>/\*)
    | (?P<raw>[bc]?r(?P<hashes>\#*)")
    | (?P<string>[bc]?"(?:\\.|[^"\\])*")
    | (?P<char>b?'(?:\\u\{[^}]*\}|\\.|[^'\\])')
    | (?P<lifetime>'[A-Za-z_]\w*)
    | (?P<token>[A-Za-z_]\w*|\d\w*|.)
""", re.VERBOSE | re.DOTALL)
IDENTIFIER = re.compile(r"[A-Za-z_]\w*")
COMMENT_MARK = re.compile(r"/\*|\*/")
# cfg keys that rustc's print of the host does not settle: debug assertions and overflow checks are the profile's, CPU features
# the flags'
FLAG_KEYS = ("debug_assertions", "overflow_checks", "target_feature")
FALSE_KEYS = ("doc", "doctest", "miri")  # never set in a test build


def tokens_of(text: str) -> list[str]:
    """The tokens of a Rust source: comments dropped, string and character literals kept whole (so neither a `#[test]` in a
    string nor a quote in a character literal is read as code)."""
    found = []
    position = 0
    while position < len(text):
        match = TOKEN.match(text, position)
        if match.group("block"):
            position = comment_end(text, match.end())
        elif match.group("raw"):
            close = text.find('"' + match.group("hashes"), match.end())
            position = len(text) if close < 0 else close + 1 + len(match.group("hashes"))
            found.append(text[match.start():position])
        else:
            position = match.end()
            if match.lastgroup:
                found.append(match.group())
    return found


def comment_end(text: str, position: int) -> int:
    """Where a block comment opened before `position` ends: Rust's block comments nest."""
    depth = 1
    while depth:
        found = COMMENT_MARK.search(text, position)
        if found is None:
            return len(text)
        depth += 1 if found.group() == "/*" else -1
        position = found.end()
    return position


def closing(tokens: list[str], start: int) -> int:
    """The index of the bracket that closes the one at `start` (the end of the tokens when none does)."""
    depth = 0
    for index in range(start, len(tokens)):
        depth += {"[": 1, "(": 1, "{": 1, "]": -1, ")": -1, "}": -1}.get(tokens[index], 0)
        if depth == 0:
            return index
    return len(tokens)


@dataclass
class SourceItems:
    """What one file holds for the test listing: its `#![cfg]`, its `#[test]` functions and its out-of-line modules, each with
    the inline modules around it and the cfgs written on it and on them (a cfg is the tokens inside `cfg(...)`)."""

    cfgs: list[tuple[str, ...]] = field(default_factory=list)
    tests: list[tuple[tuple[str, ...], str, tuple]] = field(default_factory=list)
    modules: list[tuple[tuple[str, ...], str, str | None, tuple]] = field(default_factory=list)  # inline, name, #[path], cfgs


def items_of(text: str) -> SourceItems:
    """The tests and modules of a source, read from its tokens. Only items of modules count: a `#[test]` in a function body,
    an impl or a macro body is no test libtest lists."""
    tokens = tokens_of(text)
    items = SourceItems()
    scopes: list[tuple[str, list] | None] = []  # each open brace: an inline module (its name and cfgs), or None
    attributes: list[list[str]] = []
    index = 0
    while index < len(tokens):
        token, ahead = tokens[index], tokens[index + 1 : index + 3]
        in_module = None not in scopes
        if token == "#" and (ahead[:1] == ["["] or ahead == ["!", "["]):
            inner = ahead[0] == "!"
            end = closing(tokens, index + 1 + inner)
            body = tokens[index + 2 + inner : end]
            if inner and body[:2] == ["cfg", "("] and in_module:
                (scopes[-1][1] if scopes else items.cfgs).append(tuple(body[2:-1]))
            elif not inner:
                attributes.append(body)
            index = end + 1
            continue
        if token in ("mod", "fn") and in_module and ahead and IDENTIFIER.fullmatch(ahead[0]):
            inline = tuple(name for name, _ in scopes)
            cfgs = (*(c for _, own in scopes for c in own), *(tuple(a[2:-1]) for a in attributes if a[:2] == ["cfg", "("]))
            if token == "fn" and ["test"] in attributes:
                items.tests.append((inline, ahead[0], cfgs))
            elif token == "mod" and ahead[1:] == [";"]:
                path = next((a[2][1:-1] for a in attributes if len(a) == 3 and a[:2] == ["path", "="]), None)
                items.modules.append((inline, ahead[0], path, cfgs))
            elif token == "mod" and ahead[1:] == ["{"]:
                scopes.append((ahead[0], [tuple(a[2:-1]) for a in attributes if a[:2] == ["cfg", "("]]))
                index += 1
            attributes = []
            index += 2
            continue
        if token == "{":
            scopes.append(None)
        elif token == "}" and scopes:
            scopes.pop()
        if token in ("{", "}", ";"):
            attributes = []
        index += 1
    return items


def cfg_value(tokens: tuple[str, ...], start: int, fact) -> tuple[bool | None, int]:
    """The value of the cfg predicate at `start` and the index after it; None: it cannot be told. `all`, `any` and `not` are
    three-valued, so `all(unix, something_unknown)` is still known false off unix."""
    name, after = tokens[start], tokens[start + 1 : start + 2]
    if after == ("=",):
        return fact(name, tokens[start + 2].strip('"')), start + 3
    if after != ("(",):
        return fact(name, None), start + 1
    values, index = [], start + 2
    while tokens[index] != ")":
        value, index = cfg_value(tokens, index, fact)
        values.append(value)
        if tokens[index] == ",":
            index += 1
    if name == "not" and len(values) == 1:
        return (None if values[0] is None else not values[0]), index + 1
    if name == "all":
        return (False if False in values else None if None in values else True), index + 1
    if name == "any":
        return (True if True in values else None if None in values else False), index + 1
    return None, index + 1


def cfg_holds(cfgs: tuple, fact) -> bool | None:
    """Whether every cfg holds; None when one cannot be told and none is false."""
    values = []
    for cfg in cfgs:
        try:
            value, end = cfg_value(cfg, 0, fact)
        except IndexError:  # a cfg this reader does not understand
            value, end = None, len(cfg)
        values.append(value if end == len(cfg) else None)
    return False if False in values else None if None in values else True


def host_cfg() -> set[tuple[str, str | None]]:
    """The cfgs of a native build for this machine, as rustc prints them (nothing is compiled), less the ones flags change."""
    done = subprocess.run([os.environ.get("RUSTC", "rustc"), "--print", "cfg"], cwd=ROOT, capture_output=True, text=True)
    if done.returncode != 0:
        raise Failure(f"rustc --print cfg failed, so which cfgs a native build has is unknown: {done.stderr[-500:]}")
    found = set()
    for line in done.stdout.splitlines():
        key, _, value = line.partition("=")
        if key not in FLAG_KEYS:
            found.add((key, value.strip('"') if value else None))
    return found


def profile_debug_assertions(manifest: dict, profile: str) -> bool | None:
    """Whether the profile the tests are built with has debug assertions (its `inherits` chain followed)."""
    profiles = manifest.get("profile", {})
    for _ in range(len(profiles) + 1):
        entry = profiles.get(profile, {})
        if "debug-assertions" in entry:
            return bool(entry["debug-assertions"])
        if "inherits" not in entry:
            return {"dev": True, "test": True, "release": False, "bench": False}.get(profile)
        profile = entry["inherits"]
    return None


def enabled_features(package: dict, requested: tuple[str, ...]) -> set[str]:
    """The features of a package a build turns on: the ones asked for (`NAME` when the package has it, `PACKAGE/NAME`),
    `default`, and the ones they turn on."""
    declared = package["features"]
    todo = [f.split("/", 1)[-1] for f in requested if "/" not in f or f.split("/", 1)[0] == package["name"]] + ["default"]
    found: set[str] = set()
    while todo:  # each feature is taken once
        feature = todo.pop()
        if feature in found or feature not in declared:
            continue
        found.add(feature)
        for value in declared[feature]:
            head = value.split("/", 1)[0]
            if not value.startswith("dep:") and not head.endswith("?"):
                todo.append(head)
    return found


def source_tests(root: str, read, available: set[str], parsed: dict[str, SourceItems]) -> tuple[list[tuple[str, tuple]], set[str]]:
    """The `#[test]` functions of the crate at `root` with their libtest names and every cfg above them, walking the modules
    from the root file; and the files reached."""
    found = []
    reached = {root}
    todo = [(root, True, (), ())]
    while todo:
        file, as_mod_rs, path, cfgs = todo.pop()
        if file not in parsed:
            parsed[file] = items_of(read(file))
        items = parsed[file]
        cfgs = (*cfgs, *items.cfgs)
        for inline, name, own in items.tests:
            found.append(("::".join((*path, *inline, name)), (*cfgs, *own)))
        for inline, name, attribute, own in items.modules:
            child = module_file(file, as_mod_rs, inline, name, attribute, available)
            if child is not None and child[0] not in reached:  # a file is one module, as the module tree takes it
                reached.add(child[0])
                todo.append((*child, (*path, *inline, name), (*cfgs, *own)))
    return found, reached


def list_source(config: dict, sets: list[tuple[str, ...]], read, files: list[str]) -> tuple[dict[tuple[tuple[str, ...], str, str], Listed], int]:
    """(features, package, target) -> its tests, read from the source for the test targets of each build, as `list_cargo` gives
    them from the binaries; and how many tests sit behind a cfg that cannot be told without building (left out)."""
    meta = workspace_metadata()
    members = set(meta.get("workspace_default_members") or meta["workspace_members"])
    host = host_cfg()
    manifest = tomllib.loads(Path(meta["workspace_root"], "Cargo.toml").read_text())
    debug = profile_debug_assertions(manifest, config.get("profile", "test-release"))
    host_keys = {key for key, _ in host}
    available, parsed = set(files), {}
    found: dict[tuple[tuple[str, ...], str, str], Listed] = {}
    unknown: set[tuple[str, str, str]] = set()
    for package in (p for p in meta["packages"] if p["id"] in members):
        for target in package["targets"]:
            source = Path(target["src_path"])
            if not target.get("test", True) or not source.is_relative_to(ROOT):
                continue
            kind = target_kind(target)
            tests, reached = source_tests(str(source.relative_to(ROOT)), read, available, parsed)
            for features in sets:
                enabled = enabled_features(package, features)
                if not set(target.get("required-features") or []) <= enabled:
                    continue

                def fact(key: str, value: str | None) -> bool | None:
                    if key in ("test", "feature", "debug_assertions"):
                        return {"test": value is None, "feature": value in enabled, "debug_assertions": debug}[key]
                    if key in host_keys or key in ("unix", "windows"):
                        return (key, value) in host
                    return False if key in FALSE_KEYS else None

                names = []
                for name, cfgs in tests:
                    holds = cfg_holds(cfgs, fact)
                    if holds is None:
                        unknown.add((package["name"], kind, name))
                    elif holds and name not in names:
                        names.append(name)
                found[(features, package["name"], kind)] = Listed(names, set(), source, reached)
    return found, len(unknown)


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


def baseline_groups(router: Router, names: list[str]) -> dict[str, set[str]]:
    """Baseline test name -> the groups of the files that hold the test (found by its function name)."""
    found: dict[str, set[str]] = {name: set() for name in names}
    for path in every_file():
        if not path.endswith(".rs"):
            continue
        text = (ROOT / path).read_text(errors="replace")
        for name in names:
            if re.search(r"\bfn\s+" + re.escape(name.split("::")[-1]) + r"\b", text):
                found[name] |= router.owners(path)
    return found


def unwaived(waived: dict[str, str], files: list[str] | None, router: Router, baselines_path: str | None = None) -> dict[str, str]:
    """The baselines to run after all: those whose group a changed file is routed to (or any file nothing names), so a change
    that reaches a waived test is never green by skipping it. An edit of the baselines file alone does not: it is how a test
    is waived."""
    files = [f for f in files or [] if f != baselines_path]
    if not files or not waived:
        return {}
    owners = baseline_groups(router, list(waived))
    routed = [router.route(f) for f in files]
    return {n: r for n, r in waived.items() if any(g is None or g & owners[n] for g in routed)}


def check_skips(waived: dict[str, str], names: list[str]) -> list[str]:
    """`--skip NAME` skips every test whose name contains NAME: a baseline must be a test and only that one."""
    errors = []
    names = sorted(set(names))  # a test of two builds is one test
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
    forced = unwaived(waived, files, router, config.get("baselines"))
    effective = {n: r for n, r in waived.items() if n not in forced}
    before, tests = commands(config, chosen, sorted(effective))
    record = Path(arguments.record) if arguments.record else None
    print("test groups: " + (" ".join(g.name for g in chosen) or "none"))
    if forced:
        print(f"   a changed file is routed to the group of {len(forced)} waived baseline(s), so they run: {', '.join(forced)}")
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
    cache: dict[str, str] = {}

    def read(path: str) -> str:
        if path not in cache:
            cache[path] = (ROOT / path).read_text(errors="replace")
        return cache[path]

    trees = crate_trees(files, read)
    errors: list[str] = check_routing(router, groups, routes, files)
    owners: dict[tuple[str, str, str, str], list[str]] = {}  # (package, target, features, test) -> its groups
    counts = {g.name: 0 for g in groups}
    hits: dict[tuple[str, int], int] = {}
    routed_files: list[tuple[Path, str, str]] = []
    homes: dict[tuple[str, str, str, str], str | None] = {}  # a test in no group -> its file
    skipped = set(arguments.skip_feature)
    builds, listings, unread = listings_for(arguments, config, groups, router, changed, read, files)
    for (features, package, kind), listed in sorted(listings.items()):
        errors += check_tree(trees, listed, f"{package} {kind}", "its source declares" if arguments.no_build else "rustc built it from")
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
            file = source_file(listed, kind, name, trees)
            if who:
                routed_files.append((file, who[0], f"{package} {kind} {name}"))
            else:
                homes[(package, kind, ",".join(features), name)] = str(file.relative_to(ROOT)) if file.is_relative_to(ROOT) else None
    errors += check_python(config, groups, files, owners, counts, routed_files)
    covered = {(package, kind, name) for (package, kind, features, name), who in owners.items() if who}
    for (package, kind, features, name), who in owners.items():
        label = f"{package} {kind}{' with ' + features if features else ''} {name}"
        if not who and (package, kind, name) not in covered:  # a test of two builds is in a group of either
            key = (package, kind, features, name)
            errors.append(f"in no group: {label} ({placement(router, owners, key, homes.get(key))})")
        elif len(set(who)) > 1:
            errors.append(f"in {len(set(who))} groups ({', '.join(sorted(set(who)))}) of one build: {label}")
    names = [key[3] for key in owners]
    errors += check_skips(baselines(config), names) if changed is None else []
    for group in groups:
        if counts[group.name] == 0 and not all(set(s.features) & skipped for s in group.selectors if s.package):
            errors.append(f"group {group.name} holds no test")
        for index, selector in enumerate(group.selectors):
            if set(selector.features) & skipped:
                continue
            if selector.package and changed is None and not hits.get((group.name, index)):
                errors.append(f"group {group.name}: {selector.package} {selector.target or '*'} {list(selector.filters)} selects no test")
            for filter_ in selector.filters:
                single = Selector(selector.package, selector.target, (filter_,), selector.features, (), None, ())
                if changed is None and not any(selects(single, p, k, n) for (p, k, f, n) in owners if f == ",".join(selector.features)):
                    errors.append(f"group {group.name}: filter {filter_!r} selects no test")
    errors += check_files(router, routed_files)
    if errors:
        print("\n".join(errors[:60]), file=sys.stderr)
        if len(errors) > 60:
            print(f"... and {len(errors) - 60} more", file=sys.stderr)
        return 1
    unique = {(p, k, n) for (p, k, _, n) in owners}
    left_out = [",".join(b) for b in builds if set(b) & skipped]
    if arguments.no_build:
        print(f"test groups (source level, nothing built): {len(unique)} tests in {len(groups)} groups, every file routed; {unread} tests "
              "behind a cfg this cannot evaluate were not checked; the listing of the built binaries is the merge check's")
    else:
        print(f"test groups: {len(unique)} tests in {len(groups)} groups, every file routed")
    if left_out:
        print(f"test groups: SKIPPED the builds with feature(s) {', '.join(sorted(skipped))} ({'; '.join(left_out)}): tests that exist only there are not checked")
    return 0


def listings_for(arguments: argparse.Namespace, config: dict, groups: list[Group], router: Router, changed: list[str] | None, read,
                 files: list[str]) -> tuple[list[tuple[str, ...]], dict[tuple[tuple[str, ...], str, str], Listed], int]:
    """The builds `check` looks at, their tests, and how many tests the source listing could not place. Reading the source
    costs no build, so `--no-build` reads every build the groups use."""
    skipped = set(arguments.skip_feature)
    if arguments.no_build:
        builds = feature_sets(config, groups)
        return builds, *list_source(config, [b for b in builds if not set(b) & skipped], read, files)
    builds = wanted_builds(config, groups, router, changed)
    return builds, list_cargo(config, [b for b in builds if not set(b) & skipped]), 0


def placement(router: Router, owners: dict[tuple[str, str, str, str], list[str]], key: tuple[str, str, str, str], path: str | None) -> str:
    """Where a test in no group goes: the group whose `paths` name its file, and the filter that takes it (its module, when
    no test of another group's would come along; else its full name). Said with the error, so the fix is one edit."""
    package, kind, features, name = key
    if path is None:
        return "its file is outside the repository: name the test in a group's filters"
    groups = sorted(router.owners(path))
    if len(groups) != 1:
        where = f"the groups {', '.join(groups)} all name" if groups else "no group's `paths` name"
        return f"{where} its file {path}: put the file in one group's `paths` and the test in that group's filters"
    module = name.rsplit("::", 1)[0] + "::" if "::" in name else name
    taken = any(module in other and who and groups[0] not in who for (p, k, f, other), who in owners.items()
                if (p, k, f) == (package, kind, features))
    filter_ = name if taken else module
    build = f"{package} {kind}" + (f" with {features}" if features else "")
    return f"its file {path} is in the `paths` of group {groups[0]}: add {filter_!r} to that group's filters for {build}"


def check_tree(trees: dict[str, ModuleTree], listed: Listed, label: str, built: str = "rustc built it from") -> list[str]:
    """The module tree reaches every file rustc built the binary from (or the source listing reached), or the routing it
    supports would miss a test's file."""
    if not listed.sources or not listed.root.is_relative_to(ROOT):
        return []
    tree = trees.get(str(listed.root.relative_to(ROOT)))
    if tree is None:
        return []
    return [f"{label}: {built} {f}, which the module tree of {tree.root} does not reach (an inline module that declares modules, an include!, or a "
            f"module declaration it cannot read)"
            for f in sorted(listed.sources - tree.files)]


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
        for module, name in list_unittest(directory, [m for m in grouped if f"{directory}/{m}.py" in files]):
            who = sorted({g.name for g in groups for s in g.selectors if s.unittest == directory and module in s.modules})
            owners[("python", directory, "", name)] = who
            for g in who:
                counts[g] += 1
            if who:
                routed_files.append((ROOT / directory / f"{module}.py", who[0], f"python {name}"))
    return errors


def check_files(router: Router, routed_files: list[tuple[Path, str, str]]) -> list[str]:
    """A test's file is in its group's `paths`, so a change to it runs the group."""
    errors = []
    seen = set()
    for file, group, label in routed_files:
        if not file.is_relative_to(ROOT) or (file, group) in seen:
            continue
        seen.add((file, group))
        path = str(file.relative_to(ROOT))
        if group not in router.owners(path):
            errors.append(f"{label} is written in {path}, which is not in the `paths` of its group {group}")
    return errors


def check_routing(router: Router, groups: list[Group], routes: list[tuple[list[str], list[str]]], files: list[str]) -> list[str]:
    errors = []
    for path in files:
        if router.route(path) is None:
            errors.append(f"no group or route names {path}")
        elif path.startswith("src/e2e/") and path.endswith(".rs") and not router.owners(path):
            errors.append(f"no group owns end-to-end module {path}")
    for group in groups:
        for pattern in [*group.paths, *group.also_paths]:
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
            sub.add_argument("--skip-feature", action="append", default=[], help="leave out the builds with this feature (a machine that cannot build it), and say so")
            sub.add_argument("--no-build", action="store_true", help="read the tests from the source instead of the built test binaries: builds nothing")
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
