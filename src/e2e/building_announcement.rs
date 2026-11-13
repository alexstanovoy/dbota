//! The announcer's call of a fallen tower or barracks, as a viewer hears it: the building's
//! destruction sounds at the fall and the call comes a little after it, never at the same
//! instant. The fall itself is not delayed: the building collapses at once.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::audio::AudioPlayer;
use bevy::prelude::*;
use bota_proto::{Cheat, EntityId, SlotId, Team, UnitKind, Vec2};

use super::fog_pov::{Spectated, replay_observed};
use super::harness::Client;

/// The seat that watches.
const WATCHING_SEAT: u8 = 0;
/// The least the call comes after the destruction sound that it is a pause and not one sound.
const AT_LEAST: Duration = Duration::from_secs(1);
/// The most the first call may come after it, to still be a "slight" delay.
const AT_MOST: Duration = Duration::from_secs(3);

/// What started sounding, with the viewer's clock: a sound event of the mixer by name, or an
/// announcer line by the file it plays.
type Sounded = Arc<Mutex<Vec<(Duration, String)>>>;

fn listen(client: &mut Client) -> Sounded {
    let sounded = Sounded::default();
    let into = sounded.clone();
    client.world_mut().add_observer(
        move |add: On<Add<AudioPlayer>>,
              players: Query<&AudioPlayer>,
              server: Res<AssetServer>,
              mixer: Res<crate::mixer::Mixer>,
              time: Res<Time>| {
            let name = match mixer.event_at(add.entity) {
                Some((event, _)) => event.to_owned(),
                None => {
                    let Ok(player) = players.get(add.entity) else {
                        return;
                    };
                    let Some(path) = server.get_path(player.0.id()) else {
                        return;
                    };
                    path.to_string()
                }
            };
            into.lock()
                .expect("unpoisoned")
                .push((time.elapsed(), name));
        },
    );
    sounded
}

fn first(sounded: &Sounded, part: &str) -> Option<Duration> {
    sounded
        .lock()
        .expect("unpoisoned")
        .iter()
        .find(|(_, name)| name.contains(part))
        .map(|(at, _)| *at)
}

/// The building of a kind and side nearest a spot.
fn nearest(
    view: &bota_proto::WorldView,
    team: Team,
    kind: UnitKind,
    to: Vec2,
) -> bota_proto::UnitView {
    view.units
        .iter()
        .filter(|u| u.team == team && u.kind == kind)
        .min_by_key(|u| u.pos.distance_squared(to))
        .unwrap_or_else(|| panic!("a {team:?} {kind:?}"))
        .clone()
}

/// A tower and a barracks of `team` fall (one tick after another in a recording of a match), the
/// watching seat beside them; the sounds the replay then makes, and the clip the tower collapses
/// in at its first frame.
fn fall_of(team: Team) -> (Sounded, bool, &'static str) {
    let mut game = Spectated::start_unrecorded("0");
    game.cheat(Cheat::Invulnerable { on: true });
    game.cheat(Cheat::AllVision { on: true });
    let (tower, barracks) = {
        let view = game.client.core().view().expect("the match");
        let ancient = view
            .units
            .iter()
            .find(|u| u.kind == UnitKind::Ancient && u.team == team)
            .expect("an Ancient")
            .pos;
        (
            nearest(view, team, UnitKind::Tower, ancient),
            nearest(view, team, UnitKind::Barracks, ancient),
        )
    };
    let middle = Vec2::from_ints(
        (tower.pos.x.to_int() + barracks.pos.x.to_int()) / 2,
        (tower.pos.y.to_int() + barracks.pos.y.to_int()) / 2,
    );
    game.cheat(Cheat::Teleport { at: middle });
    let watcher = SlotId(WATCHING_SEAT);
    game.wait("the spectator sees the hero among them", |_, client| {
        client
            .view()
            .units
            .iter()
            .any(|u| u.owner == Some(watcher) && u.pos.within(middle, bota_core::rules::units(100)))
    });
    let ids: Vec<EntityId> = vec![tower.id, barracks.id];
    let (file, _) = game.save_after_falls(&ids, 3, 0);
    let mut sounded = None;
    let mut client = replay_observed(&file, Some(WATCHING_SEAT), &[], |client| {
        sounded = Some(listen(client));
    });
    let sounded = sounded.expect("listening");
    // The fall is not delayed: the tower is collapsing in the first frames.
    client.frames(2);
    let collapsing = {
        let world = client.world_mut();
        world
            .query_filtered::<&crate::units::Unit, With<crate::units::Dying>>()
            .iter(world)
            .any(|unit| unit.id == tower.id)
    };
    let tower_sound = if team == Team::Dire {
        "Building_DireTower.Destruction"
    } else {
        "Building_RadiantTower.Destruction"
    };
    client.within(Duration::from_secs(120), "both calls spoken", |_| {
        first(&sounded, "announcer_twr_fall").is_some()
            && first(&sounded, "announcer_rax_fall").is_some()
    });
    (sounded, collapsing, tower_sound)
}

fn check(team: Team) {
    let (sounded, collapsing, tower_sound) = fall_of(team);
    assert!(collapsing, "the tower collapses at the fall, not later");
    let fell = first(&sounded, tower_sound).expect("the destruction sounded");
    let barracks_fell =
        first(&sounded, "Building_Generic.Destruction").expect("the barracks' fall sounded");
    let tower_call = first(&sounded, "announcer_twr_fall").expect("the tower's call");
    let barracks_call = first(&sounded, "announcer_rax_fall").expect("the barracks' call");
    assert!(
        tower_call >= fell + AT_LEAST,
        "the tower's call ({tower_call:?}) waits for its destruction sound ({fell:?})"
    );
    assert!(
        tower_call <= fell + AT_MOST,
        "and is still a slight delay: call {tower_call:?}, destruction {fell:?}"
    );
    assert!(
        barracks_call >= barracks_fell + AT_LEAST,
        "the barracks' call ({barracks_call:?}) waits for its destruction sound ({barracks_fell:?})"
    );
}

#[test]
fn a_fallen_dire_tower_and_barracks_are_called_after_their_destruction_sounds() {
    check(Team::Dire);
}

#[test]
fn a_fallen_radiant_tower_and_barracks_are_called_after_their_destruction_sounds() {
    check(Team::Radiant);
}
