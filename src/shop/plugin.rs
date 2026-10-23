//! The shop's systems, and when they run.

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

/// The shop works once the match is known, and in the tactical phase before
/// it, where what is chosen is queued for the match's start.
pub fn shop_runs(
    known: Res<crate::session::MatchKnown>,
    tactical: Res<crate::select::TacticalPhase>,
) -> bool {
    known.0 || tactical.0
}
