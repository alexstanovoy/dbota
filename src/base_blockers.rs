//! Permanent map fields whose visuals are created by the game, rather than a map model.

use bevy::prelude::*;
use bota_proto::Team;

use crate::particles::{EffectSight, Handle, Library, Playing};
use crate::session::{MatchKnown, Session};

#[derive(Resource, Default)]
struct BaseBlockers(Vec<(usize, Handle)>);

pub fn plugin(app: &mut App) {
    app.init_resource::<BaseBlockers>()
        .add_systems(First, crate::loading::reset::<BaseBlockers>.in_set(crate::loading::Begin))
        .add_systems(Update, stand.after(crate::particles::Loaded));
}

fn effect(team: Team) -> &'static str {
    match team {
        Team::Dire => "base_gate.dire_ambient",
        _ => "base_gate.radiant_ambient",
    }
}

fn stand(
    session: NonSend<Session>,
    known: Res<MatchKnown>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
    mut blockers: ResMut<BaseBlockers>,
) {
    let Some(view) = session.0.view() else {
        for (_, handle) in blockers.0.drain(..) { playing.cancel(handle); }
        return;
    };
    let Some(map) = session.0.session().map(|s| s.map()) else { return; };
    let Some(library) = library else { return; };
    // Installed game build 6975 starts the permanent field's ambient after 1.5 seconds.
    if !known.0 || view.tick < bota_core::rules::TICKS_PER_SECOND * 3 / 2 { return; }
    for (index, gate) in map.base_blockers.iter().enumerate() {
        if blockers.0.iter().any(|(held, _)| *held == index) { continue; }
        let Some(handle) = library.effect(effect(gate.team))
            .and_then(|effect| playing.hold(effect, index as u32, EffectSight::Point)) else { continue; };
        if let Some(instance) = playing.get_mut(handle) {
            place(instance, gate);
        }
        blockers.0.push((index, handle));
    }
}


fn place(instance: &mut dbota_particles::Instance, gate: &bota_core::BaseBlockerDef) {
    let angle = gate.facing.brads as f32 * std::f32::consts::TAU / 65536.0;
    let forward = Vec3::new(angle.cos(), angle.sin(), 0.0);
    let origin = Vec3::new(gate.at.x.to_f32(), gate.at.y.to_f32(), gate.height.to_f32());
    instance.set_control_point(0, origin + Vec3::Z * 32.0, Vec3::X, true);
    // CP1 is a source-world point used by the direction initializer, while CP2 is scalar data.
    let point = crate::coords::dota_to_sim(85.0, 10.0);
    instance.set_control_point(1, Vec3::new(point.x, point.y, 0.0), Vec3::X, true);
    instance.set_control_point(2, Vec3::splat(10000.0), Vec3::X, true);
    instance.set_control_point(3, origin + forward * 200.0, Vec3::X, true);
    instance.set_control_point(4, origin - forward * 200.0, Vec3::X, true);
}
