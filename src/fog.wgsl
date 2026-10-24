// The standard material with the fog of war over it: outside the watched
// side's vision the lit colour is dulled, darkened and cooled. The terrain's
// also draws its path layers (layers.rs). The visual clarity aid sets the
// units apart from the scenery: the scenery a little duller and darker (the
// ground more so in the forest's shade), the units lit round their rims.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
}

// fog.rs, FogExt. The veil's alpha is how fogged a cell is; row 0 is the
// north edge of the map. params x: world units the veil spans; y: the alpha
// of a fully fogged cell; z: the surface (fog.rs, SURFACE_*); w: the visual
// clarity aid's strength. The cast is one texel: what the dulled colour is
// multiplied by, for the time of day. The paths texture's red is the dirt
// texture's brightness, green and blue the two stones'; 0.5 is the mean. The
// field's left half is the dirt's colour, its right half the stone's, a cell
// of FIELD_CELL world units a texel, row 0 the south edge; the left half's
// alpha is the forest's shade, the right half's which stone.
#ifdef BINDLESS
#import bevy_pbr::mesh_bindings::mesh
#import bevy_render::bindless::{bindless_samplers_filtering, bindless_textures_2d}

// Where each material's resources lie in the bindless arrays.
struct FogIndices {
    params: u32,
    veil_texture: u32,
    veil_sampler: u32,
    cast_texture: u32,
    paths_texture: u32,
    paths_sampler: u32,
    field_texture: u32,
}
struct FogUniform {
    params: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(110) var<storage> fog_uniforms: array<FogUniform>;
@group(#{MATERIAL_BIND_GROUP}) @binding(111) var<storage> fog_indices: array<FogIndices>;

// The drawn mesh's material, set by the fragment from its instance.
var<private> fog_material: FogIndices;

fn fog_params() -> vec4<f32> {
    return fog_uniforms[fog_material.params].params;
}
fn veil_at(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(bindless_textures_2d[fog_material.veil_texture], bindless_samplers_filtering[fog_material.veil_sampler], uv, 0.0);
}
fn day_cast() -> vec4<f32> {
    return textureLoad(bindless_textures_2d[fog_material.cast_texture], vec2(0, 0), 0);
}
fn paths_at(uv: vec2<f32>) -> vec4<f32> {
    return textureSample(bindless_textures_2d[fog_material.paths_texture], bindless_samplers_filtering[fog_material.paths_sampler], uv);
}
fn field_at(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(bindless_textures_2d[fog_material.field_texture], bindless_samplers_filtering[fog_material.veil_sampler], uv, 0.0);
}
fn field_size() -> vec2<f32> {
    return vec2<f32>(textureDimensions(bindless_textures_2d[fog_material.field_texture]));
}
#else
struct FogUniform {
    params: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> fog_uniform: FogUniform;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var veil_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var veil_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var cast_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var paths_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var paths_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var field_texture: texture_2d<f32>;

fn fog_params() -> vec4<f32> {
    return fog_uniform.params;
}
fn veil_at(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(veil_texture, veil_sampler, uv, 0.0);
}
fn day_cast() -> vec4<f32> {
    return textureLoad(cast_texture, vec2(0, 0), 0);
}
fn paths_at(uv: vec2<f32>) -> vec4<f32> {
    return textureSample(paths_texture, paths_sampler, uv);
}
fn field_at(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(field_texture, veil_sampler, uv, 0.0);
}
fn field_size() -> vec2<f32> {
    return vec2<f32>(textureDimensions(field_texture));
}
#endif

// layers.rs: FIELD_CELL, FIELD_RANGE.
const FIELD_CELL: f32 = 32.0;
const FIELD_RANGE: f32 = 1.5;
// World units one repeat of the dirt and the stone texture covers.
const DIRT_TILE: f32 = 288.0;
const STONE_TILE: f32 = 256.0;
// World units one repeat of the dirt texture covers when read as the noise
// that breaks a path's edge off the vertex grid's steps.
const EDGE_TILE: f32 = 1100.0;

// A path's share of a pixel: its vertex weight sharpened against the
// texture's brightness, so its edge follows the stones and the cracks, and
// against a slow noise, so it wanders off the vertex grid's steps.
fn sharpen(weight: f32, height: f32, noise: f32) -> f32 {
    return smoothstep(0.3, 0.7, weight + (height - 0.5) * 0.8 + (noise - 0.5) * 0.9);
}

// The field's texel coordinates of a bota point, in the left half.
fn field_uv(p: vec2<f32>) -> vec2<f32> {
    let n = field_size();
    let cells = p / FIELD_CELL;
    let half_texel = 0.5 / n;
    return clamp(cells / n, half_texel, vec2(0.5, 1.0) - half_texel);
}

// The terrain's colour with its dirt and stone drawn over the grass, given
// the field's dirt texel there.
fn with_paths(base: vec3<f32>, in: VertexOutput, dirt_field: vec4<f32>) -> vec3<f32> {
#ifdef VERTEX_UVS_B
    let p = vec2(in.world_position.x, -in.world_position.z);
    let dirt_h = paths_at(p / DIRT_TILE).r;
    let stones = paths_at(p / STONE_TILE).gb;
    let uv = field_uv(p);
    let dirt = dirt_field.rgb * FIELD_RANGE;
    let stone_field = field_at(uv + vec2(0.5, 0.0));
    let stone = stone_field.rgb * FIELD_RANGE;
    let stone_h = mix(stones.x, stones.y, stone_field.a);
    let noise = paths_at(p / EDGE_TILE + 0.37).r;
    let with_dirt = mix(base, dirt * dirt_h * 2.0, sharpen(in.uv_b.x, dirt_h, noise));
    return mix(with_dirt, stone * stone_h * 2.0, sharpen(in.uv_b.y, stone_h, 1.0 - noise));
#else
    return base;
#endif
}

// fog.rs, SURFACE_*.
const SURFACE_PATHS: f32 = 2.0;
const SURFACE_FOGGED_FIGURE: f32 = 3.0;
const SURFACE_FIGURE: f32 = 4.0;
// The rim light's colour at full strength, at a grazing view.
const RIM: vec3<f32> = vec3(0.16, 0.155, 0.14);

// The lit scenery set back behind what stands on it: a little duller and
// darker, darker still by the forest's shade, 0 to 1.
fn set_back(color: vec3<f32>, shade: f32) -> vec3<f32> {
    let strength = fog_params().w;
    let grey = dot(color, vec3(0.2126, 0.7152, 0.0722));
    let dull = mix(color, vec3(grey), 0.2 * strength);
    return dull * (1.0 - 0.16 * strength) * (1.0 - 0.3 * strength * shade);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
#ifdef BINDLESS
    fog_material = fog_indices[mesh[in.instance_index].material_and_lightmap_bind_group_slot & 0xffffu];
#endif
    let params = fog_params();
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    let figure = params.z >= SURFACE_FOGGED_FIGURE;
    // The forest's shade, on the ground that has the field.
    var shade = 0.0;
    if params.z == SURFACE_PATHS {
        let p = vec2(in.world_position.x, -in.world_position.z);
        let dirt_field = field_at(field_uv(p));
        shade = dirt_field.a;
        let base = pbr_input.material.base_color;
        pbr_input.material.base_color = vec4(with_paths(base.rgb, in, dirt_field), base.a);
    }

    let uv = vec2(in.world_position.x, params.x + in.world_position.z) / params.x;
    var fog = clamp(veil_at(uv).a / params.y, 0.0, 1.0);
    if params.z == SURFACE_FIGURE {
        fog = 0.0;
    }

    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    if params.w > 0.0 {
        if figure {
            let facing = saturate(dot(normalize(pbr_input.N), pbr_input.V));
            let rim = pow(1.0 - facing, 4.0) * params.w;
            let grey = dot(out.color.rgb, vec3(0.2126, 0.7152, 0.0722));
            let vivid = max(mix(vec3(grey), out.color.rgb, 1.0 + 0.2 * params.w), vec3(0.0));
            out.color = vec4(vivid + RIM * rim, out.color.a);
        } else {
            out.color = vec4(set_back(out.color.rgb, shade), out.color.a);
        }
    }
    let grey = dot(out.color.rgb, vec3(0.2126, 0.7152, 0.0722));
    // Most of its colour gone, then darkened and cast (fog.rs, `cast`).
    let dull = mix(out.color.rgb, vec3(grey), 0.92) * day_cast().rgb;
    out.color = vec4(mix(out.color.rgb, dull, fog), out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
