"""Errors that mean a renderer omitted work, shared by scene checks and frame timing."""
import re

RENDERER_ERROR = re.compile(
    r"wgpu|naga|bevy_render|bevy_pbr|bevy_core_pipeline|bevy_shader|bevy_ui_render"
    r"|[Ss]hader|[Pp]ipeline|Validation Error"
)
