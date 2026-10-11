// The standard material with the fog of war over it: outside the watched
// side's vision the lit colour is dulled, darkened and cooled.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
}

// Alpha is how fogged a cell is; row 0 is the north edge of the map.
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var veil_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var veil_sampler: sampler;
// x: world units the image spans; y: the alpha of a fully fogged cell.
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var<uniform> veil_extent: vec4<f32>;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    let uv = vec2(in.world_position.x, veil_extent.x + in.world_position.z) / veil_extent.x;
    let fog = clamp(textureSampleLevel(veil_texture, veil_sampler, uv, 0.0).a / veil_extent.y, 0.0, 1.0);

    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    let grey = dot(out.color.rgb, vec3(0.2126, 0.7152, 0.0722));
    // Dota-dense: about two fifths of the seen ground's brightness on screen
    // (a tenth and a half in linear light), and most of its colour gone.
    let dull = mix(out.color.rgb, vec3(grey), 0.92) * vec3(0.135, 0.14, 0.16);
    out.color = vec4(mix(out.color.rgb, dull, fog), out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
