// wash.rs: a picture through Dota's panel filters. levels x: the saturation
// kept; y: the brightness factor. The wash multiplies the result; the cut is
// the part of the picture drawn (corner, then size, in texture coordinates).

#import bevy_ui::ui_vertex_output::UiVertexOutput

struct Washed {
    levels: vec4<f32>,
    wash: vec4<f32>,
    cut: vec4<f32>,
}

@group(1) @binding(0) var<uniform> material: Washed;
@group(1) @binding(1) var picture: texture_2d<f32>;
@group(1) @binding(2) var picture_sampler: sampler;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let uv = material.cut.xy + in.uv * material.cut.zw;
    let color = textureSample(picture, picture_sampler, uv);
    let grey = dot(color.rgb, vec3(0.2126, 0.7152, 0.0722));
    let kept = mix(vec3(grey), color.rgb, material.levels.x) * material.levels.y;
    return vec4(kept * material.wash.rgb, color.a * material.wash.a);
}
