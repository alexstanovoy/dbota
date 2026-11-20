#!/usr/bin/env python3
"""Check native dependency feature reuse and the browser importer's graphics isolation.

The native comparison covers bare Cargo commands on the default members and the package
and feature selections used by the test groups. Each command's full set of host and target
variants is compared as a whole. The wasm importer is checked for graphics packages. The
webgpu, webgl2 and web-replay client builds are outside the feature comparison because they
intentionally select different features.
"""

from pathlib import Path
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
BUILD_EDGES = ("--edges", "normal,build")
TEST_EDGES = ("--edges", "normal,build,dev")


def native_invocations() -> dict[tuple[str, ...], str]:
    config = tomllib.loads(ROOT.joinpath("tools/test_groups.toml").read_text())
    default_features = config.get("features", [])
    invocations = {
        BUILD_EDGES: "build",
        (*BUILD_EDGES, "-p", "dbota"): "client build",
        TEST_EDGES: "test listing / all-targets lint",
        (*TEST_EDGES, "-p", "dbota"): "client tests",
        (*TEST_EDGES, "-p", "dbota_import"): "importer tests",
    }
    for group in config["group"]:
        for test in group["tests"]:
            if "package" not in test:
                continue
            package = test["package"]
            features = ",".join(sorted(test.get("features", default_features)))
            arguments = ("--features", features) if features else ()
            invocations.setdefault((*TEST_EDGES, *arguments), f"test listing ({features})")
            invocations.setdefault(
                (*TEST_EDGES, "-p", package, *arguments),
                f"{package} tests" + (f" ({features})" if features else ""),
            )
    return invocations


def features_for(label: str, arguments: tuple[str, ...]) -> dict[str, set[frozenset[str]]]:
    command = [
        "cargo", "tree", "--locked", "--offline", "--color", "never",
        "--prefix", "none", "--format", "{p}\t{f}", *arguments,
    ]
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    if result.returncode:
        raise ValueError(f"{label}: cargo tree failed\n{result.stderr.strip()}")
    features = {}
    for line in result.stdout.splitlines():
        if not line:
            continue
        package, separator, names = line.partition("\t")
        if not separator:
            raise ValueError(f"{label}: unrecognized cargo tree row: {line!r}")
        names = names.removesuffix(" (*)").split(",")
        features.setdefault(package, set()).add(frozenset(filter(None, names)))
    if not features:
        raise ValueError(f"{label}: cargo tree listed no packages")
    return features


def describe(feature_sets: frozenset[frozenset[str]]) -> str:
    return " or ".join("[" + ", ".join(names) + "]" for names in sorted(map(sorted, feature_sets)))


def package_label(package: str) -> str:
    # Cargo appends a source location to path and Git packages.
    return package.split(" (", 1)[0]


def main() -> int:
    try:
        by_package = {}
        for arguments, label in native_invocations().items():
            for package, features in features_for(label, arguments).items():
                by_package.setdefault(package, {}).setdefault(frozenset(features), []).append(label)
        # WebGPU and WebGL2 deliberately use separate graphs. The importer uses neither.
        browser_importer = features_for(
            "browser importer",
            (*BUILD_EDGES, "-p", "dbota_import_web", "--target", "wasm32-unknown-unknown"),
        )
    except (OSError, ValueError) as error:
        print(f"feature forks: {error}", file=sys.stderr)
        return 1
    forks = {package: variants for package, variants in by_package.items() if len(variants) > 1}
    for package, variants in sorted(forks.items()):
        print(package_label(package), file=sys.stderr)
        for features, labels in variants.items():
            print(f"  {', '.join(labels)}: {describe(features)}", file=sys.stderr)
    print(f"feature forks: {len(forks)} packages")
    graphics = sorted(
        package_label(package) for package in browser_importer
        if package.split()[0].startswith(("bevy", "wgpu", "naga", "winit"))
    )
    if graphics:
        print(f"browser importer pulls in graphics dependencies: {', '.join(graphics)}", file=sys.stderr)
    return int(bool(forks or graphics))


if __name__ == "__main__":
    sys.exit(main())
