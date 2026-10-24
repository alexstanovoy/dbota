// A particle card: one instance per card, its four corners made from the instance's head and
// tail edges (two triangles). Premultiplied output: the colour times its alpha, and the share
// of what is behind it covers (1 over it, 0 added to it); or, for MOD2X, twice the colour times
// what is behind: a card's texture times its overbright, weighed away from grey by its colour
// (Source's spritecards), or a decal's texture times its tint (Source's projected decals).

#import bevy_render::view::View

@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var card_texture: texture_2d<f32>;
@group(1) @binding(1) var card_sampler: sampler;

struct Card {
    @builtin(vertex_index) index: u32,
    @location(0) head: vec3<f32>,
    @location(1) head_side: vec3<f32>,
    @location(2) tail: vec3<f32>,
    @location(3) tail_side: vec3<f32>,
    @location(4) head_color: vec4<f32>,
    @location(5) tail_color: vec4<f32>,
    @location(6) uv: vec4<f32>,
    @location(7) uv_next: vec4<f32>,
    // Frame blend, saturate, cover, the modulating overbright (negative: a decal).
    @location(8) extra: vec4<f32>,
};

struct Fragment {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) uv_next: vec2<f32>,
    @location(3) extra: vec4<f32>,
};

@vertex
fn vertex(card: Card) -> Fragment {
    // Corners 0 and 1 are the head's (left, right), 2 and 3 the tail's.
    var corners = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u);
    let corner = corners[card.index % 6u];
    let right = (corner & 1u) == 1u;
    let tail = corner >= 2u;
    let side = select(-1.0, 1.0, right);
    let position = select(card.head + side * card.head_side, card.tail + side * card.tail_side, tail);
    var out: Fragment;
    out.clip = view.clip_from_world * vec4<f32>(position, 1.0);
    out.color = select(card.head_color, card.tail_color, tail);
    out.uv = vec2<f32>(select(card.uv.x, card.uv.z, right), select(card.uv.y, card.uv.w, tail));
    out.uv_next = vec2<f32>(select(card.uv_next.x, card.uv_next.z, right), select(card.uv_next.y, card.uv_next.w, tail));
    out.extra = card.extra;
    return out;
}

@fragment
fn fragment(in: Fragment) -> @location(0) vec4<f32> {
    let texel = mix(
        textureSample(card_texture, card_sampler, in.uv),
        textureSample(card_texture, card_sampler, in.uv_next),
        in.extra.x,
    );
    let alpha = in.color.a * texel.a;
#ifdef MODULATE
    var weighed = in.color.rgb * texel.rgb;
    if in.extra.w >= 0.0 {
        weighed = mix(vec3<f32>(0.5), texel.rgb * in.extra.w, in.color.rgb);
    }
    return vec4<f32>(clamp(mix(vec3<f32>(0.5), weighed, alpha), vec3<f32>(0.0), vec3<f32>(1.0)), alpha);
#else
    var color = in.color.rgb * texel.rgb;
    if in.extra.y > 0.5 {
        color = clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    return vec4<f32>(color * alpha, alpha * in.extra.z);
#endif
}
