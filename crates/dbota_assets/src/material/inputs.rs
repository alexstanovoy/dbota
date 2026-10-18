//! Which glTF slot each of a material's textures feeds, and through which channels: the table
//! Source2Viewer 20 falls back to when it cannot read a game's compiled shaders (it cannot read
//! Dota's), and its rules for matching the shader's inputs to glTF's.

use super::vmat::Material;

/// Channels of a texture, by index (0 red .. 3 alpha).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Channels(pub &'static [u8]);

pub(super) const R: Channels = Channels(&[0]);
pub(super) const G: Channels = Channels(&[1]);
pub(super) const B: Channels = Channels(&[2]);
pub(super) const A: Channels = Channels(&[3]);
pub(super) const AG: Channels = Channels(&[3, 1]);
pub(super) const RGB: Channels = Channels(&[0, 1, 2]);
pub(super) const RGBA: Channels = Channels(&[0, 1, 2, 3]);

/// A glTF slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Slot {
    BaseColor,
    Normal,
    MetallicRoughness,
    SpecularFactor,
    Occlusion,
    Emissive,
}

/// Feed `slot` with channels `from` of the texture, as its channels `to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Remap {
    pub slot: Slot,
    pub from: Channels,
    pub to: Channels,
}

type Inputs = &'static [(Channels, &'static str)];

/// The inputs each glTF slot takes, in Source2Viewer's order.
const GLTF: [(Slot, Inputs); 6] = [
    (
        Slot::BaseColor,
        &[(RGB, "TextureColor"), (A, "TextureTranslucency")],
    ),
    (Slot::Normal, &[(RGB, "TextureNormal")]),
    (
        Slot::MetallicRoughness,
        &[(G, "TextureRoughness"), (B, "TextureMetalness")],
    ),
    (Slot::SpecularFactor, &[(A, "TextureSpecularMask")]),
    (Slot::Occlusion, &[(R, "TextureAmbientOcclusion")]),
    (Slot::Emissive, &[(RGB, "TextureSelfIllumMask")]),
];

/// Per shader, per texture parameter, the inputs its channels hold. An empty name is an input
/// that depends on the material's features.
const SHADERS: &[(&str, &[(&str, Inputs)])] = &[
    (
        "global_lit_simple",
        &[
            (
                "g_tColor",
                &[(RGB, "TextureColor"), (A, "TextureTranslucency")],
            ),
            ("g_tNormal", &[(RGB, "TextureNormal")]),
            (
                "g_tSpecular",
                &[
                    (R, "TextureReflectance"),
                    (G, "TextureSelfIllum"),
                    (B, "TextureBloom"),
                ],
            ),
        ],
    ),
    (
        "multiblend",
        &[
            ("g_tColor0", &[(RGB, "TextureColor0")]),
            (
                "g_tColor1",
                &[(RGB, "TextureColor1"), (A, "TextureRevealMask1")],
            ),
            (
                "g_tColor2",
                &[(RGB, "TextureColor2"), (A, "TextureRevealMask2")],
            ),
            (
                "g_tColor3",
                &[(RGB, "TextureColor3"), (A, "TextureRevealMask3")],
            ),
            ("g_tSpecular0", SPECULAR[0]),
            ("g_tSpecular1", SPECULAR[1]),
            ("g_tSpecular2", SPECULAR[2]),
            ("g_tSpecular3", SPECULAR[3]),
            (
                "g_tTintMasks",
                &[
                    (R, "TextureTintMask0"),
                    (G, "TextureTintMask1"),
                    (B, "TextureTintMask2"),
                    (A, "TextureTintMask3"),
                ],
            ),
            (
                "g_tTint2Masks",
                &[
                    (R, "TextureTint2Mask0"),
                    (G, "TextureTint2Mask1"),
                    (B, "TextureTint2Mask2"),
                    (A, "TextureTint2Mask3"),
                ],
            ),
        ],
    ),
    (
        "hero",
        &[
            (
                "g_tColor",
                &[(RGB, "TextureColor"), (A, "TextureTranslucency")],
            ),
            ("g_tNormal", &[(RGB, "TextureNormal")]),
            ("g_tCubeMap", &[(RGBA, "TextureCubeMap")]),
            (
                "g_tCubeMapSeparateMask",
                &[(G, "TextureCubeMapSeparateMask")],
            ),
            (
                "g_tFresnelWarp",
                &[
                    (R, "TextureFresnelWarpRim"),
                    (G, "TextureFresnelWarpColor"),
                    (B, "TextureFresnelWarpSpec"),
                ],
            ),
            (
                "g_tMasks1",
                &[
                    (R, "TextureDetailMask"),
                    (G, "TextureDiffuseWarpMask"),
                    (B, "TextureMetalnessMask"),
                    (A, "TextureSelfIllumMask"),
                ],
            ),
            (
                "g_tMasks2",
                &[
                    (R, "TextureSpecularMask"),
                    (G, "TextureRimMask"),
                    (B, "TextureTintByBaseMask"),
                    (A, "TextureSpecularExponent"),
                ],
            ),
            ("g_tDetail", &[(RGBA, "TextureDetail")]),
            ("g_tDetail2", &[(RGBA, "TextureDetail2")]),
        ],
    ),
    (
        "grasstile_preview",
        &[
            (
                "g_tColor",
                &[(RGB, "TextureColor"), (A, "TextureTranslucency")],
            ),
            ("g_tTintMask", &[(G, "TextureTintMask")]),
            ("g_tSpecular", &[(G, "TextureReflectance")]),
            ("g_tSelfIllum", &[(G, "TextureSelfIllum")]),
        ],
    ),
    (
        "generic",
        &[
            ("g_tColor", &[(RGB, "TextureColor")]),
            ("g_tNormal", &[(RGB, "TextureNormal")]),
            (
                "g_tMetalnessReflectanceFresnel",
                &[
                    (R, "TextureMetalness"),
                    (G, "TextureReflectance"),
                    (B, "TextureFresnel"),
                ],
            ),
            ("g_tRoughness", &[(R, "TextureRoughness")]),
        ],
    ),
    (
        "vr_standard",
        &[
            (
                "g_tColor",
                &[(RGB, "TextureColor"), (A, "TextureTranslucency")],
            ),
            ("g_tColor1", &[(RGB, "TextureColor")]),
            ("g_tColor2", &[(RGB, "TextureColor")]),
            ("g_tNormal", &[(RGB, "TextureNormal")]),
            ("g_tNormal1", &[(RGB, "TextureNormal")]),
            ("g_tNormal2", &[(RGB, "TextureNormal")]),
        ],
    ),
    (
        "vr_complex",
        &[
            ("g_tColor", &[(RGB, "TextureColor"), (A, "")]),
            (
                "g_tNormal",
                &[(RGB, "TextureNormal"), (A, "TextureRoughness")],
            ),
            ("g_tAmbientOcclusion", &[(R, "TextureAmbientOcclusion")]),
            ("g_tTintMask", &[(R, "TextureTintMask")]),
            ("g_tMetalness", &[(R, "TextureMetalness")]),
            ("g_tSelfIllumMask", &[(R, "TextureSelfIllumMask")]),
            ("g_tBentNormal", &[(RGB, "TextureBentNormal")]),
            ("g_tDetail", &[(RGB, "TextureDetail")]),
            ("g_tDetailMask", &[(R, "TextureDetailMask")]),
            ("g_tNormalDetail", &[(RGB, "TextureNormalDetail")]),
            ("g_tSquishColor", &[(RGB, "TextureSquishColor")]),
            ("g_tStretchColor", &[(RGB, "TextureStretchColor")]),
            ("g_tSquishNormal", &[(RGB, "TextureSquishNormal")]),
            ("g_tStretchNormal", &[(RGB, "TextureStretchNormal")]),
            (
                "g_tSquishAmbientOcclusion",
                &[(R, "TextureSquishAmbientOcclusion")],
            ),
            (
                "g_tStretchAmbientOcclusion",
                &[(R, "TextureStretchAmbientOcclusion")],
            ),
        ],
    ),
    (
        "vr_simple",
        &[
            ("g_tColor", &[(RGB, "TextureColor"), (A, "")]),
            (
                "g_tNormal",
                &[(RGB, "TextureNormal"), (A, "TextureRoughness")],
            ),
            ("g_tAmbientOcclusion", &[(R, "TextureAmbientOcclusion")]),
            ("g_tTintMask", &[(R, "TextureTintMask")]),
        ],
    ),
    (
        "vr_simple_2way_blend",
        &[
            (
                "g_tColorA",
                &[(RGB, "TextureColorA"), (A, "TextureMetalnessA")],
            ),
            (
                "g_tNormalA",
                &[(RGB, "TextureNormalA"), (A, "TextureRoughnessA")],
            ),
            (
                "g_tColorB",
                &[(RGB, "TextureColorB"), (A, "TextureMetalnessB")],
            ),
            (
                "g_tNormalB",
                &[(RGB, "TextureNormalB"), (A, "TextureRoughnessB")],
            ),
            ("g_tMask", &[(R, "TextureMask")]),
        ],
    ),
    (
        "vr_eyeball",
        &[
            (
                "g_tColor",
                &[(RGB, "TextureColor"), (A, "TextureReflectance")],
            ),
            ("g_tIris", &[(RGB, "IrisNormal"), (A, "IrisRoughness")]),
            ("g_tNormal", &[(AG, "TextureNormal")]),
            ("g_tIrisMask", &[(R, "TextureIrisMask")]),
            ("g_tSelfIllumMask", &[(R, "TextureSelfIllumMask")]),
        ],
    ),
    (
        "csgo_weapon",
        &[
            ("g_tColor", &[(RGB, "TextureColor")]),
            (
                "g_tMetalness",
                &[(R, "TextureRoughness"), (G, "TextureMetalness")],
            ),
            ("g_tAmbientOcclusion", &[(R, "TextureAmbientOcclusion")]),
        ],
    ),
    ("sky", &[("g_tSkyTexture", &[(RGBA, "SkyTexture")])]),
];

const SPECULAR: [Inputs; 4] = [
    &[
        (R, "TextureReflectance0"),
        (G, "TextureSelfIllum0"),
        (B, "TextureBloom0"),
    ],
    &[
        (R, "TextureReflectance1"),
        (G, "TextureSelfIllum1"),
        (B, "TextureBloom1"),
    ],
    &[
        (R, "TextureReflectance2"),
        (G, "TextureSelfIllum2"),
        (B, "TextureBloom2"),
    ],
    &[
        (R, "TextureReflectance3"),
        (G, "TextureSelfIllum3"),
        (B, "TextureBloom3"),
    ],
];

/// The glTF slots texture parameter `key` of `material` feeds.
pub(super) fn remaps(material: &Material, key: &str) -> Vec<Remap> {
    let inputs = inputs(material, key);
    let mut out = Vec::new();
    'slots: for (slot, wanted) in GLTF {
        let same = inputs.len() == wanted.len()
            && inputs
                .iter()
                .zip(wanted)
                .all(|((c, n), (wc, wn))| c == wc && same_input(n, wn));
        if same {
            out.push(Remap {
                slot,
                from: RGBA,
                to: RGBA,
            });
            break 'slots;
        }
        for (to, wanted_name) in wanted {
            for (from, name) in &inputs {
                if same_input(name, wanted_name) {
                    out.push(Remap {
                        slot,
                        from: *from,
                        to: *to,
                    });
                }
            }
        }
    }
    for (from, name) in &inputs {
        if same_input(name, "TextureMetalnessMask") {
            out.push(Remap {
                slot: Slot::MetallicRoughness,
                from: *from,
                to: B,
            });
        }
    }
    out
}

/// The inputs texture parameter `key` holds, by the shader's table; a parameter the table does
/// not know is one input of all four channels, named after it (`g_tMask` is `TextureMask`).
fn inputs(material: &Material, key: &str) -> Vec<(Channels, String)> {
    let shader = material
        .shader
        .rsplit_once('.')
        .map_or(material.shader.as_str(), |(stem, _)| stem);
    let known = SHADERS
        .iter()
        .find(|(name, _)| *name == shader)
        .and_then(|(_, params)| params.iter().find(|(k, _)| *k == key));
    let Some((_, list)) = known else {
        return vec![(RGBA, key.replace("g_t", "Texture"))];
    };
    list.iter()
        .filter_map(|(channels, name)| {
            let name = match *name {
                "" => feature_input(shader, key, material)?,
                name => name,
            };
            Some((*channels, name.to_owned()))
        })
        .collect()
}

/// What a channel holds that depends on the material's features.
fn feature_input(shader: &str, key: &str, material: &Material) -> Option<&'static str> {
    match (shader, key) {
        ("vr_simple", "g_tColor") if material.int("F_METALNESS_TEXTURE") != 0 => {
            Some("TextureMetalness")
        }
        ("vr_simple", "g_tColor") if material.int("F_AMBIENT_OCCLUSION_TEXTURE") != 0 => {
            Some("TextureAmbientOcclusion")
        }
        ("vr_complex", "g_tColor")
            if material.int("F_TRANSLUCENT") != 0 || material.int("F_ALPHA_TEST") != 0 =>
        {
            Some("TextureTranslucency")
        }
        ("vr_complex", "g_tColor") => Some("TextureMetalness"),
        _ => None,
    }
}

/// Whether input `layered` is glTF input `unlayered` or one of its layers (`TextureColor0`,
/// `TextureColor1`, `TextureColorA` and `TextureLayer1Color` are `TextureColor`). Either side
/// may be the glTF one; inputs glTF does not take match only themselves.
fn same_input(layered: &str, unlayered: &str) -> bool {
    if layered == unlayered {
        return true;
    }
    let gltf = |name: &str| {
        GLTF.iter()
            .any(|(_, list)| list.iter().any(|(_, n)| *n == name))
    };
    let (layered, unlayered) = match (gltf(unlayered), gltf(layered)) {
        (true, _) => (layered, unlayered),
        (false, true) => (unlayered, layered),
        (false, false) => return false,
    };
    let base = unlayered.strip_prefix("Texture").unwrap_or(unlayered);
    [
        format!("Texture{base}A"),
        format!("Texture{base}1"),
        format!("Texture{base}0"),
        format!("TextureLayer1{base}"),
    ]
    .iter()
    .any(|candidate| candidate == layered)
}
