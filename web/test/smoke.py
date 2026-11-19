#!/usr/bin/env python3
"""Checks of the page in a headless Chromium (Playwright), each on a build named by its flag. The first run
(the import of a Dota 2 install in the client, its screens and refusals) is web/test/first_run.py's.

The pages that need no import play on a native import of this build (`dbota import`, DBOTA_ASSETS names it)
through `?cache=http`, served under /cache beside the page:

    DBOTA_WEB_REPLAY=$PWD/chat.brp DBOTA_WEB_ARGS="--replay built-in --pov 0 --seek 4600" \
        PROFILE=release OUT=temp/web-replay web/build.sh
    PROFILE=release OUT=temp/web-live web/build.sh
    CARGO_TARGET_DIR=temp/bota-target cargo build --release --manifest-path ../bota/Cargo.toml \
        -p bota_server --features websocket
    PYTHONPATH=temp/pwlib python3 web/test/smoke.py --replay-page temp/web-replay \
        --replays-page temp/web-live --server temp/bota-target/release/bota_server --radovid RADOVID \
        --startup-page temp/web-live

Each page's check is a module beside this script, web/test/page_<name>.py (flag `--<name>-page`): what it checks, its flag
and the options only it reads, and the check itself. A new page is a new module; this file does not change for it.
"""
import argparse
import importlib
import pathlib
import sys


# (POSITION in the help, flags, add_argument's options) of the options more than one page reads.
COMMON = (
    (80, ('--server',), dict(help="a bota_server built with the websocket feature, for --replays-page")),
    (250, ('--radovid',), dict(help="the radovid binary, for --replays-page (with --server)")),
    (260, ('--shots',), dict(action="store_true", help="save screenshots during the replay or fullscreen check")),
)


def pages():
    """The page modules beside this script, in the order their checks run."""
    here = pathlib.Path(__file__).resolve().parent
    sys.path.insert(0, str(here))
    found = [importlib.import_module(path.stem) for path in sorted(here.glob("page_*.py"))]
    return sorted(found, key=lambda page: (page.ORDER, page.__name__))


def main():
    ap = argparse.ArgumentParser()
    loaded = pages()
    arguments = [*COMMON, *(argument for page in loaded for argument in page.ARGUMENTS)]
    added = {}
    for position, flags, options in sorted(arguments, key=lambda argument: (argument[0], argument[1])):
        if flags in added:
            if added[flags] != options:
                sys.exit(f"{flags[0]} is declared twice with different options")
            continue
        added[flags] = options
        ap.add_argument(*flags, **options)
    args = ap.parse_args()
    for page in loaded:
        if getattr(args, page.FLAG[2:].replace("-", "_")):
            page.run(args)


if __name__ == "__main__":
    main()
