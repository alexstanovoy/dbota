"""`--terrain-page`: full-map replay build without a seat POV.
"""

import pathlib

from browser import check_scene_page

FLAG = "--terrain-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (30, ('--terrain-page',), dict(type=pathlib.Path, help="full-map replay build without a seat POV")),
    (40, ('--terrain-api',), dict(choices=["webgpu", "webgl2"], action="append")),
    (50, ('--terrain-shots',), dict(type=pathlib.Path)),
    (60, ('--terrain-refused-shot',), dict(type=pathlib.Path, help="capture a match with an oversized ability icon")),
)
ORDER = 100


def run(args):
    if args.terrain_shots:
        args.terrain_shots.mkdir(parents=True, exist_ok=True)
    check_scene_page(
        args.terrain_page, args.terrain_api or ("webgpu", "webgl2"), terrain=True,
        shots=args.terrain_shots, refused_shot=args.terrain_refused_shot
    )
