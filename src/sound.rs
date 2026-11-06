//! Sounds: Dota's own from the cache, each cue of the client core matched to
//! one (a cue Dota has no sound for plays none); quieter the farther from the camera, under the
//! player's volume.

use std::collections::HashMap;
use std::time::Duration;

use bevy::{audio::Volume, prelude::*};
use bota_client_core::{Cue, CueKind};
use bota_proto::{EntityId, HeroId, SlotId, Team, TeleportView, UnitKind};

use crate::camera::RtsCamera;
use crate::config::Settings;
use crate::manifest::Manifest;
use crate::mixer::{Mixer, Owner, Request, Started};
use crate::session::Session;

/// New missiles a tick looked into at most: each is matched against every
/// unit, and a snapshot may hold thousands of them.
const SHOTS_PER_TICK: usize = 32;

/// One kind of sound and the sources it may be played from.
struct Binding {
    /// The logical name of its row, for the trace.
    name: String,
    sources: Vec<Handle<AudioSource>>,
    /// The Dota sound event each source plays, which the mixer's limits are of; empty where the
    /// manifest names none.
    events: Vec<String>,
    /// Heard on the hero the player plays alone (the manifest's `heard=own_hero`).
    own_hero_only: bool,
    next: usize,
}

/// Every kind of sound the client plays.
#[derive(Resource, Default)]
struct Bindings(Vec<(Kind, Binding)>);

/// How many times a hero's death has been sounded by its own row, for the end-to-end tests.
#[cfg(test)]
pub(crate) fn deaths_sounded(world: &World, hero: HeroId) -> usize {
    let kind = row(Subject::Hero(hero), Event::Death, None);
    world
        .resource::<Bindings>()
        .0
        .iter()
        .find(|(k, _)| *k == kind)
        .map_or(0, |(_, binding)| binding.next)
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
    /// The hero one plays coming into the range of a shop, and going out of it.
    ShopAvailable,
    ShopUnavailable,
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
    /// A barracks or a filler: Dota gives them one generic event.
    Building,
    Ancient,
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
    /// A building brought down.
    Destroyed,
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
            Subject::Building => "building".to_owned(),
            Subject::Ancient => "ancient".to_owned(),
            Subject::Hero(hero) => format!("hero.{}", manifest.dota_name("hero", hero.0)?),
        };
        let event = match self.event {
            Event::PreAttack => "preattack",
            Event::Attack => "attack",
            Event::Impact => "impact",
            Event::Death => "death",
            Event::Destroyed => "destroyed",
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
                Row::new(Subject::Tower, Event::Destroyed, Some(side)),
                Row::new(Subject::Ancient, Event::Destroyed, Some(side)),
            ]);
        }
        rows.push(Row::new(Subject::Building, Event::Destroyed, None));
        for hero in 0..bota_core::HEROES.len() {
            let subject = Subject::Hero(HeroId(hero as u16));
            for event in [Event::PreAttack, Event::Attack, Event::Impact, Event::Death] {
                rows.push(Row::new(subject, event, None));
            }
        }
        rows
    }
}

/// The row of the sound of the fall a building was told by.
fn fall_row(told: &bota_proto::EventKind) -> Option<Row> {
    let bota_proto::EventKind::StructureDestroyed { team, kind, .. } = *told else {
        return None;
    };
    let side = Side::of(team);
    Some(match kind {
        UnitKind::Tower => Row::new(Subject::Tower, Event::Destroyed, side),
        UnitKind::Ancient => Row::new(Subject::Ancient, Event::Destroyed, side),
        UnitKind::Barracks | UnitKind::Filler => {
            Row::new(Subject::Building, Event::Destroyed, None)
        }
        _ => return None,
    })
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

/// The pitch of the second layer of a danger ping (`PingWarning.Layer`), played as speed; the
/// volumes of the ping events are their own, the mixer's.
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
    landing_loop: Option<Entity>,
    origin_loop: Option<Entity>,
    landing: (f32, f32),
    origin: Option<crate::teleport::Origin>,
}

pub fn plugin(app: &mut App) {
    app.add_plugins(crate::mixer::plugin)
        .init_resource::<Bindings>()
        .init_resource::<PendingCues>()
        .add_systems(Update, load.run_if(crate::loading::requested))
        .add_systems(PostUpdate, report.in_set(crate::loading::Report))
        .add_systems(Update, (volume, (collect_cues, play).chain(), portals));
}

#[derive(Resource, Default)]
struct PendingCues(Vec<Cue>);

fn collect_cues(mut session: NonSendMut<Session>, mut pending: ResMut<PendingCues>) {
    pending.0.clear();
    pending.0.append(&mut session.0.cues);
}

fn cue_emitter(cue: &Cue) -> Option<EntityId> {
    if let Some(thrower) = &cue.thrower {
        return Some(thrower.id);
    }
    match cue.told.as_ref()? {
        bota_proto::EventKind::Damaged { source, target, .. } => {
            if cue.kind == CueKind::TowerShot {
                Some(*target)
            } else {
                *source
            }
        }
        bota_proto::EventKind::Died {
            unit,
            killer,
            credited_hero,
            ..
        } => {
            if cue.kind == CueKind::HeroDeath {
                Some(*unit)
            } else {
                credited_hero.as_deref().copied().or(*killer)
            }
        }
        bota_proto::EventKind::AbilityCast { caster, .. } => Some(*caster),
        bota_proto::EventKind::LevelUp { unit, .. } => Some(*unit),
        bota_proto::EventKind::StructureDestroyed { unit, .. } => Some(*unit),
        _ => None,
    }
}

/// A public fall says who died, without disclosing a position outside the selected sight.
fn permitted_cue(cue: &mut Cue, sight: &crate::fog::EventSight) -> bool {
    if let Some(thrower) = &cue.thrower {
        return sight.allows_unit(thrower);
    }
    let Some(event) = &cue.told else {
        // Local interface cues need no world subject; positional cues must have one.
        return cue.at.is_none();
    };
    if let bota_proto::EventKind::AbilityCast { caster, .. } = event {
        let Some(unit) = sight.core().view().and_then(|view| view.unit(*caster)) else {
            return false;
        };
        return sight.allows(event, crate::fog::EventSubject::Unit(unit));
    }
    if cue.kind == CueKind::HeroDeath {
        if !sight.allows(event, crate::fog::EventSubject::Event) {
            return false;
        }
        let witnessed = match event {
            bota_proto::EventKind::Died { unit, .. } => cue
                .before
                .as_ref()
                .and_then(|before| before.iter().find(|before| before.id == *unit)),
            _ => None,
        };
        let seen = witnessed.is_some_and(|unit| {
            sight.allows(event, crate::fog::EventSubject::Unit(unit))
                && cue.at == Some((unit.pos.x.to_f32(), unit.pos.y.to_f32()))
        });
        if !seen {
            cue.at = None;
        }
        return true;
    }
    cue.at.is_none_or(|at| {
        sight.allows(
            event,
            crate::fog::EventSubject::Point(bota_client_core::world_vec(at)),
        )
    })
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
            // A hero's own, sounded through its rows; so is a building's fall.
            CueKind::Attack | CueKind::Hit | CueKind::HeroDeath | CueKind::StructureFell => {
                vec![]
            }
            // Each reason has its own sound (Kind::Denied).
            CueKind::Refused(_) | CueKind::Undeniable(_) | CueKind::QuickBuyFull => vec![],
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
        Kind::ShopAvailable => named(&["sound.shop.available"]),
        Kind::ShopUnavailable => named(&["sound.shop.unavailable"]),
    }
}

fn load(mut bindings: ResMut<Bindings>, manifest: Res<Manifest>, server: Res<AssetServer>) {
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
        Kind::ShopAvailable,
        Kind::ShopUnavailable,
    ])
    .chain(Row::all().into_iter().map(Kind::Row));
    for kind in kinds {
        let names = sources_of(kind, &manifest);
        let files: Vec<(String, String)> = names
            .iter()
            .flat_map(|name| manifest.sounds_with_events(name))
            .collect();
        bindings.0.push((
            kind,
            Binding {
                name: names.join("|"),
                sources: files.iter().map(|(path, _)| server.load(path)).collect(),
                events: files.into_iter().map(|(_, event)| event).collect(),
                own_hero_only: names.iter().any(|n| manifest.own_hero_sounds.contains(n)),
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

/// The bindings that choose sources, the mixer that owns active playback, and the listener.
struct Sounder<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    mixer: &'a mut Mixer,
    bindings: &'a mut Bindings,
    now: Duration,
    focus: Vec2,
    silent: bool,
    /// The hero the player hears the damage taken of.
    ears: Option<EntityId>,
    /// The hero of the player's own seat: none for a spectator or a replay.
    own: Option<EntityId>,
    /// The fog of war, for the events that are heard only where their source is in sight.
    fog: &'a crate::fog::FogGrid,
    /// The tick to trace the decisions of, when they are traced.
    trace: Option<u32>,
}

impl Sounder<'_, '_, '_> {
    /// Sounds one of a kind's sounds at `gain` before the distance, where `at` is; blows on the
    /// unit `merge` names merge into one sound.
    fn sound(
        &mut self,
        kind: Kind,
        gain: f32,
        at: Option<(f32, f32)>,
        emitter: Option<EntityId>,
        merge: Option<EntityId>,
    ) {
        let Some((_, binding)) = self.bindings.0.iter_mut().find(|(k, _)| *k == kind) else {
            return;
        };
        if let Some(tick) = self.trace {
            println!(
                "sound tick={tick} name={} on={} heard={} sources={}",
                binding.name,
                merge.map_or("none".to_owned(), |unit| unit.idx.to_string()),
                !binding.own_hero_only || (merge.is_some() && merge == self.ears),
                binding.sources.len()
            );
        }
        // A blow taken as damage is heard on the hero one plays alone, so on no one's while
        // spectating or watching a replay.
        if binding.own_hero_only && (merge.is_none() || merge != self.ears) {
            return;
        }
        if self.silent || binding.sources.is_empty() {
            return;
        }
        let pick = binding.next % binding.sources.len();
        let factor_source = if matches!(
            kind,
            Kind::Row(Row {
                subject: Subject::Hero(_),
                event: Event::Attack,
                ..
            })
        ) {
            emitter
        } else {
            self.ears
        };
        let request = Request {
            event: if binding.events[pick].is_empty() {
                &binding.name
            } else {
                &binding.events[pick]
            },
            owner: Owner::None,
            emitter,
            merge,
            at,
            listener: Some((self.focus.x, self.focus.y)),
            gain,
            speed: speed_of(kind),
            on_player: sounds_from_the_players_entity(kind, self.own, factor_source),
            fog: Some(self.fog),
        };
        let source = binding.sources[pick].clone();
        if self.mixer.start(self.commands, self.now, &request, source) != Started::Refused {
            binding.next += 1;
        }
    }

    /// Sounds the cues of the client core, leaving `quiet` of the purchases unsounded; how many
    /// are still to be left.
    fn cues(
        &mut self,
        cues: impl Iterator<Item = Cue>,
        mut quiet: usize,
        sight: &crate::fog::EventSight,
    ) -> usize {
        for mut cue in cues {
            if !permitted_cue(&mut cue, sight) {
                continue;
            }
            if cue.kind == CueKind::Buy && quiet > 0 {
                quiet -= 1;
                continue;
            }
            let emitter = cue_emitter(&cue);
            match cue.kind {
                // Dota sounds a refusal it tells, and only that.
                CueKind::Refused(reason) => {
                    if let Some(told) = crate::error_msg::told(reason) {
                        self.sound(Kind::Denied(told.sound), cue.gain, None, emitter, None);
                    }
                }
                CueKind::Undeniable(_) | CueKind::QuickBuyFull => {
                    self.sound(
                        Kind::Denied(crate::error_msg::Deny::General),
                        cue.gain,
                        None,
                        emitter,
                        None,
                    );
                }
                // A hero's attack, blow and fall are its own rows'. The blows on one hero merge.
                kind @ (CueKind::Attack | CueKind::Hit | CueKind::HeroDeath) => {
                    let Some(hero) = cue.hero else {
                        continue;
                    };
                    let (event, merge) = match kind {
                        CueKind::Attack => (Event::Attack, None),
                        CueKind::Hit => (Event::Impact, cue.target),
                        _ => (Event::Death, None),
                    };
                    self.sound(
                        row(Subject::Hero(hero), event, None),
                        cue.gain,
                        cue.at,
                        emitter,
                        merge,
                    );
                }
                CueKind::StructureFell => {
                    if let Some(row) = cue.told.as_ref().and_then(fall_row) {
                        self.sound(Kind::Row(row), cue.gain, cue.at, emitter, None);
                    }
                }
                kind => self.sound(Kind::Cue(kind), cue.gain, cue.at, emitter, None),
            }
        }
        quiet
    }
}

/// Plays the cues of this frame, and the shots of creeps and towers the
/// camera is near.
#[allow(clippy::too_many_arguments)]
fn play(
    mut commands: Commands,
    sight: crate::fog::EventSight,
    mut pending: ResMut<PendingCues>,
    mut bindings: ResMut<Bindings>,
    mut mixer: ResMut<Mixer>,
    time: Res<Time>,
    cam: Single<&RtsCamera>,
    settings: Res<Settings>,
    args: Res<crate::Args>,
    mut last_ping: Local<Option<bota_proto::Heard>>,
    mut queued: Local<QueuedBuys>,
    mut shops: Local<ShopWatch>,
) {
    let core = sight.core();
    let (chosen, spoken) = queued.watch(core, time.elapsed_secs());
    let mut sounder = Sounder {
        commands: &mut commands,
        mixer: &mut mixer,
        bindings: &mut bindings,
        now: time.elapsed(),
        focus: Vec2::new(cam.focus.x, -cam.focus.z),
        silent: settings.muted || args.mute,
        ears: core.ears(),
        own: core.my_hero(),
        fog: sight.fog(),
        trace: args.trace_feel.then(|| core.view().map_or(0, |v| v.tick)),
    };
    // Dota sounds a purchase when it is made: once for each choice in the
    // strategy phase, and none for those the match then buys on its own.
    if chosen {
        sounder.sound(Kind::Cue(CueKind::Buy), BUY_GAIN, None, None, None);
    }
    queued.spoken = sounder.cues(pending.0.drain(..), spoken, &sight);
    match shop_change(core, &mut shops, time.elapsed_secs()) {
        Some(true) => sounder.sound(Kind::ShopAvailable, 1.0, None, None, None),
        Some(false) => sounder.sound(Kind::ShopUnavailable, 1.0, None, None, None),
        None => {}
    }
    shots(core, sight.fog(), &mut sounder);
    swings(core, sight.fog(), &mut sounder);
    // A ping is heard by its side wherever the camera is, as Dota's are.
    for kind in crate::pings::fresh(&sight, &mut last_ping) {
        match kind {
            bota_proto::PingKind::Regular => sounder.sound(Kind::Ping, 1.0, None, None, None),
            bota_proto::PingKind::Danger => {
                sounder.sound(Kind::PingDanger, 1.0, None, None, None);
                sounder.sound(Kind::PingDangerLayer, 1.0, None, None, None);
            }
        }
    }
}

struct PortalAudio<'a> {
    bindings: &'a Bindings,
    mixer: &'a Mixer,
    focus: Vec2,
    silent: bool,
}

impl PortalAudio<'_> {
    fn hum(&self, commands: &mut Commands, kind: Kind, at: (f32, f32)) -> Option<Entity> {
        let (_, binding) = self.bindings.0.iter().find(|(key, _)| *key == kind)?;
        let source = binding.sources.first().cloned()?;
        let gain = self
            .mixer
            .gain_for(binding.events.first()?, at, (self.focus.x, self.focus.y));
        if self.silent || gain < 0.03 {
            return None;
        }
        Some(
            commands
                .spawn((
                    crate::mixer::GameAudio,
                    AudioPlayer::new(source),
                    PlaybackSettings::LOOP.with_volume(Volume::Linear(gain)),
                ))
                .id(),
        )
    }
}

/// Hums over every teleport under way, at the landing and, when the hero is
/// in sight, where it stands; a teleport the server tells landed has a sound
/// at both ends, one it tells was broken off, or that vanishes untold, only
/// goes quiet.
fn portals(
    mut commands: Commands,
    sight: crate::fog::EventSight,
    mut bindings: ResMut<Bindings>,
    mut mixer: ResMut<Mixer>,
    time: Res<Time>,
    cam: Single<&RtsCamera>,
    settings: Res<Settings>,
    args: Res<crate::Args>,
    mut humming: Local<HashMap<SlotId, Humming>>,
) {
    let ends: Vec<(SlotId, bool)> = sight
        .core()
        .told
        .iter()
        .filter_map(|event| match *event {
            bota_proto::EventKind::Teleported { slot, landed } => Some((slot, landed)),
            _ => None,
        })
        .collect();
    let Some(view) = sight.core().view() else {
        return;
    };
    let silent = settings.muted || args.mute;
    let focus = Vec2::new(cam.focus.x, -cam.focus.z);
    {
        let core = sight.core();
        let mut sounder = Sounder {
            commands: &mut commands,
            mixer: &mut mixer,
            bindings: &mut bindings,
            now: time.elapsed(),
            focus,
            silent,
            ears: core.ears(),
            own: core.my_hero(),
            fog: sight.fog(),
            trace: None,
        };
        finish_portals(&mut sounder, core, &mut humming, &view.teleports, &ends);
    }
    let audio = PortalAudio {
        bindings: &bindings,
        mixer: &mixer,
        focus,
        silent,
    };
    follow_portals(&mut commands, &audio, &mut humming, view, &sight);
}

fn finish_portals(
    sounder: &mut Sounder,
    core: &bota_client_core::App,
    humming: &mut HashMap<SlotId, Humming>,
    teleports: &[TeleportView],
    ends: &[(SlotId, bool)],
) {
    for (slot, landed) in ended_humming(humming.keys().copied(), teleports, ends) {
        let Some(hum) = humming.remove(&slot) else {
            continue;
        };
        for entity in hum.landing_loop.into_iter().chain(hum.origin_loop) {
            sounder.commands.entity(entity).despawn();
        }
        if landed {
            let emitter = core.body_of(slot);
            sounder.sound(Kind::HeroArrived, 1.0, Some(hum.landing), emitter, None);
            if let Some(origin) = hum
                .origin
                .filter(|origin| !sounder.fog.hides(Some(origin.seen)))
            {
                sounder.sound(Kind::HeroLeft, 1.0, Some(origin.at), emitter, None);
            }
        }
    }
}

fn follow_portals(
    commands: &mut Commands,
    audio: &PortalAudio,
    humming: &mut HashMap<SlotId, Humming>,
    view: &bota_proto::WorldView,
    sight: &crate::fog::EventSight,
) {
    for teleport in &view.teleports {
        let landing = (
            teleport.destination.x.to_f32(),
            teleport.destination.y.to_f32(),
        );
        let hum = humming.entry(teleport.slot).or_insert_with(|| Humming {
            landing_loop: audio.hum(commands, Kind::PortalArriving, landing),
            origin_loop: None,
            landing,
            origin: None,
        });
        let origin = crate::teleport::origin(sight, teleport);
        // The hero can leave sight on the completion frame. Keep the last witnessed origin
        // for its departure, while the ongoing loop still follows current sight only.
        hum.origin = origin.or(hum.origin);
        match (origin, hum.origin_loop) {
            (None, Some(entity)) => {
                commands.entity(entity).despawn();
                hum.origin_loop = None;
            }
            (Some(origin), None) => {
                hum.origin_loop = audio.hum(commands, Kind::PortalLeaving, origin.at);
            }
            _ => {}
        }
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

/// Seconds a change of the hero's range of a shop must hold before it sounds, so a hero
/// walking along the edge of the range does not chime on every step.
const SHOP_SETTLE: f32 = 0.25;

/// What the sounds of the shops are told by: whether the hero one plays is in the range of a
/// shop, as of the last look.
#[derive(Default)]
struct ShopWatch {
    /// Where the hero was last known to stand: none before the first look of a match.
    near: Option<bool>,
    /// A change not yet held for [`SHOP_SETTLE`]: where it goes and since when.
    changing: Option<(bool, f32)>,
}

impl ShopWatch {
    /// Takes one look: `looked` is none while no match is in view (the watch forgets, so the next
    /// match's first look only learns where its hero stands) or while the hero lies dead (it
    /// keeps the state it fell in), else whether the hero is in the range of a shop. The change
    /// to sound, once it has held.
    fn look(&mut self, in_match: bool, looked: Option<bool>, now: f32) -> Option<bool> {
        if !in_match {
            *self = ShopWatch::default();
            return None;
        }
        let near = looked?;
        let Some(was) = self.near else {
            self.near = Some(near);
            return None;
        };
        if near == was {
            self.changing = None;
            return None;
        }
        let since = match self.changing {
            Some((to, since)) if to == near => since,
            _ => now,
        };
        self.changing = Some((near, since));
        if now - since < SHOP_SETTLE {
            return None;
        }
        self.near = Some(near);
        self.changing = None;
        Some(near)
    }
}

/// Whether a sound is from the entity of the player the game is played on, which Dota gives the
/// event's `volume_on_player_mult` for: the attack and blow rows of a hero are the watched hero's
/// own, and that is the player's only when the seat has a hero and it is the one watched.
fn sounds_from_the_players_entity(
    kind: Kind,
    own: Option<EntityId>,
    entity: Option<EntityId>,
) -> bool {
    let hero_blow = matches!(
        kind,
        Kind::Row(Row {
            subject: Subject::Hero(_),
            event: Event::Attack | Event::Impact,
            ..
        })
    );
    hero_blow && own.is_some() && own == entity
}

/// Whether the hero one plays has come into the range of a shop (`Some(true)`) or gone out of it
/// (`Some(false)`), as Dota sounds it: the home shop's, or a secret shop's (the courier's range
/// does not count).
fn shop_change(core: &bota_client_core::App, watch: &mut ShopWatch, now: f32) -> Option<bool> {
    let looked = core
        .my_standing_hero()
        .map(|hero| core.unit_at_home_shop(hero) || core.unit_at_secret_shop(hero));
    watch.look(core.view().is_some(), looked, now)
}

/// The shots of creeps and towers that let a missile go this tick.
fn shots(core: &bota_client_core::App, fog: &crate::fog::FogGrid, sound: &mut Sounder) {
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
        let subject = match shooter.kind {
            UnitKind::Tower => Subject::Tower,
            UnitKind::CreepRanged => Subject::RangedCreep,
            UnitKind::CreepSiege => Subject::SiegeCreep,
            _ => continue,
        };
        sound.sound(
            row(subject, Event::Attack, Some(side)),
            1.0,
            at,
            Some(shooter.id),
            None,
        );
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
            sound.sound(
                row(Subject::RangedCreep, Event::Impact, Some(side)),
                1.0,
                at,
                Some(u.id),
                p.target,
            );
        }
    }
}

/// Windup sounds for visible heroes and melee creeps, plus the creeps' strikes.
/// Hero impacts already come through their damage cues; sounding them here too
/// would duplicate a hit and sound canceled or missed blows.
fn swings(core: &bota_client_core::App, fog: &crate::fog::FogGrid, sound: &mut Sounder) {
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
    };
    let strike_of =
        |of: &bota_proto::WorldView, id| swing_of(of, id).map(|swing| of.tick + swing.strike_in);
    let melee = view
        .units
        .iter()
        .filter(|u| {
            matches!(u.kind, UnitKind::CreepMelee | UnitKind::CreepFlagbearer)
                && !fog.hides(u.seen_by)
        })
        .take(SHOTS_PER_TICK);
    // Hero windups do not consume the existing creep sound budget.
    let heroes = view
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Hero && !fog.hides(u.seen_by))
        .take(SHOTS_PER_TICK);
    for u in melee.chain(heroes) {
        let subject = match (u.kind, u.hero) {
            (UnitKind::Hero, Some(hero)) => Subject::Hero(hero),
            (UnitKind::CreepMelee | UnitKind::CreepFlagbearer, _) => Subject::MeleeCreep,
            _ => continue,
        };
        let at = Some((u.pos.x.to_f32(), u.pos.y.to_f32()));
        let (now, before) = (strike_of(view, u.id), strike_of(prev, u.id));
        if now.is_some() && now != before {
            sound.sound(
                row(subject, Event::PreAttack, None),
                1.0,
                at,
                Some(u.id),
                None,
            );
        }
        if subject == Subject::MeleeCreep
            && before.is_some_and(|due| now != Some(due) && due <= view.tick)
        {
            let target = swing_of(prev, u.id).map(|swing| swing.target);
            sound.sound(
                row(Subject::MeleeCreep, Event::Attack, None),
                1.0,
                at,
                Some(u.id),
                target,
            );
        }
    }
}

/// The sounds' part of the loading.
fn report(
    bindings: Res<Bindings>,
    server: Res<AssetServer>,
    mut parts: ResMut<crate::loading::Parts>,
) {
    let sources = bindings.0.iter().flat_map(|(_, binding)| &binding.sources);
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

    fn protocol_error(core: &bota_client_core::App) -> Option<&str> {
        core.feed.iter().rev().find_map(|line| match &line.kind {
            bota_client_core::FeedKind::ProtocolBroken { why } => Some(why.as_str()),
            _ => None,
        })
    }

    fn sound_policy_app(policy: &str) -> App {
        use clap::Parser;
        use std::sync::atomic::{AtomicUsize, Ordering};

        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "dbota-sound-policy-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("sounds")).expect("fixture folder");
        std::fs::write(root.join("sounds/events.txt"), policy).expect("fixture event");
        let mixer = Mixer::load(&Manifest {
            root: root.clone(),
            ..Manifest::default()
        });
        std::fs::remove_dir_all(root).expect("fixture removed after reading");
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<crate::fog::FogGrid>()
            .init_resource::<crate::main_menu::GameEpoch>()
            .init_resource::<PendingCues>()
            .insert_resource(Settings::default())
            .insert_resource(crate::Args::parse_from(["dbota"]))
            .insert_resource(mixer)
            .insert_resource(Bindings(vec![(
                row(Subject::Hero(HeroId::SHADOW_FIEND), Event::Attack, None),
                Binding {
                    name: "synthetic".to_owned(),
                    sources: vec![Handle::default()],
                    events: vec!["Synthetic.Attack".to_owned()],
                    own_hero_only: false,
                    next: 0,
                },
            )]))
            .insert_non_send(crate::main_menu::idle())
            .add_systems(Update, (collect_cues, play).chain());
        app.world_mut().spawn(RtsCamera {
            focus: Vec3::ZERO,
            distance: 1000.0,
        });
        crate::fog::init_test_event_sight(&mut app);
        app
    }

    fn scene_app(policy: &str) -> (App, [EntityId; 2]) {
        let (core, emitters) = crate::units::tests::owned_hero_scene();
        let mut app = sound_policy_app(policy);
        app.insert_non_send(Session(core));
        (app, emitters)
    }

    fn emit_projectiles(app: &mut App, emitters: &[EntityId]) -> Vec<Cue> {
        use bota_proto::{Angle, ProjectileView, ServerMsg};

        let mut session = app.world_mut().non_send_mut::<Session>();
        let mut view = session.0.view().expect("the scene").clone();
        view.tick += 1;
        for emitter in emitters {
            let unit = view.unit(*emitter).expect("the actual emitter");
            let (pos, team) = (unit.pos, unit.team);
            view.projectiles.push(ProjectileView {
                id: EntityId {
                    idx: 100 + view.projectiles.len() as u32,
                    generation: 1,
                },
                pos,
                facing: Angle::default(),
                team,
                ability: None,
                source: Some(*emitter),
                target: None,
                inflictor: None,
                seen_by: None,
                model: None,
            });
        }
        session.0.handle(ServerMsg::Snapshot { view });
        assert!(
            !session.0.lost,
            "valid projectile snapshot: {:?}",
            protocol_error(&session.0)
        );
        session.0.cues.clone()
    }

    #[test]
    fn separated_emitters_of_one_row_sound_in_the_same_frame() {
        let (mut app, emitters) =
            scene_app("event Synthetic.Attack 0 120 0 0.05 100 0 0 0 -1 1 70 10000 - 1 -1\n");
        emit_projectiles(&mut app, &emitters);
        app.update();
        let world = app.world_mut();
        assert_eq!(
            world.query::<&AudioPlayer>().iter(world).count(),
            2,
            "the imported repeat distance permits both source positions"
        );
    }

    #[test]
    fn a_portal_landing_cannot_exceed_the_world_voice_budget() {
        let mut world = World::new();
        let mut mixer = Mixer::default();
        let mut sources = Assets::<AudioSource>::default();
        let source = sources.add(crate::mixer::two_minute_source());
        for index in 0..crate::mixer::MAX_VOICES {
            let event = format!("gameplay{index}");
            mixer.start(
                &mut world.commands(),
                Duration::ZERO,
                &Request {
                    event: &event,
                    owner: Owner::None,
                    merge: None,
                    at: None,
                    gain: 1.0,
                    speed: 1.0,
                    ..Request::default()
                },
                source.clone(),
            );
        }
        let mut bindings = Bindings(vec![(
            Kind::HeroArrived,
            Binding {
                name: "landing".to_owned(),
                sources: vec![source],
                events: vec!["Synthetic.Landing".to_owned()],
                own_hero_only: false,
                next: 0,
            },
        )]);
        let fog = crate::fog::FogGrid::default();
        {
            let mut commands = world.commands();
            let mut sounder = Sounder {
                commands: &mut commands,
                mixer: &mut mixer,
                bindings: &mut bindings,
                now: Duration::ZERO,
                focus: Vec2::ZERO,
                silent: false,
                ears: None,
                own: None,
                fog: &fog,
                trace: None,
            };
            sounder.sound(Kind::HeroArrived, 1.0, Some((0.0, 0.0)), None, None);
        }
        world.flush();
        assert_eq!(
            world.query::<&AudioPlayer>().iter(&world).count(),
            crate::mixer::MAX_VOICES,
            "a portal landing bypassed the mixer of the other world sounds"
        );
    }

    #[test]
    fn projectile_cues_limit_each_emitter_without_claiming_loop_ownership() {
        let (mut app, emitters) =
            scene_app("event Synthetic.Attack 0 120 0 0 100 1 1 0 -1 1 70 10000 - 1 -1\n");
        for emitter in [emitters[0], emitters[1], emitters[0]] {
            emit_projectiles(&mut app, &[emitter]);
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(100));
            app.update();
        }
        let world = app.world_mut();
        assert_eq!(
            world.query::<&AudioPlayer>().iter(world).count(),
            2,
            "one emitter's instance limit refused another emitter or allowed its own excess"
        );
    }

    #[test]
    fn only_the_actual_thrower_gets_the_players_volume_factor() {
        let (mut app, emitters) =
            scene_app("event Synthetic.Attack 0 120 0 0 100 0 0 0 -1 1 70 10000 - 0.25 -1\n");
        let mut expected = Vec::new();
        for (index, emitter) in emitters.iter().enumerate() {
            let cues = emit_projectiles(&mut app, &[*emitter]);
            let cue = cues
                .iter()
                .find(|cue| cue.kind == CueKind::Attack)
                .expect("the actual launch cue");
            let at = cue.at.expect("a positional launch");
            let gain = app
                .world()
                .resource::<Mixer>()
                .gain_for("Synthetic.Attack", at, (0.0, 0.0));
            let player = if index == 0 { 0.25 } else { 1.0 };
            expected.push((at.0, cue.gain * gain * player));
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(100));
            app.update();
        }
        let world = app.world_mut();
        let mut actual: Vec<_> = world
            .query::<(Entity, &PlaybackSettings)>()
            .iter(world)
            .filter_map(|(entity, settings)| {
                let (_, at) = world.resource::<Mixer>().event_at(entity)?;
                Some((at?.0, settings.volume.to_linear()))
            })
            .collect();
        actual.sort_by(|a, b| a.0.total_cmp(&b.0));
        expected.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(actual.len(), 2, "both real launch cues started");
        for (actual, expected) in actual.iter().zip(expected) {
            assert!(
                (actual.1 - expected.1).abs() < 0.000_01,
                "emitter at {} sounded at {}, expected {}",
                actual.0,
                actual.1,
                expected.1
            );
        }
    }

    /// Looks at the shops once a second as long as `looks` lasts: `(in match, near)`.
    fn watched(watch: &mut ShopWatch, looks: &[(bool, Option<bool>)]) -> Vec<Option<bool>> {
        looks
            .iter()
            .enumerate()
            .map(|(second, (in_match, near))| watch.look(*in_match, *near, second as f32))
            .collect()
    }

    #[test]
    fn only_the_players_own_heros_blows_get_the_players_factor() {
        let hero = |event| row(Subject::Hero(HeroId::PUDGE), event, None);
        let id = |idx| Some(EntityId { idx, generation: 1 });
        let (me, ally, enemy) = (id(1), id(2), id(3));
        for event in [Event::Attack, Event::Impact] {
            assert!(sounds_from_the_players_entity(hero(event), me, me));
            assert!(
                !sounds_from_the_players_entity(hero(event), me, ally),
                "an ally's"
            );
            assert!(
                !sounds_from_the_players_entity(hero(event), me, enemy),
                "an enemy's"
            );
            assert!(
                !sounds_from_the_players_entity(hero(event), None, enemy),
                "a spectator's"
            );
        }
        assert!(!sounds_from_the_players_entity(hero(Event::Death), me, me));
        let creep = row(Subject::MeleeCreep, Event::Attack, None);
        assert!(!sounds_from_the_players_entity(creep, me, me));
    }

    #[test]
    fn a_hero_starting_in_a_shop_hears_nothing_and_leaving_then_returning_sounds_each_once() {
        let mut watch = ShopWatch::default();
        let heard = watched(
            &mut watch,
            &[
                (true, Some(true)),
                (true, Some(true)),
                (true, Some(false)),
                (true, Some(false)),
                (true, Some(true)),
                (true, Some(true)),
            ],
        );
        assert_eq!(
            heard,
            [None, None, None, Some(false), None, Some(true)],
            "the change sounds once it has held for a look"
        );
    }

    #[test]
    fn a_hero_stepping_along_the_edge_of_the_range_does_not_chime() {
        let mut watch = ShopWatch::default();
        let mut heard = vec![watch.look(true, Some(true), 0.0)];
        for step in 1..40 {
            let near = step % 2 == 0;
            heard.push(watch.look(true, Some(near), step as f32 * 0.1));
        }
        assert!(heard.iter().all(Option::is_none), "{heard:?}");
    }

    #[test]
    fn a_dead_hero_keeps_the_state_it_fell_in() {
        let mut watch = ShopWatch::default();
        let heard = watched(
            &mut watch,
            &[
                (true, Some(false)),
                (true, None),
                (true, None),
                (true, Some(false)),
            ],
        );
        assert_eq!(
            heard, [None; 4],
            "dying away from the shop and standing there again"
        );
    }

    #[test]
    fn a_second_match_starting_in_the_shop_does_not_inherit_the_first_ones_state() {
        let mut watch = ShopWatch::default();
        let first = watched(&mut watch, &[(true, Some(false)), (true, Some(false))]);
        let between = watched(&mut watch, &[(false, None), (false, None)]);
        let second = watched(&mut watch, &[(true, Some(true)), (true, Some(true))]);
        assert_eq!(first, [None; 2]);
        assert_eq!(between, [None; 2]);
        assert_eq!(second, [None; 2], "the first look of a match only learns");
    }

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
        names
            .iter()
            .flat_map(|n| manifest.sounds_with_events(n))
            .map(|(path, _)| path)
            .collect()
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
        for (event, count, path, dota_event) in [
            (
                Event::PreAttack,
                4,
                "hero/shared/small_blade/whoosh",
                "Hero_Pudge.PreAttack",
            ),
            (Event::Attack, 3, "hero/pudge/attack", "Hero_Pudge.Attack"),
            (Event::Impact, 3, "hero/pudge/attack", "Hero_Pudge.Attack"),
        ] {
            let files = files_of(&manifest, row(pudge, event, None));
            let expected: Vec<_> = (1..=count)
                .map(|index| {
                    crate::manifest::cache_path(&format!("sounds/weapons/{path}{index:02}.wav"))
                })
                .collect();
            assert_eq!(files, expected, "{event:?}: exactly the install's variants");
            let logical = Row::new(pudge, event, None)
                .name(&manifest)
                .expect("a sound row");
            assert!(
                manifest
                    .sounds_with_events(&logical)
                    .iter()
                    .all(|(_, event)| event == dota_event),
                "{event:?}: the install's event name"
            );
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
