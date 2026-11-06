//! Every building Dota breaks, as it breaks: its model swapped for the baked destruction model,
//! which plays its death clip from the first frame, the fall sounded; once the clip has played the
//! building is gone for good (the destruction model ends with every piece below the ground), and
//! one that fell where the viewer did not see it, or before the viewer's replay began, is just
//! gone.

use std::sync::{Arc, Mutex};

use bevy::audio::AudioPlayer;
use bevy::prelude::*;
use bota_proto::{Cheat, EntityId, SlotId, Team, UnitKind, UnitView, Vec2};

use super::fog_pov::{Spectated, replay_observed};
use super::harness::Client;
use crate::units::{Dying, ModelBank, Unit};

/// The seat that watches its enemy's buildings fall.
const WATCHING_SEAT: u8 = 0;

/// Frames to play for the longest collapse (the Ancient's, 11.6 s) to be over, at 100 ms each.
const FRAMES_TO_COLLAPSE: usize = 130;

/// How far from the watching hero a building is still in its sight, in world units.
const SIGHT: i32 = 1500;

/// How a building is drawn while it falls: its model and clip.
struct Falling {
    model: String,
    clip: &'static str,
}

fn falling(client: &mut Client, id: EntityId) -> Option<Falling> {
    let world = client.world_mut();
    let (model, clip) = world
        .query_filtered::<(&Unit, &crate::anim::Pose, &Dying), ()>()
        .iter(world)
        .find(|(unit, ..)| unit.id == id)
        .map(|(unit, pose, _)| (unit.model, pose.clip()))?;
    let bank = world.resource::<ModelBank>();
    Some(Falling {
        model: bank.0[model?].model.logical.clone(),
        clip,
    })
}

/// The sound events started so far, by name.
#[derive(Clone, Default)]
struct Heard(Arc<Mutex<Vec<String>>>);

impl Heard {
    fn listen(client: &mut Client) -> Heard {
        let heard = Heard::default();
        let into = heard.clone();
        client.world_mut().add_observer(
            move |add: On<Add, AudioPlayer>, mixer: Res<crate::mixer::Mixer>| {
                if let Some((name, _)) = mixer.event_at(add.entity) {
                    into.0.lock().unwrap().push(name.to_owned());
                }
            },
        );
        heard
    }

    fn count(&self, event: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|n| *n == event)
            .count()
    }
}

/// The unit of a kind, among the given team's, nearest a spot.
fn nearest(view: &bota_proto::WorldView, team: Team, kind: UnitKind, to: Vec2) -> UnitView {
    view.units
        .iter()
        .filter(|u| u.team == team && u.kind == kind)
        .min_by_key(|u| u.pos.distance_squared(to))
        .unwrap_or_else(|| panic!("a {team:?} {kind:?}"))
        .clone()
}

/// The enemy's buildings: the Ancient and the tower, barracks and filler beside it, which the
/// hero that stands at the Ancient sees, and a tower at the other end of the lane, which it does
/// not.
struct Buildings {
    theirs: Team,
    seen: Vec<UnitView>,
    unseen: UnitView,
}

impl Buildings {
    /// The side of the enemy in the names of models and sounds.
    fn side(&self) -> &'static str {
        if self.theirs == Team::Dire {
            "dire"
        } else {
            "radiant"
        }
    }

    fn expected_model(&self, building: &UnitView) -> String {
        let map = bota_core::map_of(bota_proto::MapId(0));
        let side = self.side();
        match building.kind {
            UnitKind::Barracks => {
                let ranged =
                    bota_core::barracks_ranged_at(map, building.team, building.pos) == Some(true);
                format!(
                    "barracks.{side}.{}.destruction",
                    if ranged { "ranged" } else { "melee" }
                )
            }
            UnitKind::Tower => format!("tower.{side}.destruction"),
            UnitKind::Filler => format!("filler.{side}.destruction"),
            UnitKind::Ancient => format!("ancient.{side}.destruction"),
            other => panic!("{other:?} is no building that falls"),
        }
    }

    fn tower_sound(&self) -> &'static str {
        if self.theirs == Team::Dire {
            "Building_DireTower.Destruction"
        } else {
            "Building_RadiantTower.Destruction"
        }
    }

    fn ancient_sound(&self) -> &'static str {
        if self.theirs == Team::Dire {
            "Dire.ancient.Destruction"
        } else {
            "Radiant.ancient.Destruction"
        }
    }
}

/// A recorded match in which the watching seat's hero stands at its enemy's Ancient, and the
/// replay of the enemy's buildings falling, with the tick they are gone on.
fn record_the_buildings_falling(after: u32) -> (super::fog_pov::ReplayFile, Buildings, u32) {
    let mut game = Spectated::start_unrecorded("0");
    game.cheat(Cheat::Invulnerable { on: true });
    let watcher = SlotId(WATCHING_SEAT);
    let (theirs, ancient) = {
        let view = game.client.core().view().expect("the match");
        let hero = view
            .units
            .iter()
            .find(|u| u.kind == UnitKind::Hero && u.owner == Some(watcher))
            .expect("the watching seat's hero");
        let theirs = if hero.team == Team::Radiant {
            Team::Dire
        } else {
            Team::Radiant
        };
        (theirs, nearest(view, theirs, UnitKind::Ancient, hero.pos))
    };
    let (seen, unseen) = {
        let view = game.client.core().view().expect("the match");
        let near = |kind| nearest(view, theirs, kind, ancient.pos);
        let seen = vec![
            ancient.clone(),
            near(UnitKind::Tower),
            near(UnitKind::Barracks),
            near(UnitKind::Filler),
        ];
        let far = nearest(view, theirs, UnitKind::Tower, Vec2::from_ints(8000, 8000));
        (seen, far)
    };
    // Between the Ancient and the barracks, which is the farthest of them.
    let middle = Vec2::from_ints(
        (seen[0].pos.x.to_int() + seen[2].pos.x.to_int()) / 2,
        (seen[0].pos.y.to_int() + seen[2].pos.y.to_int()) / 2,
    );
    game.cheat(Cheat::Teleport { at: middle });
    game.wait("the spectator sees the hero among them", |_, client| {
        client
            .view()
            .units
            .iter()
            .any(|u| u.owner == Some(watcher) && u.pos.within(middle, bota_core::rules::units(100)))
    });
    for building in &seen {
        assert!(
            building.pos.within(middle, bota_core::rules::units(SIGHT)),
            "{:?} stands in the hero's sight",
            building.kind
        );
    }
    let ids: Vec<EntityId> = seen.iter().chain([&unseen]).map(|u| u.id).collect();
    let (file, gone) = game.save_after_falls(&ids, after);
    (
        file,
        Buildings {
            theirs,
            seen,
            unseen,
        },
        gone,
    )
}

/// Dire's tower, barracks, filler and Ancient fall in the destruction models of their own kinds,
/// each collapsing and sounded as Dota's events say, to a viewer who sees them fall; when the
/// clips are over nothing is left of any, drawn or hoverable. A tower that fell where the viewer
/// sees nothing is gone at once, though it too sounds, as Dota sounds a fall everywhere.
#[test]
fn buildings_collapse_in_their_destruction_models_to_the_viewer_who_sees_them_fall() {
    let (file, buildings, _) = record_the_buildings_falling(3);
    let mut heard = None;
    let mut client = replay_observed(&file, Some(WATCHING_SEAT), &[], |client| {
        heard = Some(Heard::listen(client));
    });
    let heard = heard.expect("listening");
    client.frames(2);
    for building in &buildings.seen {
        let drawn = falling(&mut client, building.id)
            .unwrap_or_else(|| panic!("the {:?} is collapsing", building.kind));
        assert_eq!(
            drawn.model,
            buildings.expected_model(building),
            "{:?}",
            building.kind
        );
        assert_eq!(drawn.clip, "death", "{:?}", building.kind);
    }
    assert!(
        falling(&mut client, buildings.unseen.id).is_none()
            && client.body(buildings.unseen.id).is_none(),
        "the tower that fell out of sight does not collapse before the viewer"
    );
    assert_eq!(
        heard.count(buildings.tower_sound()),
        1,
        "the tower in sight sounds, the one that fell out of it does not"
    );
    assert!(
        heard.count("Building_Generic.Destruction") >= 1,
        "the barracks and the filler sound (as one, when they fall together)"
    );
    assert_eq!(
        heard.count(buildings.ancient_sound()),
        1,
        "the Ancient sounds"
    );
    client.frames(FRAMES_TO_COLLAPSE);
    for building in buildings.seen.iter().chain([&buildings.unseen]) {
        assert!(
            falling(&mut client, building.id).is_none() && client.body(building.id).is_none(),
            "nothing is left of the {:?} once its clip has played",
            building.kind
        );
    }
}

/// A replay started past the fall shows the place as the fall left it, at once: the buildings
/// were never there to collapse.
#[test]
fn a_replay_started_past_the_fall_shows_no_collapse_and_no_building() {
    let settle = crate::capture::SETTLE_TICKS;
    let (file, buildings, gone) = record_the_buildings_falling(settle + 70);
    // Landing more than a second after the fall: sooner, the jump would show it falling.
    let past = (gone + settle + 60).to_string();
    let mut client = replay_observed(&file, Some(WATCHING_SEAT), &["--seek", &past], |_| {});
    client.frames(2);
    let tick = client.tick();
    for building in buildings.seen.iter().chain([&buildings.unseen]) {
        let collapsing = falling(&mut client, building.id).is_some();
        let drawn = client.body(building.id).is_some();
        let standing = client.view().unit(building.id).is_some();
        assert!(
            !collapsing && !drawn,
            "the {:?} is not drawn, nor collapsing: collapsing {collapsing}, drawn {drawn}, \
             in the view {standing}, at tick {tick} of a fall on {gone}",
            building.kind
        );
    }
}
