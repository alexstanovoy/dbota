//! Indicator coverage is resolved in physical pixels after the legacy border has been laid out.
use bevy::{
    asset::embedded_asset, prelude::*, render::render_resource::AsBindGroup, shader::ShaderRef,
};

#[derive(Component, Clone, Copy, PartialEq)]
pub(crate) struct Indicator {
    pub color: Color,
    round: bool,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, PartialEq)]
pub(crate) struct Outline {
    #[uniform(0)]
    color: LinearRgba,
    #[uniform(0)]
    widths: Vec4,
    #[uniform(0)]
    shape: Vec4,
}

impl UiMaterial for Outline {
    fn fragment_shader() -> ShaderRef {
        "embedded://dbota/minimap/outline.wesl".into()
    }
}

pub(super) fn plugin(app: &mut App) {
    embedded_asset!(app, "outline.wesl");
    app.add_plugins(UiMaterialPlugin::<Outline>::default())
        .add_systems(PostUpdate, update.after(bevy::ui::UiSystems::Layout));
}

pub(super) fn bundle(round: bool, materials: &mut Assets<Outline>) -> impl Bundle + use<> {
    let indicator = Indicator {
        color: Color::WHITE,
        round,
    };
    (
        super::indicator_node(round),
        indicator,
        MaterialNode(materials.add(Outline {
            color: Color::WHITE.to_linear(),
            widths: Vec4::ONE,
            shape: Vec4::new(if round { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0),
        })),
    )
}

fn update(
    nodes: Query<(&ComputedNode, &Indicator, &MaterialNode<Outline>)>,
    mut materials: ResMut<Assets<Outline>>,
) {
    for (node, indicator, material) in &nodes {
        let border = node.border();
        // A one-pixel legacy line remains one pixel; wider borders keep fractional coverage.
        let half = |width: f32| {
            if width < 1.5 {
                width
            } else {
                width * crate::overlay::INDICATOR_SCALE
            }
        };
        let next = Outline {
            color: indicator.color.to_linear(),
            widths: Vec4::new(
                half(border.min_inset.x),
                half(border.min_inset.y),
                half(border.max_inset.x),
                half(border.max_inset.y),
            ),
            shape: Vec4::new(if indicator.round { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0),
        };
        if let Some(mut held) = materials.get_mut(&material.0)
            && *held != next
        {
            *held = next;
        }
    }
}
