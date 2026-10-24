// A particle card: one instance per card, its four corners made from the instance's head and
// tail edges (two triangles). Premultiplied output: the colour times its alpha, and the share
// of what is behind it covers (1 over it, 0 added to it); or, for MOD2X, twice the colour times
// what is behind: a card's texture times its overbright, weighed away from grey by its colour
// (Source's spritecards), or a decal's texture times its tint (Source's projected decals).

#import bevy_render::view::View

@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var card_texture: texture_2d<f32>;
@group(1) @binding(1) var card_sampler: sampler;
// The second texture layer's (the first's again for a look of one).
@group(1) @binding(2) var layer_texture: texture_2d<f32>;

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
    @location(9) uv2: vec4<f32>,
    @location(10) uv2_next: vec4<f32>,
    // The first layer's channel mix, the second's, how the second combines, the layers (1, 2).
    @location(11) layers: vec4<u32>,
};

struct Fragment {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) uv_next: vec2<f32>,
    @location(3) extra: vec4<f32>,
    @location(4) uv2: vec2<f32>,
    @location(5) uv2_next: vec2<f32>,
    @location(6) @interpolate(flat) layers: vec4<u32>,
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
    out.uv2 = vec2<f32>(select(card.uv2.x, card.uv2.z, right), select(card.uv2.y, card.uv2.w, tail));
    out.uv2_next = vec2<f32>(select(card.uv2_next.x, card.uv2_next.z, right), select(card.uv2_next.y, card.uv2_next.w, tail));
    out.layers = card.layers;
    return out;
}

fn luminance(c: vec4<f32>) -> f32 {
    return dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// A layer's colour and alpha by its channel mix (Source's spritecard order): `a` what the layers
// before it give, `b` its texel.
fn channels(mode: u32, a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    switch mode {
        case 0u: { return vec4<f32>(b.rgb, a.a); }
        case 2u: { return vec4<f32>(a.rgb, b.a); }
        case 3u: { return vec4<f32>(b.rgb, a.a * b.a); }
        case 4u: { return vec4<f32>(mix(a.rgb, b.rgb, b.a), a.a); }
        case 5u: { return vec4<f32>(mix(a.rgb, b.rgb, luminance(b)), a.a); }
        case 6u: { return vec4<f32>(b.rgb, luminance(b)); }
        case 7u: { return vec4<f32>(a.rgb, luminance(b)); }
        case 8u: { return vec4<f32>(a.rgb, a.a * luminance(b)); }
        case 9u: { return vec4<f32>(vec3<f32>(b.r), a.a); }
        case 10u: { return vec4<f32>(vec3<f32>(b.g), a.a); }
        case 11u: { return vec4<f32>(vec3<f32>(b.b), a.a); }
        case 12u: { return vec4<f32>(b.r); }
        case 13u: { return vec4<f32>(b.g); }
        case 14u: { return vec4<f32>(b.b); }
        default: { return b; }
    }
}

// A layer after the first combined with what is before it.
fn combine(mode: u32, before: vec4<f32>, layer: vec4<f32>) -> vec4<f32> {
    switch mode {
        case 1u: { return before * layer * 2.0; }
        case 2u: { return layer; }
        case 3u: { return before + layer; }
        case 4u: { return before - layer; }
        case 5u: { return (before + layer) * 0.5; }
        case 6u: { return vec4<f32>(before.rgb * luminance(layer), layer.a); }
        default: { return before * layer; }
    }
}

@fragment
fn fragment(in: Fragment) -> @location(0) vec4<f32> {
    let first = mix(
        textureSample(card_texture, card_sampler, in.uv),
        textureSample(card_texture, card_sampler, in.uv_next),
        in.extra.x,
    );
    let second = mix(
        textureSample(layer_texture, card_sampler, in.uv2),
        textureSample(layer_texture, card_sampler, in.uv2_next),
        in.extra.x,
    );
    // The layers mixed as Source's spritecards do, from white and opaque.
    var texel = channels(in.layers.x, vec4<f32>(1.0), first);
    if in.layers.w > 1u {
        texel = combine(in.layers.z, texel, channels(in.layers.y, texel, second));
    }
    texel = max(texel, vec4<f32>(0.0));
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
