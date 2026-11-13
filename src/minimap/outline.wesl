import bevy_ui_render::ui_vertex_output::UiVertexOutput;

struct Outline {
    color: vec4<f32>,
    widths: vec4<f32>,
    shape: vec4<f32>,
}
@group(1) @binding(0) var<uniform> material: Outline;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let point = in.uv * in.size;
    let distances = vec4(point.x, point.y, in.size.x - point.x, in.size.y - point.y);
    let edges = clamp(material.widths + vec4(0.5) - distances, vec4(0.0), vec4(1.0));
    var coverage = max(max(edges.x, edges.y), max(edges.z, edges.w));
    if material.shape.x > 0.5 {
        let radius = min(in.size.x, in.size.y) * 0.5;
        let distance = radius - length(point - in.size * 0.5);
        coverage = clamp(material.widths.y + 0.5 - distance, 0.0, 1.0)
            * clamp(distance + 0.5, 0.0, 1.0);
    }
    return vec4(material.color.rgb, material.color.a * coverage);
}
