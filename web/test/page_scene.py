"""`--scene-page`: a replay reaches the drawn forest and heroes on WebGPU and WebGL2, with a
visible canvas and no page error. A client panic fails the check immediately.
"""

import pathlib

from browser import check_scene_page

FLAG = "--scene-page"
# (POSITION in smoke.py's help, flags, add_argument's options): the page's flag first, then the options only it reads.
ARGUMENTS = (
    (20, ('--scene-page',), dict(type=pathlib.Path, help="a replay build: forest and heroes drawn on both graphics APIs")),
)
ORDER = 100


def run(args):
    check_scene_page(args.scene_page)
