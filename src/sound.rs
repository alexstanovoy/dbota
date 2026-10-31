//! Sounds: Dota's own from the cache, each cue of the client core matched to
//! one (a cue Dota has no sound for plays none); quieter the farther from the camera, under the
//! player's volume.

use std::collections::HashMap;

use bevy::{audio::Volume, prelude::*};
use bota_client_core::{Cue, CueKind};
use bota_proto::{HeroId, SlotId, Team, TeleportView, UnitKind};

use crate::camera::RtsCamera;
use crate::config::Settings;
use crate::manifest::Manifest;
use crate::session::Session;

/// World units from the camera within which a sound is heard at full.
const EARSHOT: f32 = 1200.0;
/// World units over which a sound fades out past [`EARSHOT`].
const FADE: f32 = 3000.0;
/// Seconds a kind of sound rests before it may sound again.
pub const REST: f32 = 0.05;
/// New missiles a tick looked into at most: each is matched against every
/// unit, and a snapshot may hold thousands of them.
const SHOTS_PER_TICK: usize = 32;

/// The gain of a melee creep's whoosh and blow: their events play at full
/// volume where a ranged creep's shot plays at 0.6, which sounds at 0.35 here.
const MELEE_GAIN: f32 = 0.6;

/// One kind of sound and the sources it may be played from.
struct Voice {
    /// The logical name of its row, for the trace.
    name: String,
    sources: Vec<Handle<AudioSource>>,
    rest: f32,
    next: usize,
}

/// Every kind of sound the client plays.
#[derive(Resource, Default)]
struct Voices(Vec<(Kind, Voice)>);

/// How many times a hero's death has been sounded by its own row, for the end-to-end tests.
#[cfg(test)]
pub(crate) fn deaths_sounded(world: &World, hero: HeroId) -> usize {
    let kind = row(Subject::Hero(hero), Event::Death, None);
    world
        .resource::<Voices>()
        .0
        .iter()
        .find(|(k, _)| *k == kind)
        .map_or(0, |(_, voice)| voice.next)
}

/// A kind of sound.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Cue(CueKind),
    /// One of Dota's refusals.
    Denied(crate::error_msg::Deny),
    /// A row of the manifest's sound bindings.
    Row(Row),
    /// The hum over a teleport's landing.
    PortalArriving,
    /// The hum where a hero is leaving.
    PortalLeaving,
    /// A hero landing from a teleport.
    HeroArrived,
    /// A hero gone by a teleport.
    HeroLeft,
    /// A ping heard, and a warning with the layer Dota plays over it.
    Ping,
    PingDanger,
    PingDangerLayer,
}

/// The sound of a row.
fn row(subject: Subject, event: Event, side: Option<Side>) -> Kind {
    Kind::Row(Row::new(subject, event, side))
}

/// What a sound row is of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Subject {
    MeleeCreep,
    RangedCreep,
    SiegeCreep,
    Tower,
    Hero(HeroId),
}

/// What happens to its subject, as Dota's sound events name it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Event {
    /// A swing beginning (`PreAttack`).
    PreAttack,
    /// A missile leaving or a blow landing (`Attack`).
    Attack,
    /// A missile landing (`ProjectileImpact`), or a hero's blow.
    Impact,
    /// A hero falling.
    Death,
}

/// A row of the manifest's sound bindings, the sound of an event of a kind
/// of unit, of one side's where Dota gives each its own:
/// `sound.<subject>.<event>[.<side>]`, every file that shares the name one of
/// its variants. A subject or event with no row is silent, as in Dota.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Row {
    subject: Subject,
    event: Event,
    side: Option<Side>,
}

impl Row {
    const fn new(subject: Subject, event: Event, side: Option<Side>) -> Row {
        Row {
            subject,
            event,
            side,
        }
    }

    /// The row's logical name, or none for a hero the manifest does not know.
    fn name(self, manifest: &Manifest) -> Option<String> {
        let subject = match self.subject {
            Subject::MeleeCreep => "creep.melee".to_owned(),
            Subject::RangedCreep => "creep.ranged".to_owned(),
            Subject::SiegeCreep => "creep.siege".to_owned(),
            Subject::Tower => "tower".to_owned(),
            Subject::Hero(hero) => format!("hero.{}", manifest.dota_name("hero", hero.0)?),
        };
        let event = match self.event {
            Event::PreAttack => "preattack",
            Event::Attack => "attack",
            Event::Impact => "impact",
            Event::Death => "death",
        };
        Some(match self.side {
            Some(side) => format!("sound.{subject}.{event}.{}", side.name()),
            None => format!("sound.{subject}.{event}"),
        })
    }

    /// Every row the client sounds.
    fn all() -> Vec<Row> {
        let mut rows = vec![
            Row::new(Subject::MeleeCreep, Event::PreAttack, None),
            Row::new(Subject::MeleeCreep, Event::Attack, None),
        ];
        for side in Side::BOTH {
            rows.extend([
                Row::new(Subject::RangedCreep, Event::Attack, Some(side)),
                Row::new(Subject::RangedCreep, Event::Impact, Some(side)),
                Row::new(Subject::SiegeCreep, Event::Attack, Some(side)),
                Row::new(Subject::Tower, Event::Attack, Some(side)),
            ]);
        }
        for hero in 0..bota_core::HEROES.len() {
            let subject = Subject::Hero(HeroId(hero as u16));
            for event in [Event::Attack, Event::Impact, Event::Death] {
                rows.push(Row::new(subject, event, None));
            }
        }
        rows
    }
}

/// The side a creep or a tower fights for: Dota gives each its own sound set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    Radiant,
    Dire,
}

impl Side {
    const BOTH: [Side; 2] = [Side::Radiant, Side::Dire];

    /// The side's part of a row's name.
    const fn name(self) -> &'static str {
        match self {
            Side::Radiant => "radiant",
            Side::Dire => "dire",
        }
    }

    /// The side of a team; the jungle's creeps and tower-less teams have none.
    fn of(team: Team) -> Option<Side> {
        match team {
            Team::Radiant => Some(Side::Radiant),
            Team::Dire => Some(Side::Dire),
            _ => None,
        }
    }
}

/// Dota's volumes of its ping events (`General.Ping`, `General.PingWarning`
/// and its second layer `PingWarning.Layer`, in game_sounds_ui), and the
/// layer's pitch, played as speed.
const PING_GAIN: f32 = 0.6;
const PING_DANGER_GAIN: f32 = 0.5;
const PING_LAYER_GAIN: f32 = 0.5;
const PING_LAYER_SPEED: f32 = 1.25;

/// How fast a kind is played: Dota's pitch where its event raises it.
fn speed_of(kind: Kind) -> f32 {
    match kind {
        Kind::PingDangerLayer => PING_LAYER_SPEED,
        _ => 1.0,
    }
}

/// The gain of a purchase sound, as the client core cues it.
const BUY_GAIN: f32 = 0.9;
/// Seconds the buy cues of the strategy phase's choices may still come after they are sent.
const QUEUED_BUYS_PATIENCE: f32 = 5.0;

/// The items the strategy phase chose, followed from frame to frame: a choice
/// sounds as it is made, and the buy cues of the orders that spend the choices
/// when the match begins are skipped.
#[derive(Default)]
struct QueuedBuys {
    len: usize,
    /// Buy cues still to be skipped, and when the last order was sent.
    spoken: usize,
    sent_at: f32,
}

impl QueuedBuys {
    /// Looks at the queue; whether a choice was just made, and how many buy
    /// cues are to be skipped.
    fn watch(&mut self, core: &bota_client_core::App, now: f32) -> (bool, usize) {
        let len = core.queued_buys.len();
        let chosen = core.in_tactical_phase() && len > self.len;
        if !core.in_tactical_phase() && len < self.len {
            self.spoken += self.len - len;
            self.sent_at = now;
        }
        if self.spoken > 0 && now - self.sent_at > QUEUED_BUYS_PATIENCE {
            self.spoken = 0;
        }
        self.len = len;
        (chosen, self.spoken)
    }
}

/// The loops of one teleport under way, and what its landing needs.
struct Humming {
    loops: Vec<Entity>,
    landing: (f32, f32),
    origin: Option<(f32, f32)>,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Voices>()
        .add_systems(Update, load.run_if(crate::loading::requested))
        .add_systems(PostUpdate, report.in_set(crate::loading::Report))
        .add_systems(Update, (volume, play, portals));
}

/// The logical names of the sounds a kind plays, each with every variant that shares it.
fn sources_of(kind: Kind, manifest: &Manifest) -> Vec<String> {
    let named = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
    match kind {
        Kind::Cue(cue) => match cue {
            CueKind::LastHit => named(&["sound.last_hit"]),
            CueKind::Deny => named(&["sound.deny"]),
            CueKind::Raze => named(&["sound.raze"]),
            CueKind::Requiem => named(&["sound.requiem"]),
            // Dota has no sound for these (its low-health cue is none, and its own-kill
            // notification is silent): they play none.
            CueKind::LowHp | CueKind::HeroKill => vec![],
            CueKind::Buy => named(&["sound.buy"]),
            CueKind::LevelUp => named(&["sound.level_up"]),
            CueKind::TowerShot => named(&["sound.tower.hero_impact"]),
            // A hero's own, sounded through its rows.
            CueKind::Attack | CueKind::Hit | CueKind::HeroDeath => vec![],
            // Each reason has its own sound (Kind::Denied).
            CueKind::Refused(_) | CueKind::Undeniable(_) => vec![],
        },
        Kind::Denied(deny) => named(deny.logical()),
        Kind::Row(row) => row.name(manifest).into_iter().collect(),
        Kind::PortalArriving => named(&["sound.portal.loop_appear"]),
        Kind::PortalLeaving => named(&["sound.portal.loop_disappear"]),
        Kind::HeroArrived => named(&["sound.portal.hero_appear"]),
        Kind::HeroLeft => named(&["sound.portal.hero_disappear"]),
        Kind::Ping => named(&["sound.ping.regular"]),
        Kind::PingDanger => named(&["sound.ping.danger"]),
        Kind::PingDangerLayer => named(&["sound.ping.warning_layer"]),
    }
}

fn load(mut voices: ResMut<Voices>, manifest: Res<Manifest>, server: Res<AssetServer>) {
    let kinds = [
        CueKind::LastHit,
        CueKind::Deny,
        CueKind::Raze,
        CueKind::Requiem,
        CueKind::Buy,
        CueKind::LevelUp,
        CueKind::TowerShot,
    ]
    .map(Kind::Cue)
    .into_iter()
    .chain([
        Kind::Denied(crate::error_msg::Deny::General),
        Kind::Denied(crate::error_msg::Deny::Cooldown),
        Kind::Denied(crate::error_msg::Deny::Mana),
        Kind::PortalArriving,
        Kind::PortalLeaving,
        Kind::HeroArrived,
        Kind::HeroLeft,
        Kind::Ping,
        Kind::PingDanger,
        Kind::PingDangerLayer,
    ])
    .chain(Row::all().into_iter().map(Kind::Row));
    for kind in kinds {
        let names = sources_of(kind, &manifest);
        let sources: Vec<Handle<AudioSource>> = names
            .iter()
            .flat_map(|name| manifest.sounds(name))
            .map(|path| server.load(path))
            .collect();
        voices.0.push((
            kind,
            Voice {
                name: names.join("|"),
                sources,
                rest: 0.0,
                next: 0,
            },
        ));
    }
}

/// The master volume from the settings, applied to sounds started after.
fn volume(settings: Res<Settings>, args: Res<crate::Args>, mut global: ResMut<GlobalVolume>) {
    if !settings.is_changed() {
        return;
    }
    let v = if settings.muted || args.mute {
        0.0
    } else {
        settings.volume
    };
    global.volume = Volume::Linear(v);
}

/// Plays the cues of this frame, and the shots of creeps and towers the
/// camera is near.
fn play(
    mut commands: Commands,
    mut session: NonSendMut<Session>,
    mut voices: ResMut<Voices>,
    time: Res<Time>,
    cam: Single<&RtsCamera>,
    settings: Res<Settings>,
    args: Res<crate::Args>,
    fog: Res<crate::fog::FogGrid>,
    mut last_ping: Local<Option<bota_proto::Heard>>,
    mut queued: Local<QueuedBuys>,
) {
    for (_, voice) in &mut voices.0 {
        voice.rest = (voice.rest - time.delta_secs()).max(0.0);
    }
    let core = &mut session.0;
    let cues: Vec<Cue> = std::mem::take(&mut core.cues);
    let silent = settings.muted || args.mute;
    let focus = Vec2::new(cam.focus.x, -cam.focus.z);
    let tick = core.view().map_or(0, |v| v.tick);
    let mut sound = |kind: Kind, gain: f32, at: Option<(f32, f32)>| {
        let fall = at.map_or(1.0, |at| fall_off(at, focus));
        let Some((_, voice)) = voices.0.iter_mut().find(|(k, _)| *k == kind) else {
            return;
        };
        if args.trace_feel {
            println!(
                "sound tick={tick} name={} sources={}",
                voice.name,
                voice.sources.len()
            );
        }
        if silent || voice.rest > 0.0 || voice.sources.is_empty() || fall * gain < 0.03 {
            return;
        }
        voice.rest = REST;
        let source = voice.sources[voice.next % voice.sources.len()].clone();
        voice.next += 1;
        commands.spawn((
            AudioPlayer::new(source),
            PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(gain * fall))
                .with_speed(speed_of(kind)),
        ));
    };
    // Dota sounds a purchase when it is made: once for each choice in the
    // strategy phase, and none for those the match then buys on its own.
    let (chosen, spoken) = queued.watch(core, time.elapsed_secs());
    if chosen {
        sound(Kind::Cue(CueKind::Buy), BUY_GAIN, None);
    }
    let mut quiet = spoken;
    for cue in cues {
        if cue.kind == CueKind::Buy && quiet > 0 {
            quiet -= 1;
            continue;
        }
        match cue.kind {
            // Dota sounds a refusal it tells, and only that.
            CueKind::Refused(reason) => {
                if let Some(told) = crate::error_msg::told(reason) {
                    sound(Kind::Denied(told.sound), cue.gain, None);
                }
            }
            CueKind::Undeniable(_) => sound(
                Kind::Denied(crate::error_msg::Deny::General),
                cue.gain,
                None,
            ),
            // A hero's attack, blow and fall are its own rows'.
            kind @ (CueKind::Attack | CueKind::Hit | CueKind::HeroDeath) => {
                let event = match kind {
                    CueKind::Attack => Event::Attack,
                    CueKind::Hit => Event::Impact,
                    _ => Event::Death,
                };
                if let Some(hero) = cue.hero {
                    sound(row(Subject::Hero(hero), event, None), cue.gain, cue.at);
                }
            }
            kind => sound(Kind::Cue(kind), cue.gain, cue.at),
        }
    }
    queued.spoken = quiet;
    shots(core, &fog, &mut sound);
    swings(core, &fog, &mut sound);
    // A ping is heard by its side wherever the camera is, as Dota's are.
    for kind in crate::pings::fresh(core, &mut last_ping) {
        match kind {
            bota_proto::PingKind::Regular => sound(Kind::Ping, PING_GAIN, None),
            bota_proto::PingKind::Danger => {
                sound(Kind::PingDanger, PING_DANGER_GAIN, None);
                sound(Kind::PingDangerLayer, PING_LAYER_GAIN, None);
            }
        }
    }
}

/// How much of its volume a sound at a place keeps for a listener at `focus`.
pub(crate) fn fall_off((x, y): (f32, f32), focus: Vec2) -> f32 {
    let far = Vec2::new(x, y).distance(focus);
    (1.0 - (far - EARSHOT).max(0.0) / FADE).clamp(0.0, 1.0)
}

/// Hums over every teleport under way, at the landing and, when the hero is
/// in sight, where it stands; a teleport the server tells landed has a sound
/// at both ends, one it tells was broken off, or that vanishes untold, only
/// goes quiet.
fn portals(
    mut commands: Commands,
    session: NonSend<Session>,
    voices: Res<Voices>,
    cam: Single<&RtsCamera>,
    settings: Res<Settings>,
    args: Res<crate::Args>,
    mut humming: Local<HashMap<SlotId, Humming>>,
) {
    let ends: Vec<(SlotId, bool)> = session
        .0
        .told
        .iter()
        .filter_map(|event| match *event {
            bota_proto::EventKind::Teleported { slot, landed } => Some((slot, landed)),
            _ => None,
        })
        .collect();
    let Some(view) = session.0.view() else {
        return;
    };
    let silent = settings.muted || args.mute;
    let focus = Vec2::new(cam.focus.x, -cam.focus.z);
    let play = |commands: &mut Commands, kind: Kind, at: (f32, f32), looped: bool| {
        let source = voices
            .0
            .iter()
            .find(|(k, _)| *k == kind)
            .and_then(|(_, voice)| voice.sources.first().cloned())?;
        let gain = fall_off(at, focus);
        if silent || gain < 0.03 {
            return None;
        }
        let playback = if looped {
            PlaybackSettings::LOOP
        } else {
            PlaybackSettings::DESPAWN
        };
        Some(
            commands
                .spawn((
                    AudioPlayer::new(source),
                    playback.with_volume(Volume::Linear(gain)),
                ))
                .id(),
        )
    };
    for (slot, landed) in ended_humming(humming.keys().copied(), &view.teleports, &ends) {
        let Some(hum) = humming.remove(&slot) else {
            continue;
        };
        for entity in hum.loops {
            commands.entity(entity).despawn();
        }
        if landed {
            play(&mut commands, Kind::HeroArrived, hum.landing, false);
            if let Some(origin) = hum.origin {
                play(&mut commands, Kind::HeroLeft, origin, false);
            }
        }
    }
    for teleport in &view.teleports {
        let landing = (
            teleport.destination.x.to_f32(),
            teleport.destination.y.to_f32(),
        );
        let hum = humming.entry(teleport.slot).or_insert_with(|| {
            let origin = crate::teleport::origin(view, teleport);
            let loops = [
                play(&mut commands, Kind::PortalArriving, landing, true),
                origin.and_then(|at| play(&mut commands, Kind::PortalLeaving, at, true)),
            ];
            Humming {
                loops: loops.into_iter().flatten().collect(),
                landing,
                origin,
            }
        });
        hum.origin = crate::teleport::origin(view, teleport).or(hum.origin);
    }
}

/// The seats whose teleport stops humming this frame, and whether the hero landed: first the
/// ends the server told, so that a scroll read on the very tick another ended starts a hum
/// of its own afterwards, then the humming that left the view with no end told, which only
/// goes quiet.
fn ended_humming(
    humming: impl Iterator<Item = SlotId>,
    teleports: &[TeleportView],
    told: &[(SlotId, bool)],
) -> Vec<(SlotId, bool)> {
    let untold = humming.filter(|slot| {
        told.iter().all(|(ended, _)| ended != slot) && !teleports.iter().any(|t| t.slot == *slot)
    });
    told.iter()
        .copied()
        .chain(untold.map(|slot| (slot, false)))
        .collect()
}

/// The shots of creeps and towers that let a missile go this tick.
fn shots(
    core: &bota_client_core::App,
    fog: &crate::fog::FogGrid,
    sound: &mut impl FnMut(Kind, f32, Option<(f32, f32)>),
) {
    let (Some(view), Some(prev)) = (core.view(), core.prev_view()) else {
        return;
    };
    if view.tick == prev.tick || core.smooth.since > 0.0 {
        return;
    }
    let new = view.projectiles.iter().filter(|p| {
        p.inflictor.is_none()
            && !fog.hides(p.seen_by)
            && prev
                .projectiles
                .binary_search_by_key(&p.id, |q| q.id)
                .is_err()
    });
    for p in new.take(SHOTS_PER_TICK) {
        let at = Some((p.pos.x.to_f32(), p.pos.y.to_f32()));
        let Some(shooter) = p.source.and_then(|id| view.unit(id)) else {
            continue;
        };
        let Some(side) = Side::of(shooter.team) else {
            continue;
        };
        match shooter.kind {
            UnitKind::Tower => sound(row(Subject::Tower, Event::Attack, Some(side)), 0.7, at),
            UnitKind::CreepRanged => sound(
                row(Subject::RangedCreep, Event::Attack, Some(side)),
                0.35,
                at,
            ),
            UnitKind::CreepSiege => sound(
                row(Subject::SiegeCreep, Event::Attack, Some(side)),
                0.25,
                at,
            ),
            _ => {}
        }
    }
    // A ranged creep's missile gone since the last tick has landed.
    for p in prev.projectiles.iter().filter(|p| {
        p.inflictor.is_none()
            && !fog.hides(p.seen_by)
            && view
                .projectiles
                .binary_search_by_key(&p.id, |q| q.id)
                .is_err()
    }) {
        let shooter = p.source.and_then(|id| prev.unit(id));
        if let Some(u) = shooter.filter(|u| u.kind == UnitKind::CreepRanged)
            && let Some(side) = Side::of(u.team)
        {
            let at = Some((p.pos.x.to_f32(), p.pos.y.to_f32()));
            sound(
                row(Subject::RangedCreep, Event::Impact, Some(side)),
                0.35,
                at,
            );
        }
    }
}

/// The swings of melee creeps in sight this tick, as Dota's sound sets tell
/// them: a whoosh as one begins, a blow as one lands. A swing gone before its
/// strike tick was broken off, and struck nothing.
fn swings(
    core: &bota_client_core::App,
    fog: &crate::fog::FogGrid,
    sound: &mut impl FnMut(Kind, f32, Option<(f32, f32)>),
) {
    let (Some(view), Some(prev)) = (core.view(), core.prev_view()) else {
        return;
    };
    if view.tick == prev.tick || core.smooth.since > 0.0 {
        return;
    }
    let swing_of = |of: &bota_proto::WorldView, id| {
        of.aggro
            .binary_search_by_key(&id, |a| a.unit)
            .ok()
            .and_then(|at| of.aggro[at].swing)
            .map(|swing| of.tick + swing.strike_in)
    };
    let melee = |u: &&bota_proto::UnitView| {
        matches!(u.kind, UnitKind::CreepMelee | UnitKind::CreepFlagbearer) && !fog.hides(u.seen_by)
    };
    for u in view.units.iter().filter(melee).take(SHOTS_PER_TICK) {
        let at = Some((u.pos.x.to_f32(), u.pos.y.to_f32()));
        let (now, before) = (swing_of(view, u.id), swing_of(prev, u.id));
        if now.is_some() && now != before {
            sound(
                row(Subject::MeleeCreep, Event::PreAttack, None),
                MELEE_GAIN,
                at,
            );
        }
        if before.is_some_and(|due| now != Some(due) && due <= view.tick) {
            sound(
                row(Subject::MeleeCreep, Event::Attack, None),
                MELEE_GAIN,
                at,
            );
        }
    }
}

/// The sounds' part of the loading.
fn report(voices: Res<Voices>, server: Res<AssetServer>, mut parts: ResMut<crate::loading::Parts>) {
    let sources = voices.0.iter().flat_map(|(_, voice)| &voice.sources);
    let (done, total) = sources.fold((0, 0), |(d, t), h| {
        (d + usize::from(crate::loading::settled(&server, h)), t + 1)
    });
    if total > 0 {
        parts.set("sounds", done, total);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bota_proto::{EntityId, Team};

    fn teleport(slot: u8) -> TeleportView {
        TeleportView {
            slot: SlotId(slot),
            team: Team::Radiant,
            building: EntityId {
                idx: 1,
                generation: 1,
            },
            destination: bota_proto::Vec2::ZERO,
            ticks_left: 10,
            ticks_total: 90,
        }
    }

    fn end(slot: u8, landed: bool) -> (SlotId, bool) {
        (SlotId(slot), landed)
    }

    #[test]
    fn a_hum_ends_as_the_server_told_or_quietly_when_it_leaves_the_view_untold() {
        let humming = [SlotId(0), SlotId(1), SlotId(2)];
        let cases = [
            (
                "nothing ended",
                vec![teleport(0), teleport(1), teleport(2)],
                vec![],
                vec![],
            ),
            (
                "told landed and gone",
                vec![teleport(1), teleport(2)],
                vec![end(0, true)],
                vec![(SlotId(0), true)],
            ),
            (
                "told broken off and gone",
                vec![teleport(1), teleport(2)],
                vec![end(0, false)],
                vec![(SlotId(0), false)],
            ),
            (
                "gone with no end told",
                vec![teleport(0), teleport(2)],
                vec![],
                vec![(SlotId(1), false)],
            ),
            (
                "told landed, and a scroll read on the same tick is in the view",
                vec![teleport(0), teleport(1), teleport(2)],
                vec![end(2, true)],
                vec![(SlotId(2), true)],
            ),
        ];
        for (case, teleports, told, expected) in cases {
            let ended = ended_humming(humming.into_iter(), &teleports, &told);
            assert_eq!(ended, expected, "{case}");
        }
    }

    /// A manifest whose every sound file is present, in a folder of its own.
    fn manifest_with_every_sound() -> Manifest {
        let root = std::env::temp_dir().join(format!("dbota-sound-rows-{}", std::process::id()));
        for line in crate::manifest::MANIFEST.lines() {
            let words: Vec<&str> = line.split_whitespace().collect();
            if let ["sound", _, _, path, ..] = words.as_slice() {
                let file = root.join(path);
                std::fs::create_dir_all(file.parent().expect("a folder")).expect("a folder");
                std::fs::write(file, b"").expect("a file");
            }
        }
        let manifest = Manifest::load(&root);
        let _ = std::fs::remove_dir_all(&root);
        manifest
    }

    /// The files a kind of sound plays.
    fn files_of(manifest: &Manifest, kind: Kind) -> Vec<String> {
        let names = sources_of(kind, manifest);
        names.iter().flat_map(|n| manifest.sounds(n)).collect()
    }

    /// Each sound plays the files of its own Dota event and no other's: a
    /// name is never the start of another's (the deny of a last hit rotated
    /// through the cooldown and mana refusals, every other Requiem sounded
    /// its cast), a side plays its own creeps' and towers' files, and a hero
    /// plays its own, none where Dota has none (Pudge died with Shadow
    /// Fiend's; his own fall has the sound of his Dota event).
    #[test]
    fn every_sound_resolves_to_exactly_the_files_of_its_event() {
        let manifest = manifest_with_every_sound();
        let one = |kind, expected: &str| {
            let files = files_of(&manifest, kind);
            assert_eq!(files.len(), 1, "{kind:?}: {files:?}");
            assert!(files[0].contains(expected), "{kind:?}: {files:?}");
        };
        one(Kind::Cue(CueKind::Deny), "sounds/ui/last_hit");
        one(Kind::Cue(CueKind::Requiem), "requiem_of_souls");
        let fiend = Subject::Hero(HeroId::SHADOW_FIEND);
        let pudge = Subject::Hero(HeroId::PUDGE);
        one(row(fiend, Event::Death, None), "shadowfiend_death");
        assert_eq!(
            files_of(&manifest, row(fiend, Event::Attack, None)).len(),
            2
        );
        one(row(pudge, Event::Death, None), "pudge/death");
        for event in [Event::Attack, Event::Impact] {
            assert!(files_of(&manifest, row(pudge, event, None)).is_empty());
        }
        let counts = [
            (
                Subject::MeleeCreep,
                Event::PreAttack,
                None,
                2,
                "shared/whoosh",
            ),
            (
                Subject::MeleeCreep,
                Event::Attack,
                None,
                3,
                "shared/melee_hit",
            ),
            (
                Subject::RangedCreep,
                Event::Attack,
                Some(Side::Radiant),
                2,
                "creep/good/",
            ),
            (
                Subject::RangedCreep,
                Event::Attack,
                Some(Side::Dire),
                2,
                "creep/bad/",
            ),
            (
                Subject::RangedCreep,
                Event::Impact,
                Some(Side::Radiant),
                2,
                "creep/good/",
            ),
            (
                Subject::RangedCreep,
                Event::Impact,
                Some(Side::Dire),
                2,
                "creep/bad/",
            ),
            (
                Subject::SiegeCreep,
                Event::Attack,
                Some(Side::Radiant),
                3,
                "creep/good/engine",
            ),
            (
                Subject::SiegeCreep,
                Event::Attack,
                Some(Side::Dire),
                1,
                "creep/bad/engine",
            ),
            (
                Subject::Tower,
                Event::Attack,
                Some(Side::Radiant),
                1,
                "attack_radiant",
            ),
            (
                Subject::Tower,
                Event::Attack,
                Some(Side::Dire),
                1,
                "attack_dire",
            ),
        ];
        for (subject, event, side, n, folder) in counts {
            let files = files_of(&manifest, row(subject, event, side));
            assert_eq!(files.len(), n, "{subject:?} {event:?} {side:?}: {files:?}");
            assert!(files.iter().all(|f| f.contains(folder)), "{files:?}");
        }
    }
}
