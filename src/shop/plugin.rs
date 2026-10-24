//! The shop's systems.

use bevy::prelude::*;

pub fn plugin(app: &mut App) {
    app.add_plugins((
        super::cell::plugin,
        super::panel::plugin,
        super::tree::plugin,
        super::corner::plugin,
        super::clicks::plugin,
    ));
}
