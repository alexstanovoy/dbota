//! The menu's small state effect for a button-like node that is not a [`super::kit::Shade`]d
//! button (a card, a tile): under the pointer, or with the keyboard's or a controller's focus, it
//! grows a little about its middle and takes a ring. It is a transform and an outline, so no
//! neighbour moves, and it is raised over them while it is lit. How far it has come is the shared
//! [`Motion`]'s hover amount (eased over the same time as every other control's), which follows
//! the pointer and the focus alike.

use bevy::prelude::*;

use crate::theme;
use crate::ui::effects::{Motion, amount};

/// Lights the node under the pointer or with the focus, enlarged by `scale` at the most.
#[derive(Component)]
#[require(
    Motion,
    UiTransform,
    ZIndex,
    Outline = Outline::new(px(2), px(0), Color::NONE)
)]
pub struct Highlight {
    pub scale: f32,
}

pub fn highlight(
    mut nodes: Query<(
        &Highlight,
        Ref<Motion>,
        &mut UiTransform,
        &mut Outline,
        &mut ZIndex,
    )>,
) {
    for (light, motion, mut transform, mut outline, mut layer) in &mut nodes {
        if !motion.is_changed() {
            continue;
        }
        let lit = amount(motion.hover);
        let scale = Vec2::splat(1.0 + (light.scale - 1.0) * lit);
        if transform.scale != scale {
            transform.scale = scale;
        }
        let ring = theme::FOCUS.with_alpha(lit);
        if outline.color != ring {
            outline.color = ring;
        }
        layer.set_if_neq(ZIndex(i32::from(lit > 0.0)));
    }
}
