//! The voices of the sounds in the world, kept as Dota keeps them. The install's sound events
//! (`sounds/events.txt`, read from the `soundevents` and the event types of the install by the
//! importer) say how many instances of an event may sound at once and which repeats are
//! blocked; a sound that loops sounds until its owner stops it. Every sound of the world's
//! units starts through [`Mixer::start`] and a looping one ends through [`Mixer::stop`] (or at
//! once, through [`Mixer::stop_loops`], when the match is gone), so that nothing sounds without a
//! bound and nothing outlives what it sounds for.
//!
//! A voice stolen while it fades out is cut where it is: the fade is for the sound that is stopped,
//! not for the one that makes room.
//!
//! Outside the mixer, and so outside its [`MAX_VOICES`]: the portal hums (`sound.rs`, which end
//! with their teleports), the soundscape's loops (`soundscape.rs`, at most 24 by its own bound),
//! the announcer (`voice.rs`). A hero's voice lines (`hero_voice.rs`) sound through it, their
//! events those of the hero spoken for ([`Mixer::set_voice_events`]).

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use bevy::{
    audio::{AudioSink, AudioSinkPlayback, PlaybackSettings, Volume},
    prelude::*,
};
use bota_proto::EntityId;

use crate::attenuation::falloff;
use crate::cache_text::{EventLine, GroupLine, LayerLine};
use crate::fog::FogGrid;
use crate::manifest::Manifest;

/// Voices sounding at once at most, fading ones included: the oldest one-shot is stopped for a
/// new voice, and a voice is refused when only loops remain.
pub const MAX_VOICES: usize = 64;
/// Random ambience shares the world budget without crowding out gameplay sounds.
pub const MAX_AMBIENCE_VOICES: usize = 8;
const _: () = assert!(MAX_AMBIENCE_VOICES < MAX_VOICES);
/// How long the blows on one unit merge into one sound: the owner's rule, half a second of a
/// unit's pain is one sound, not each blow in full.
pub const MERGE_WINDOW: Duration = Duration::from_millis(500);
/// An unknown-duration sound stops at this deadline so it cannot escape the voice budget.
const UNKNOWN_LENGTH: Duration = Duration::from_secs(10);
/// Most events read from the cache.
const MAX_EVENTS: usize = 1 << 14;
const MISSING_EVENT_BLOCK: Duration = Duration::from_millis(50);
const MISSING_EVENT_BLOCK_UNITS: f32 = 100.0;
const MAX_RECENT_GAMEPLAY: usize = MAX_VOICES * 2;
const MAX_RECENT_AMBIENCE: usize = MAX_AMBIENCE_VOICES * 2;

/// Marks the sound player of a sound of the game: the match's world, voices, announcer and
/// ambience, which end with the match. The menu's own sounds do not carry it.
#[derive(Component)]
pub struct GameAudio;

/// Whom a looping sound sounds for: it ends with that unit's wearing of the effect it sounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// Nobody: a loop that has none plays once for its length, and is told as a fault.
    None,
    Unit(EntityId),
}

/// What asks to be sounded.
pub struct Request<'a> {
    /// The sound event, or a stable binding name when the cache has no event metadata.
    pub event: &'a str,
    pub owner: Owner,
    /// The event emitter for per-entity limits, independently of loop ownership and hit target.
    pub emitter: Option<EntityId>,
    /// The unit a blow lands on: blows on one unit merge.
    pub merge: Option<EntityId>,
    /// Where it sounds, in the world; none for a sound heard wherever the camera is.
    pub at: Option<(f32, f32)>,
    /// Where the listener is (the camera's focus): the sound carries from `at` to it as its
    /// event says.
    pub listener: Option<(f32, f32)>,
    /// How strongly the caller asks for it, before the event's volume, its carrying and the mix.
    pub gain: f32,
    pub speed: f32,
    /// Whether it sounds from the entity of the player the game is played on, which Dota gives
    /// the event's `volume_on_player_mult` for.
    pub on_player: bool,
    /// The fog of war, for an event that is heard only where its source is in sight.
    pub fog: Option<&'a FogGrid>,
}

impl Default for Request<'_> {
    fn default() -> Self {
        Request {
            event: "",
            owner: Owner::None,
            emitter: None,
            merge: None,
            at: None,
            listener: None,
            gain: 1.0,
            speed: 1.0,
            on_player: false,
            fog: None,
        }
    }
}

/// The loudness every world sound is made up by: Dota's default mix keeps the interface's group
/// at 0.5, and the client's interface sounds (clicks, the menu) play at their own level
/// outside the mix, so the mix as a whole is raised until its interface group sounds as loud as
/// the interface does. The player's volume setting is the master above this.
pub const MIX_MAKEUP_GAIN: f32 = 2.0;
/// Quietest a sound with a place is started at: below it, it is not heard.
const QUIETEST_START: f32 = 0.03;
/// Most mix layers, and so most states of them, the mixer follows.
const MAX_LAYERS: usize = 64;
const MAX_GROUPS: usize = 256;

/// What became of a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Started {
    /// A loop, sounding until its owner stops it.
    Loop(Entity),
    /// A sound that plays out by itself.
    Once,
    /// Not sounded: blocked, merged, limited, or no room.
    Refused,
}

/// The sound events of the cache, by name, and the voices sounding now.
#[derive(Resource, Default)]
pub struct Mixer {
    events: HashMap<String, EventLine>,
    voices: Vec<Voice>,
    recent: Vec<Recent>,
    /// The events a loop with no owner was refused for, told once each.
    warned: HashSet<String>,
    /// No match is on to sound: nothing starts.
    closed: bool,
    /// The player has muted the game: voices are taken and kept, at no volume, so that the
    /// decisions about them do not depend on the volume knob and unmuting finds them sounding.
    muted: bool,
    /// How many times the game's sounds were ended, for what queues outside the mixer.
    ends: u32,
    /// The mix groups and the levels of the default mix.
    groups: HashMap<String, GroupLine>,
    /// The mix layers, each with how far it is on now.
    layers: Vec<LayerState>,
    /// The loudest each group sounded since the last tick: what the layers' triggers are told.
    activity: HashMap<String, f32>,
    last_tick: Option<Duration>,
    /// The events [`Mixer::set_voice_events`] took, to give back when the voice changes.
    voice_events: Vec<String>,
}

/// A mix layer and how far each of its triggers has it on, 0 to 1 (a trigger ramps at its own
/// attack and release; the layer is as much on as its most-on trigger).
struct LayerState {
    line: LayerLine,
    amounts: Vec<f32>,
}

impl LayerState {
    fn new(line: LayerLine) -> LayerState {
        let amounts = vec![0.0; line.triggers.len()];
        LayerState { line, amounts }
    }

    fn amount(&self) -> f32 {
        self.amounts.iter().copied().fold(0.0, f32::max)
    }
}

/// How fast (per second) the fog of war's share of a loop moves when its source goes into
/// sight or out of it: the `volume_FOW_filter_vel` of Dota's sound event types, 1.
const FOG_FILTER_VELOCITY: f32 = 1.0;

/// One sound sounding.
struct Voice {
    entity: Entity,
    event: String,
    looping: bool,
    class: VoiceClass,
    emitter: Option<EntityId>,
    at: Option<(f32, f32)>,
    started: Duration,
    /// The playback deadline; an owned loop without a stop time has none.
    ends: Option<Duration>,
    /// The start of its fade-out once stopped, and how long the fade lasts.
    stopped: Option<(Duration, Duration)>,
    /// The mix group it sounds in.
    group: Option<String>,
    /// What the request asked, times the event's volume and the player's factor.
    base: f32,
    /// Where the listener was when it last moved.
    listener: Option<(f32, f32)>,
    /// The radius within which sight counts for it, when the fog of war decides whether it is
    /// heard, and how much of it the fog lets through now (a loop's share moves at
    /// [`FOG_FILTER_VELOCITY`]).
    fog_radius: Option<f32>,
    fog_share: f32,
    /// `base` as the distance to the listener leaves it.
    gain: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VoiceClass {
    World,
    Ambience,
}

struct InitialMix {
    base: f32,
    gain: f32,
    group: Option<String>,
}

/// Merge and missing-event repeat windows can outlive the physical player. Background history
/// has its own allowance so completed ambience cannot consume gameplay admission history.
struct Recent {
    event: String,
    merge: Option<EntityId>,
    at: Option<(f32, f32)>,
    started: Duration,
    expires: Duration,
    class: VoiceClass,
}

impl Voice {
    fn sounding(&self) -> bool {
        self.stopped.is_none()
    }
}

impl Mixer {
    #[cfg(test)]
    pub(crate) fn event_at(&self, entity: Entity) -> Option<(&str, Option<(f32, f32)>)> {
        self.voices
            .iter()
            .find(|voice| voice.entity == entity)
            .map(|voice| (voice.event.as_str(), voice.at))
    }

    /// Reads the event policies; missing metadata uses the bounded fallback repeat policy.
    pub fn load(manifest: &Manifest) -> Mixer {
        let text = crate::cachefs::read_small(&manifest.root, "sounds/events.txt")
            .and_then(|bytes| String::from_utf8(bytes).ok());
        let Some(text) = text else {
            warn!("sounds/events.txt is missing from the cache: using the fallback repeat policy");
            return Mixer::default();
        };
        let events = crate::cache_text::sound_events(&text, MAX_EVENTS);
        let mix = crate::cachefs::read_small(&manifest.root, "sounds/mixer.txt")
            .and_then(|bytes| String::from_utf8(bytes).ok());
        if mix.is_none() {
            warn!("sounds/mixer.txt is missing from the cache: no sound is mixed");
        }
        let (groups, layers) = crate::cache_text::mixer_lines(&mix.unwrap_or_default(), 4096);
        Mixer {
            events: events.into_iter().map(|e| (e.name.clone(), e)).collect(),
            groups: groups
                .into_iter()
                .take(MAX_GROUPS)
                .map(|g| (g.name.clone(), g))
                .collect(),
            layers: layers
                .into_iter()
                .take(MAX_LAYERS)
                .map(LayerState::new)
                .collect(),
            ..Mixer::default()
        }
    }

    /// Takes the sound events of the voice the client speaks with (a hero's pack's
    /// `events.txt`), in place of the voice's it took before: only the hero spoken for has its
    /// lines' limits and mix groups here. Returns how many it took.
    pub fn set_voice_events(&mut self, text: &str) -> usize {
        for name in std::mem::take(&mut self.voice_events) {
            self.events.remove(&name);
        }
        let room = MAX_EVENTS.saturating_sub(self.events.len());
        for line in crate::cache_text::sound_events(text, room) {
            if !self.events.contains_key(&line.name) {
                self.voice_events.push(line.name.clone());
                self.events.insert(line.name.clone(), line);
            }
        }
        self.voice_events.len()
    }

    /// The volume and sound level factors of a mix group now: its level in the default mix, as
    /// each layer that is on takes it from 1 towards its own, `lerp(1, vol, a) · (1 − mute · a)`
    /// for the volume and `lerp(1, lvl, a)` for the level, `a` being how far the layer is on.
    fn group_factors(&self, group: Option<&str>) -> (f32, f32) {
        let Some(name) = group else {
            return (1.0, 1.0);
        };
        let (mut volume, mut level) = self
            .groups
            .get(name)
            .map_or((1.0, 1.0), |g| (g.volume * (1.0 - g.mute), g.level));
        for layer in &self.layers {
            let a = layer.amount();
            for set in layer.line.levels.iter().filter(|l| l.group == name) {
                volume *= (1.0 + (set.volume - 1.0) * a) * (1.0 - set.mute * a);
                level *= 1.0 + (set.level - 1.0) * a;
            }
        }
        (volume, level)
    }

    /// The share of its volume the event keeps over `distance` in its mix group now.
    fn spatial(&self, rules: Option<&EventLine>, distance: f32) -> f32 {
        let level = rules.map_or(crate::attenuation::DEFAULT_LEVEL_DB, |r| r.sound_level);
        let max = rules.and_then(|r| r.distance_max);
        let group = rules.and_then(|r| r.group.as_deref());
        falloff(distance, level, max, self.group_factors(group).1)
    }

    /// What the loudness of a sound of the event is at the sink: its gain times its group's mix
    /// level times [`MIX_MAKEUP_GAIN`], at most 1. A sound of no group is the interface's, which
    /// the makeup gain is calibrated to: it sounds at its own level.
    fn sink_gain(&self, event_gain: f32, group: Option<&str>) -> f32 {
        if self.muted {
            return 0.0;
        }
        let makeup = if group.is_some() {
            MIX_MAKEUP_GAIN
        } else {
            1.0
        };
        (event_gain * self.group_factors(group).0 * makeup).clamp(0.0, 1.0)
    }

    /// How loud a sound of the event heard from `at` by a listener at `listener` is at the sink,
    /// for the sounds that play outside the mixer's voices (the portals' hums).
    pub fn gain_for(&self, event: &str, at: (f32, f32), listener: (f32, f32)) -> f32 {
        let rules = self.events.get(event);
        let distance = Vec2::from(at).distance(Vec2::from(listener));
        let volume = rules.map_or(1.0, |r| r.volume);
        let group = rules.and_then(|r| r.group.as_deref());
        self.sink_gain(volume * self.spatial(rules, distance), group)
    }

    /// Tells the layers that a sound of `volume` (before the mix) sounded in the group this
    /// frame, for a sound that plays outside the mixer's voices (the announcer's line).
    pub fn note_activity(&mut self, group: &str, volume: f32) {
        let volume = volume * self.group_factors(Some(group)).0;
        let heard = self.activity.entry(group.to_owned()).or_insert(0.0);
        *heard = heard.max(volume);
    }

    /// Whether the event's sound repeats until its owner stops it.
    pub fn loops(&self, event: &str) -> bool {
        self.events.get(event).is_some_and(|e| e.looping)
    }

    /// Sounds the request as its event asks: not at all if a blocked repeat or a blow merged
    /// into one sounding, after stopping the oldest of the event's voices if its limit is
    /// reached, and not if the world has no room for it.
    pub fn start(
        &mut self,
        commands: &mut Commands,
        now: Duration,
        request: &Request,
        source: Handle<AudioSource>,
    ) -> Started {
        self.start_as(commands, now, request, source, VoiceClass::World)
    }

    /// Random ambience may fill spare slots, but never evicts a gameplay sound.
    pub fn start_ambience(
        &mut self,
        commands: &mut Commands,
        now: Duration,
        request: &Request,
        source: Handle<AudioSource>,
    ) -> Started {
        self.start_as(commands, now, request, source, VoiceClass::Ambience)
    }

    fn start_as(
        &mut self,
        commands: &mut Commands,
        now: Duration,
        request: &Request,
        source: Handle<AudioSource>,
        class: VoiceClass,
    ) -> Started {
        self.drop_ended(commands, now);
        if self.closed {
            return Started::Refused;
        }
        if class == VoiceClass::Ambience
            && (self.voices.len() >= MAX_VOICES
                || self
                    .voices
                    .iter()
                    .filter(|voice| voice.class == class)
                    .count()
                    >= MAX_AMBIENCE_VOICES)
        {
            return Started::Refused;
        }
        let rules = self.events.get(request.event).cloned();
        let Some(mix) = self.initial_mix(request, rules.as_ref()) else {
            return Started::Refused;
        };
        let looping = rules.as_ref().is_some_and(|r| r.looping);
        if looping && request.owner == Owner::None && self.warned.insert(request.event.to_owned()) {
            warn!("{}: a loop with no owner plays once", request.event);
        }
        let kept = looping && request.owner != Owner::None;
        if self.blocked(request, rules.as_ref(), now) {
            return Started::Refused;
        }
        let window = history_window(request, rules.as_ref());
        if (!window.is_zero() && !self.history_has_room(class))
            || !self.make_room(commands, now, request, rules.as_ref(), class)
        {
            return Started::Refused;
        }
        let started = self.spawn_voice(
            commands,
            now,
            request,
            source,
            rules.as_ref(),
            kept,
            class,
            mix,
        );
        self.remember_trigger(request, now, window, class);
        started
    }

    fn initial_mix(&self, request: &Request, rules: Option<&EventLine>) -> Option<InitialMix> {
        let (volume, group) = (
            rules.map_or(1.0, |r| r.volume),
            rules.and_then(|r| r.group.clone()),
        );
        let player = if request.on_player {
            rules.map_or(1.0, |r| r.player_mult)
        } else {
            1.0
        };
        let base = request.gain * volume * player;
        let distance = match (request.at, request.listener) {
            (Some(at), Some(listener)) => Some(Vec2::from(at).distance(Vec2::from(listener))),
            _ => None,
        };
        let spatial = distance.map_or(1.0, |d| self.spatial(rules, d));
        if distance.is_some() && request.gain * spatial < QUIETEST_START {
            return None;
        }
        // An event that follows the fog of war is heard only where its source is in sight.
        if let (Some(radius), Some(at), Some(fog)) =
            (rules.and_then(|r| r.fog_radius), request.at, request.fog)
            && !fog.lit_within(at.0, at.1, radius)
        {
            return None;
        }
        Some(InitialMix {
            base,
            gain: base * spatial,
            group,
        })
    }

    fn spawn_voice(
        &mut self,
        commands: &mut Commands,
        now: Duration,
        request: &Request,
        source: Handle<AudioSource>,
        rules: Option<&EventLine>,
        kept: bool,
        class: VoiceClass,
        mix: InitialMix,
    ) -> Started {
        let ends = lifetime(rules, kept);
        let playback = if kept {
            PlaybackSettings::LOOP
        } else {
            PlaybackSettings::DESPAWN
        };
        let entity = commands
            .spawn((
                GameAudio,
                AudioPlayer::new(source),
                playback
                    .with_volume(Volume::Linear(
                        self.sink_gain(mix.gain, mix.group.as_deref()),
                    ))
                    .with_speed(request.speed),
            ))
            .id();
        self.voices.push(Voice {
            entity,
            event: request.event.to_owned(),
            looping: kept,
            class,
            emitter: request.emitter,
            at: request.at,
            started: now,
            ends: ends.map(|length| now + length),
            stopped: None,
            group: mix.group,
            base: mix.base,
            listener: request.listener,
            fog_radius: rules.and_then(|r| r.fog_radius),
            fog_share: 1.0,
            gain: mix.gain,
        });
        if kept {
            Started::Loop(entity)
        } else {
            Started::Once
        }
    }

    /// [`Mixer::start`], and the entity of the voice it started, if it started one: what a
    /// line's lip sync follows.
    pub fn start_heard(
        &mut self,
        commands: &mut Commands,
        now: Duration,
        request: &Request,
        source: Handle<AudioSource>,
    ) -> Option<Entity> {
        match self.start(commands, now, request, source) {
            Started::Loop(entity) => Some(entity),
            Started::Once => self.voices.last().map(|voice| voice.entity),
            Started::Refused => None,
        }
    }

    /// Whether the event's block (a repeat within its time and distance of one sounding) or the
    /// merge of blows on one unit keeps the request from sounding.
    fn blocked(&self, request: &Request, rules: Option<&EventLine>, now: Duration) -> bool {
        let block = rules.map_or(Duration::ZERO, |r| r.block);
        let units = rules.map_or(0.0, |r| r.block_units);
        let sounding = self
            .voices
            .iter()
            .filter(|v| v.event == request.event && v.sounding())
            .any(|v| {
                let age = now.saturating_sub(v.started);
                age < block && within_repeat_range(v.at, request.at, units)
            });
        sounding
            || self
                .recent
                .iter()
                .filter(|recent| recent.event == request.event)
                .any(|recent| {
                    let age = now.saturating_sub(recent.started);
                    let merged = request.merge.is_some() && recent.merge == request.merge;
                    let fallback = rules.is_none()
                        && age < MISSING_EVENT_BLOCK
                        && within_repeat_range(recent.at, request.at, MISSING_EVENT_BLOCK_UNITS);
                    (merged && age < MERGE_WINDOW) || fallback
                })
    }

    fn history_has_room(&self, class: VoiceClass) -> bool {
        let capacity = match class {
            VoiceClass::World => MAX_RECENT_GAMEPLAY,
            VoiceClass::Ambience => MAX_RECENT_AMBIENCE,
        };
        self.recent
            .iter()
            .filter(|recent| recent.class == class)
            .count()
            < capacity
    }

    fn remember_trigger(
        &mut self,
        request: &Request,
        now: Duration,
        window: Duration,
        class: VoiceClass,
    ) {
        if window.is_zero() {
            return;
        }
        assert!(
            self.history_has_room(class),
            "trigger admission must reserve history before eviction"
        );
        self.recent.push(Recent {
            event: request.event.to_owned(),
            merge: request.merge,
            at: request.at,
            started: now,
            expires: now + window,
            class,
        });
        assert!(self.recent.len() <= MAX_RECENT_GAMEPLAY + MAX_RECENT_AMBIENCE);
    }

    /// Makes room for a voice: the event's limit stops the oldest of its voices (or refuses the
    /// new one), and the world's [`MAX_VOICES`] stops the oldest one-shot; false when there is no
    /// room.
    fn make_room(
        &mut self,
        commands: &mut Commands,
        now: Duration,
        request: &Request,
        rules: Option<&EventLine>,
        class: VoiceClass,
    ) -> bool {
        if let Some(rules) = rules.filter(|r| r.limit > 0) {
            let counted = |v: &Voice| {
                v.event == request.event
                    && v.sounding()
                    && (!rules.per_entity || v.emitter == request.emitter)
            };
            if self.voices.iter().filter(|v| counted(v)).count() >= rules.limit as usize {
                if class == VoiceClass::Ambience {
                    return false;
                }
                // The oldest one-shot gives way; a loop never does, its owner is the one to end
                // it and nothing would start it again.
                let oldest = self.voices.iter().position(|v| counted(v) && !v.looping);
                if !rules.stop_oldest || oldest.is_none() {
                    return false;
                }
                self.stop_at(commands, oldest, now);
            }
        }
        if self.voices.len() < MAX_VOICES {
            return true;
        }
        // Gameplay displaces ambience first, then a fading voice or the oldest one-shot.
        let stolen = self
            .voices
            .iter()
            .position(|v| v.class == VoiceClass::Ambience)
            .or_else(|| self.voices.iter().position(|v| !v.sounding()))
            .or_else(|| self.voices.iter().position(|v| !v.looping));
        match stolen {
            Some(at) => {
                let voice = self.voices.remove(at);
                commands.entity(voice.entity).try_despawn();
                true
            }
            None => false,
        }
    }

    /// Stops a voice: it fades out over its event's fade-out, or at once without one.
    pub fn stop(&mut self, commands: &mut Commands, now: Duration, entity: Entity) {
        let at = self.voices.iter().position(|v| v.entity == entity);
        self.stop_at(commands, at, now);
    }

    /// Whether no match is on to sound: the mixer starts nothing.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Ends every voice at once, one-shots and loops, and starts none until it is opened again
    /// (a match is back in view); the match they sounded in is gone.
    pub fn close(&mut self, commands: &mut Commands) {
        if !self.closed {
            self.ends += 1;
        }
        self.closed = true;
        self.recent.clear();
        for layer in &mut self.layers {
            layer.amounts.fill(0.0);
        }
        for voice in self.voices.drain(..) {
            commands.entity(voice.entity).try_despawn();
        }
    }

    /// How many times the game's sounds were ended.
    pub fn ends(&self) -> u32 {
        self.ends
    }

    /// Lets voices start again.
    pub fn open(&mut self) {
        self.closed = false;
    }

    /// Stops every loop at once, owners gone: no match is on to sound. One-shots play out.
    pub fn stop_loops(&mut self, commands: &mut Commands) {
        self.voices.retain(|voice| {
            if voice.looping {
                commands.entity(voice.entity).try_despawn();
            }
            !voice.looping
        });
    }

    fn stop_at(&mut self, commands: &mut Commands, at: Option<usize>, now: Duration) {
        let Some(at) = at else {
            return;
        };
        let fade = self
            .events
            .get(&self.voices[at].event)
            .map_or(Duration::ZERO, |e| e.fade_out);
        if !fade.is_zero() && self.voices[at].sounding() {
            self.voices[at].stopped = Some((now, fade));
            return;
        }
        let voice = self.voices.remove(at);
        commands.entity(voice.entity).try_despawn();
    }

    /// Moves a loop with its source and its listener: where it sounds, and how loud that place
    /// is to the listener (worked out at the next tick).
    pub fn follow(&mut self, entity: Entity, at: (f32, f32), listener: (f32, f32)) {
        if let Some(voice) = self.voices.iter_mut().find(|v| v.entity == entity) {
            voice.at = Some(at);
            voice.listener = Some(listener);
        }
    }

    /// Takes the voices that ended out, sets the loops' volumes and fades those being stopped.
    pub fn tick(
        &mut self,
        commands: &mut Commands,
        now: Duration,
        sinks: &mut Query<&mut AudioSink>,
        fog: Option<&FogGrid>,
    ) {
        self.drop_ended(commands, now);
        self.update_voices(now, fog);
        for voice in self.voices.iter().filter(|v| v.looping || !v.sounding()) {
            let Ok(mut sink) = sinks.get_mut(voice.entity) else {
                continue;
            };
            let left = voice.stopped.map_or(1.0, |(since, fade)| {
                1.0 - (now.saturating_sub(since).as_secs_f32() / fade.as_secs_f32()).clamp(0.0, 1.0)
            });
            sink.set_volume(Volume::Linear(self.voice_sink_gain(voice) * left));
        }
    }

    /// What a voice sounds at the sink now, before its fade-out.
    fn voice_sink_gain(&self, voice: &Voice) -> f32 {
        self.sink_gain(voice.gain, voice.group.as_deref()) * voice.fog_share
    }

    /// Moves the layers, and works out each loop's carrying to its listener and its share the fog
    /// of war lets through: a loop whose source goes out of sight goes quiet over a second, and
    /// comes back as it comes into sight again.
    fn update_voices(&mut self, now: Duration, fog: Option<&FogGrid>) {
        let dt = self.move_layers(now);
        let spatials: Vec<f32> = self
            .voices
            .iter()
            .map(|voice| match (voice.at, voice.listener) {
                (Some(at), Some(listener)) => {
                    let distance = Vec2::from(at).distance(Vec2::from(listener));
                    self.spatial(self.events.get(&voice.event), distance)
                }
                _ => 1.0,
            })
            .collect();
        for (voice, spatial) in self.voices.iter_mut().zip(spatials) {
            if !voice.looping {
                continue;
            }
            voice.gain = voice.base * spatial;
            if let (Some(radius), Some(at), Some(fog)) = (voice.fog_radius, voice.at, fog) {
                let target = if fog.lit_within(at.0, at.1, radius) {
                    1.0
                } else {
                    0.0
                };
                let step = FOG_FILTER_VELOCITY * dt;
                voice.fog_share = if target > voice.fog_share {
                    (voice.fog_share + step).min(target)
                } else {
                    (voice.fog_share - step).max(target)
                };
            }
        }
    }

    /// Moves each trigger of each layer towards on while its group sounds at its threshold or
    /// louder, and towards off when it does not, at the pace of its own attack and release; the
    /// seconds since the last look.
    fn move_layers(&mut self, now: Duration) -> f32 {
        let dt = self
            .last_tick
            .map_or(0.0, |last| now.saturating_sub(last).as_secs_f32());
        self.last_tick = Some(now);
        for voice in self.voices.iter().filter(|v| v.sounding()) {
            if let Some(group) = &voice.group {
                let heard = voice.gain * self.group_factors(Some(group)).0;
                let entry = self.activity.entry(group.clone()).or_insert(0.0);
                *entry = entry.max(heard);
            }
        }
        let pace = |span: Duration| {
            if span.is_zero() {
                f32::INFINITY
            } else {
                dt / span.as_secs_f32()
            }
        };
        for layer in &mut self.layers {
            for (trigger, amount) in layer.line.triggers.iter().zip(&mut layer.amounts) {
                let on = self
                    .activity
                    .get(&trigger.group)
                    .is_some_and(|heard| *heard >= trigger.threshold);
                let target = if on { trigger.amount } else { 0.0 };
                *amount = if target > *amount {
                    (*amount + pace(trigger.attack)).min(target)
                } else {
                    (*amount - pace(trigger.release)).max(target)
                };
            }
        }
        self.activity.clear();
        dt
    }

    fn drop_ended(&mut self, commands: &mut Commands, now: Duration) {
        self.recent.retain(|recent| now < recent.expires);
        self.voices.retain(|voice| {
            let faded = voice
                .stopped
                .is_some_and(|(since, fade)| now.saturating_sub(since) >= fade);
            let over = faded || voice.ends.is_some_and(|ends| now >= ends);
            if over {
                commands.entity(voice.entity).try_despawn();
            }
            !over
        });
    }

    /// The voices of an event sounding now (not those fading out).
    #[cfg(test)]
    pub(crate) fn sounding(&self, event: &str) -> usize {
        self.voices
            .iter()
            .filter(|v| v.event == event && v.sounding())
            .count()
    }

    /// How much of a voice the fog of war lets through now.
    #[cfg(test)]
    pub(crate) fn fog_share(&self, entity: Entity) -> Option<f32> {
        self.voices
            .iter()
            .find(|v| v.entity == entity)
            .map(|v| v.fog_share)
    }

    /// Whether no layer is on: the groups sound at their own levels.
    #[cfg(test)]
    pub(crate) fn layers_at_rest(&self) -> bool {
        self.layers.iter().all(|l| l.amount() <= 0.0)
    }

    /// How far a layer is on now, 0 to 1.
    #[cfg(test)]
    pub(crate) fn layer_amount(&self, layer: &str) -> Option<f32> {
        self.layers
            .iter()
            .find(|l| l.line.name == layer)
            .map(LayerState::amount)
    }

    /// The volume of the newest voice of the event sounding now, before any fade.
    #[cfg(test)]
    pub(crate) fn voice_gain(&self, event: &str) -> Option<f32> {
        self.voices
            .iter()
            .rev()
            .find(|v| v.event == event && v.sounding())
            .map(|v| v.gain)
    }

    /// The volume a looping voice of the event has now, before its fade.
    #[cfg(test)]
    pub(crate) fn loop_gain(&self, event: &str) -> Option<f32> {
        self.voices
            .iter()
            .find(|v| v.event == event && v.looping && v.sounding())
            .map(|v| v.gain)
    }
}

fn history_window(request: &Request, rules: Option<&EventLine>) -> Duration {
    let fallback = if rules.is_none() {
        MISSING_EVENT_BLOCK
    } else {
        Duration::ZERO
    };
    if request.merge.is_some() {
        fallback.max(MERGE_WINDOW)
    } else {
        fallback
    }
}

fn within_repeat_range(a: Option<(f32, f32)>, b: Option<(f32, f32)>, units: f32) -> bool {
    match (a, b) {
        (Some(a), Some(b)) if units > 0.0 => Vec2::from(a).distance(Vec2::from(b)) <= units,
        _ => true,
    }
}

/// The deadline for stopping a voice: the event's length or stop time, with a bounded fallback
/// when neither is known. Only an owned loop may wait indefinitely for its owner to stop it.
fn lifetime(rules: Option<&EventLine>, kept: bool) -> Option<Duration> {
    let length = rules.map(|r| r.duration).filter(|d| !d.is_zero());
    let stop = rules.and_then(|r| r.stop_at);
    match (kept, length, stop) {
        (true, _, stop) => stop,
        (false, Some(length), Some(stop)) => Some(length.min(stop)),
        (false, Some(length), None) => Some(length),
        (false, None, Some(stop)) => Some(stop),
        (false, None, None) => Some(UNKNOWN_LENGTH),
    }
}

fn retire_player(removed: On<Remove<AudioPlayer>>, mut mixer: ResMut<Mixer>) {
    mixer.voices.retain(|voice| voice.entity != removed.entity);
}

/// Reads the cache's events at startup.
pub fn plugin(app: &mut App) {
    app.init_resource::<Mixer>()
        .add_observer(retire_player)
        .add_systems(
            Startup,
            |mut mixer: ResMut<Mixer>, manifest: Res<Manifest>| {
                *mixer = Mixer::load(&manifest);
            },
        )
        .add_systems(
            Update,
            |mut commands: Commands,
             mut mixer: ResMut<Mixer>,
             time: Res<Time>,
             mut sinks: Query<&mut AudioSink>,
             fog: Option<Res<FogGrid>>| {
                mixer.tick(&mut commands, time.elapsed(), &mut sinks, fog.as_deref());
            },
        )
        .add_systems(Update, end_with_the_match)
        .add_systems(
            Update,
            |mut mixer: ResMut<Mixer>,
             settings: Res<crate::config::Settings>,
             args: Res<crate::Args>| {
                mixer.muted = settings.muted || args.mute;
            },
        );
}

/// Silences the game when its match is gone: the player left it (the menu's idle session took
/// its place), the game on screen was replaced, or the connection dropped. Every sound of the
/// game ends at once, the mixer's voices included, and the mixer starts none until a match is
/// in view again. Before the first match nothing ends: the pick phase has its own voices.
fn end_with_the_match(
    session: NonSend<crate::session::Session>,
    epoch: Res<crate::main_menu::GameEpoch>,
    mut commands: Commands,
    mut mixer: ResMut<Mixer>,
    audio: Query<Entity, With<GameAudio>>,
    mut seen: Local<(u32, bool)>,
) {
    let core = &session.0;
    let (seen_epoch, had_match) = &mut *seen;
    let live = core.view().is_some() && !core.lost;
    let replaced = *seen_epoch != epoch.0;
    *seen_epoch = epoch.0;
    if live && !replaced {
        *had_match = true;
        mixer.open();
        return;
    }
    let ended = core.lost || replaced || (*had_match && core.view().is_none());
    if !ended && !mixer.is_closed() {
        return;
    }
    if ended {
        let was_playing = *had_match;
        mixer.close(&mut commands);
        for entity in &audio {
            commands.entity(entity).try_despawn();
        }
        *had_match = false;
        // A game that never showed a match left nothing stale to keep out: the pick phase's
        // sounds of the next game start as they come.
        if !core.lost && !was_playing {
            mixer.open();
        }
    }
    // Closed after a match it stays until the match that follows is in view.
    if live {
        *had_match = true;
        mixer.open();
    }
}

#[cfg(test)]
pub(crate) fn two_minute_source() -> AudioSource {
    let mut samples = std::io::Cursor::new(Vec::new());
    let specification = hound::WavSpec {
        channels: 1,
        sample_rate: 100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::new(&mut samples, specification).expect("valid wave");
    for _ in 0..12_000 {
        writer.write_sample(0i16).expect("sample fits");
    }
    writer.finalize().expect("complete two-minute source");
    AudioSource {
        bytes: samples.into_inner().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(name: &str, looping: bool, seconds: u64) -> EventLine {
        EventLine {
            name: name.to_owned(),
            looping,
            duration: Duration::from_secs(seconds),
            fade_out: Duration::ZERO,
            block: Duration::ZERO,
            block_units: 0.0,
            limit: 0,
            per_entity: false,
            stop_oldest: true,
            stop_at: None,
            volume: 1.0,
            sound_level: 70.0,
            distance_max: None,
            group: None,
            player_mult: 1.0,
            fog_radius: None,
        }
    }

    /// A fresh mixer of the same events.
    fn mixer_again(from: &Mixer) -> Mixer {
        mixer(from.events.values().cloned().collect())
    }

    fn mixer(events: Vec<EventLine>) -> Mixer {
        Mixer {
            events: events.into_iter().map(|e| (e.name.clone(), e)).collect(),
            ..Mixer::default()
        }
    }

    fn request(event: &str, owner: Owner) -> Request<'_> {
        Request {
            event,
            owner,
            merge: None,
            at: None,
            gain: 1.0,
            speed: 1.0,
            ..Request::default()
        }
    }

    fn owner(n: u32) -> Owner {
        Owner::Unit(EntityId {
            idx: n,
            generation: 1,
        })
    }

    fn an_app() -> App {
        use clap::Parser;

        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<crate::main_menu::GameEpoch>()
            .insert_resource(Manifest::default())
            .insert_resource(crate::config::Settings::default())
            .insert_resource(crate::Args::parse_from(["dbota"]))
            .insert_non_send(crate::main_menu::idle())
            .add_plugins(plugin);
        app.update();
        app
    }

    fn start_and_finish(world: &mut World, request: &Request) {
        let started = world.resource_scope(|world, mut mixer: Mut<Mixer>| {
            mixer.start(
                &mut world.commands(),
                Duration::ZERO,
                request,
                Handle::default(),
            )
        });
        assert_eq!(started, Started::Once);
        world.flush();
        let player = world
            .query_filtered::<Entity, With<AudioPlayer>>()
            .single(world)
            .expect("one physical player");
        world.despawn(player);
    }

    #[test]
    fn unknown_length_players_cannot_outlive_their_reserved_voice_slots() {
        let mut sources = Assets::<AudioSource>::default();
        let source = sources.add(two_minute_source());
        let mut world = World::new();
        let mut mixer = Mixer::default();
        for period in 0..3 {
            let now = UNKNOWN_LENGTH * period;
            for index in 0..MAX_VOICES {
                let event = format!("missing-duration-{index}");
                mixer.start(
                    &mut world.commands(),
                    now,
                    &request(&event, Owner::None),
                    source.clone(),
                );
            }
            world.flush();
            let players = world.query::<&AudioPlayer>().iter(&world).count();
            assert_eq!(
                players, MAX_VOICES,
                "live players escaped accounting in period {period}"
            );
        }
        mixer.close(&mut world.commands());
        world.flush();
        assert_eq!(world.query::<&AudioPlayer>().iter(&world).count(), 0);
    }

    #[test]
    fn a_failed_or_finished_player_releases_its_slot_immediately() {
        let mut app = an_app();
        let world = app.world_mut();
        world.insert_resource(mixer(vec![event("loop", true, 120)]));
        let player = world.resource_scope(|world, mut mixer: Mut<Mixer>| {
            let Started::Loop(player) = mixer.start(
                &mut world.commands(),
                Duration::ZERO,
                &request("loop", owner(1)),
                Handle::default(),
            ) else {
                panic!("the loop must start");
            };
            player
        });
        world.flush();
        assert_eq!(world.resource::<Mixer>().sounding("loop"), 1);
        world.despawn(player);
        assert_eq!(
            world.resource::<Mixer>().sounding("loop"),
            0,
            "the output ended the player but its voice reservation survived"
        );
    }

    fn ambience_preserves_gameplay(gameplay_players: usize) {
        let mut world = World::new();
        let mut limited = event("shared", false, 120);
        limited.limit = 1;
        let mut mixer = mixer(vec![event("world", false, 120), limited]);
        for _ in 1..gameplay_players {
            mixer.start(
                &mut world.commands(),
                Duration::ZERO,
                &request("world", Owner::None),
                Handle::default(),
            );
        }
        mixer.start(
            &mut world.commands(),
            Duration::ZERO,
            &request("shared", Owner::None),
            Handle::default(),
        );
        world.flush();
        let before: HashSet<_> = world
            .query_filtered::<Entity, With<AudioPlayer>>()
            .iter(&world)
            .collect();
        let started = mixer.start_ambience(
            &mut world.commands(),
            Duration::ZERO,
            &request("shared", Owner::None),
            Handle::default(),
        );
        world.flush();
        let after: HashSet<_> = world
            .query_filtered::<Entity, With<AudioPlayer>>()
            .iter(&world)
            .collect();
        assert_eq!(started, Started::Refused);
        assert_eq!(
            after, before,
            "ambience evicted a gameplay event before refusing admission"
        );
    }

    #[test]
    fn ambience_at_a_full_world_budget_preserves_every_gameplay_player() {
        ambience_preserves_gameplay(MAX_VOICES);
    }

    #[test]
    fn ambience_at_an_event_limit_preserves_the_gameplay_player() {
        ambience_preserves_gameplay(1);
    }

    #[test]
    fn a_finished_player_does_not_erase_the_target_merge_window() {
        let mut app = an_app();
        let world = app.world_mut();
        world.insert_resource(mixer(vec![event("hit", false, 120)]));
        let mut blow = request("hit", Owner::None);
        blow.merge = Some(EntityId {
            idx: 90,
            generation: 1,
        });
        start_and_finish(world, &blow);
        let repeated = world.resource_scope(|world, mut mixer: Mut<Mixer>| {
            mixer.start(
                &mut world.commands(),
                Duration::from_millis(100),
                &blow,
                Handle::default(),
            )
        });
        assert_eq!(
            repeated,
            Started::Refused,
            "completion released a still-active merge block"
        );
    }

    #[test]
    fn completed_trigger_history_is_bounded_and_expiry_reopens_admission() {
        let mut app = an_app();
        let world = app.world_mut();
        for index in 0..128 {
            let event = format!("missing-{index}");
            start_and_finish(world, &request(&event, Owner::None));
        }
        let overflow = request("overflow", Owner::None);
        let refused = world.resource_scope(|world, mut mixer: Mut<Mixer>| {
            mixer.start(
                &mut world.commands(),
                Duration::ZERO,
                &overflow,
                Handle::default(),
            )
        });
        assert_eq!(
            refused,
            Started::Refused,
            "completed triggers had no history bound"
        );
        let after_expiry = world.resource_scope(|world, mut mixer: Mut<Mixer>| {
            mixer.start(
                &mut world.commands(),
                Duration::from_millis(50),
                &overflow,
                Handle::default(),
            )
        });
        assert_eq!(
            after_expiry,
            Started::Once,
            "history did not expire at its exact deadline"
        );
    }

    #[test]
    fn history_saturation_refuses_before_evicting_an_existing_event_instance() {
        let mut app = an_app();
        let world = app.world_mut();
        let mut limited = event("limited", false, 120);
        limited.limit = 1;
        world.insert_resource(mixer(vec![limited]));
        for index in 0..MAX_RECENT_GAMEPLAY {
            start_and_finish(world, &request(&format!("unknown-{index}"), Owner::None));
        }
        let started = world.resource_scope(|world, mut mixer: Mut<Mixer>| {
            mixer.start(
                &mut world.commands(),
                Duration::ZERO,
                &request("limited", Owner::None),
                Handle::default(),
            )
        });
        assert_eq!(
            started,
            Started::Once,
            "a nonmerging known event needs no history slot"
        );
        world.flush();
        let before: HashSet<_> = world
            .query_filtered::<Entity, With<AudioPlayer>>()
            .iter(world)
            .collect();
        let mut merging = request("limited", Owner::None);
        merging.merge = Some(EntityId {
            idx: 90,
            generation: 1,
        });
        let refused = world.resource_scope(|world, mut mixer: Mut<Mixer>| {
            mixer.start(
                &mut world.commands(),
                Duration::ZERO,
                &merging,
                Handle::default(),
            )
        });
        world.flush();
        let after: HashSet<_> = world
            .query_filtered::<Entity, With<AudioPlayer>>()
            .iter(world)
            .collect();
        assert_eq!(refused, Started::Refused);
        assert_eq!(
            after, before,
            "history rejection happened after event-limit eviction"
        );
    }

    #[test]
    fn ambient_trigger_history_does_not_consume_gameplay_history() {
        let mut app = an_app();
        let world = app.world_mut();
        for index in 0..MAX_RECENT_AMBIENCE {
            let name = format!("ambient-{index}");
            let started = world.resource_scope(|world, mut mixer: Mut<Mixer>| {
                mixer.start_ambience(
                    &mut world.commands(),
                    Duration::ZERO,
                    &request(&name, Owner::None),
                    Handle::default(),
                )
            });
            assert_eq!(started, Started::Once);
            world.flush();
            let player = world
                .query_filtered::<Entity, With<AudioPlayer>>()
                .single(world)
                .unwrap();
            world.despawn(player);
        }
        let overflow = world.resource_scope(|world, mut mixer: Mut<Mixer>| {
            mixer.start_ambience(
                &mut world.commands(),
                Duration::ZERO,
                &request("ambient-overflow", Owner::None),
                Handle::default(),
            )
        });
        assert_eq!(
            overflow,
            Started::Refused,
            "background trigger history was not bounded"
        );
        for index in 0..MAX_RECENT_GAMEPLAY {
            start_and_finish(world, &request(&format!("world-{index}"), Owner::None));
        }
        let overflow = world.resource_scope(|world, mut mixer: Mut<Mixer>| {
            mixer.start(
                &mut world.commands(),
                Duration::ZERO,
                &request("world-overflow", Owner::None),
                Handle::default(),
            )
        });
        assert_eq!(overflow, Started::Refused);
    }

    #[test]
    fn closing_the_match_clears_completed_trigger_merge_blocks() {
        let mut app = an_app();
        let world = app.world_mut();
        world.insert_resource(mixer(vec![event("hit", false, 120)]));
        let mut blow = request("hit", Owner::None);
        blow.merge = Some(EntityId {
            idx: 90,
            generation: 1,
        });
        start_and_finish(world, &blow);
        let blocked = world.resource_scope(|world, mut mixer: Mut<Mixer>| {
            mixer.start(
                &mut world.commands(),
                Duration::from_millis(100),
                &blow,
                Handle::default(),
            )
        });
        assert_eq!(
            blocked,
            Started::Refused,
            "the current match must really hold a merge block"
        );
        let next_match = world.resource_scope(|world, mut mixer: Mut<Mixer>| {
            mixer.close(&mut world.commands());
            mixer.open();
            mixer.start(
                &mut world.commands(),
                Duration::from_millis(100),
                &blow,
                Handle::default(),
            )
        });
        assert_eq!(
            next_match,
            Started::Once,
            "a new match inherited the old target's merge block"
        );
    }

    #[test]
    fn imported_repeat_blocks_count_sounding_instances_not_finished_triggers() {
        let mut app = an_app();
        let world = app.world_mut();
        let mut event = event("known", false, 120);
        event.block = Duration::from_secs(1);
        world.insert_resource(mixer(vec![event]));
        let first = world.resource_scope(|world, mut mixer: Mut<Mixer>| {
            mixer.start(
                &mut world.commands(),
                Duration::ZERO,
                &request("known", Owner::None),
                Handle::default(),
            )
        });
        assert_eq!(first, Started::Once);
        world.flush();
        let blocked = world.resource_scope(|world, mut mixer: Mut<Mixer>| {
            mixer.start(
                &mut world.commands(),
                Duration::from_millis(100),
                &request("known", Owner::None),
                Handle::default(),
            )
        });
        assert_eq!(
            blocked,
            Started::Refused,
            "a sounding instance lost its imported block"
        );
        let player = world
            .query_filtered::<Entity, With<AudioPlayer>>()
            .single(world)
            .unwrap();
        world.despawn(player);
        let finished = world.resource_scope(|world, mut mixer: Mut<Mixer>| {
            mixer.start(
                &mut world.commands(),
                Duration::from_millis(100),
                &request("known", Owner::None),
                Handle::default(),
            )
        });
        assert_eq!(
            finished,
            Started::Once,
            "imported sounding-instance policy became trigger history"
        );
    }

    /// The world's voices are bounded, fading ones counted: a one-shot gives way to a new voice,
    /// the loops never do, and a world of loops has no room.
    #[test]
    fn the_cap_counts_every_voice_steals_one_shots_and_never_a_loop() {
        let mut world = World::new();
        let mut mixer = mixer(vec![event("loop", true, 1), event("shot", false, 1)]);
        let mut commands = world.commands();
        let now = Duration::ZERO;
        for n in 0..MAX_VOICES - 1 {
            let started = mixer.start(
                &mut commands,
                now,
                &request("loop", owner(n as u32)),
                Handle::default(),
            );
            assert!(matches!(started, Started::Loop(_)), "loop {n}");
        }
        let once = mixer.start(
            &mut commands,
            now,
            &request("shot", Owner::None),
            Handle::default(),
        );
        assert_eq!(once, Started::Once);
        assert_eq!(mixer.voices.len(), MAX_VOICES);
        let stolen = mixer.start(
            &mut commands,
            now,
            &request("loop", owner(900)),
            Handle::default(),
        );
        assert!(
            matches!(stolen, Started::Loop(_)),
            "a loop takes a one-shot's place"
        );
        assert_eq!(mixer.sounding("shot"), 0, "the one-shot was stolen");
        let refused = mixer.start(
            &mut commands,
            now,
            &request("shot", Owner::None),
            Handle::default(),
        );
        assert_eq!(refused, Started::Refused, "no loop is stolen");
        assert_eq!(mixer.voices.len(), MAX_VOICES);
    }

    fn mixer_with_groups_and_layers() -> Mixer {
        let (groups, layers) = crate::cache_text::mixer_lines(
            "group Weapons 0.4 1 0 0 0 90 60\n\
             group Announcer 0.4 1 0 0 0 90 90\n\
             group Creeps 0.2 1 0 0 0 80 90\n\
             layer Announcer Announcer 0.4 1.0 0.3 0.5\n\
             layerlevel Announcer Weapons 0.7 1 0\n\
             layerlevel Announcer Creeps 0.5 0 0\n",
            64,
        );
        let mut mixer = mixer(vec![
            EventLine {
                group: Some("Weapons".to_owned()),
                sound_level: 81.0,
                distance_max: Some(2500.0),
                ..event("Gun", false, 1)
            },
            EventLine {
                group: Some("Creeps".to_owned()),
                sound_level: 72.0,
                ..event("Club", false, 1)
            },
        ]);
        mixer.groups = groups.into_iter().map(|g| (g.name.clone(), g)).collect();
        mixer.layers = layers.into_iter().map(LayerState::new).collect();
        mixer
    }

    /// The share of a sound that reaches the sink beside its source, in its group.
    #[test]
    fn a_mix_group_scales_its_events_by_its_level_and_the_makeup_gain() {
        let mixer = mixer_with_groups_and_layers();
        let near = |event| mixer.gain_for(event, (0.0, 0.0), (0.0, 0.0));
        assert!((near("Gun") - 0.4 * MIX_MAKEUP_GAIN).abs() < 1e-5);
        assert!((near("Club") - 0.2 * MIX_MAKEUP_GAIN).abs() < 1e-5);
        assert_eq!(
            near("Unknown"),
            1.0,
            "an event of no group is not mixed down"
        );
    }

    /// Dota's `Announcer` layer: while the announcer speaks, the world's weapons go to 0.7 of
    /// their level over the layer's attack, and creeps to half (and their reach to nothing),
    /// back to full over its release.
    #[test]
    fn the_announcer_ducks_the_world_while_it_speaks_and_lets_go_after() {
        let mut mixer = mixer_with_groups_and_layers();
        let beside = |mixer: &Mixer| mixer.gain_for("Gun", (0.0, 0.0), (0.0, 0.0));
        let full = beside(&mixer);
        let mut now = Duration::ZERO;
        let mut step = |mixer: &mut Mixer, speaking: bool, seconds: f32| {
            if speaking {
                mixer.note_activity("Announcer", 1.0);
            }
            now += Duration::from_secs_f32(seconds);
            mixer.move_layers(now);
        };
        step(&mut mixer, true, 0.0);
        assert_eq!(beside(&mixer), full, "not at the first look");
        step(&mut mixer, true, 0.15);
        let halfway = mixer.layer_amount("Announcer").expect("the layer");
        assert!((halfway - 0.5).abs() < 1e-3, "attack 0.3 s: {halfway}");
        step(&mut mixer, true, 0.3);
        assert!((beside(&mixer) - full * 0.7).abs() < 1e-4, "ducked to 0.7");
        let creeps = |mixer: &Mixer| mixer.gain_for("Club", (200.0, 0.0), (0.0, 0.0));
        assert_eq!(creeps(&mixer), 0.0, "creeps carry nowhere while it speaks");
        step(&mut mixer, false, 0.25);
        assert!(
            (mixer.layer_amount("Announcer").unwrap() - 0.5).abs() < 1e-3,
            "release 0.5 s"
        );
        step(&mut mixer, false, 0.5);
        assert!((beside(&mixer) - full).abs() < 1e-5, "let go");
        assert!(creeps(&mixer) > 0.0);
    }

    #[test]
    fn a_quiet_announcer_below_the_threshold_ducks_nothing() {
        let mut mixer = mixer_with_groups_and_layers();
        let mut now = Duration::ZERO;
        for _ in 0..20 {
            mixer.note_activity("Announcer", 0.5);
            now += Duration::from_millis(100);
            mixer.move_layers(now);
        }
        assert_eq!(mixer.layer_amount("Announcer"), Some(0.0));
    }

    /// A sound beside the player's own entity carries the event's factor for it.
    #[test]
    fn the_players_own_entity_sounds_at_the_events_player_factor() {
        let mut world = World::new();
        let mut mixer = mixer(vec![EventLine {
            player_mult: 1.25,
            ..event("Own", false, 1)
        }]);
        let mut commands = world.commands();
        let request = Request {
            on_player: true,
            ..request("Own", Owner::None)
        };
        mixer.start(&mut commands, Duration::ZERO, &request, Handle::default());
        assert_eq!(mixer.voice_gain("Own"), Some(1.25));
    }

    /// An event that follows the fog of war is not heard where its source is out of sight.
    #[test]
    fn an_event_that_follows_the_fog_is_heard_only_where_its_source_is_in_sight() {
        let cell = crate::terrain::CELL;
        let fog = FogGrid::with_seen_cells(4, &[5]);
        let mut world = World::new();
        let mut mixer = mixer(vec![EventLine {
            fog_radius: Some(0.0),
            ..event("Seen", false, 1)
        }]);
        let mut commands = world.commands();
        let heard_at = |mixer: &mut Mixer, commands: &mut Commands, x: f32| {
            let request = Request {
                at: Some((x, 1.5 * cell)),
                fog: Some(&fog),
                ..request("Seen", Owner::None)
            };
            mixer.start(commands, Duration::ZERO, &request, Handle::default())
        };
        assert_eq!(
            heard_at(&mut mixer, &mut commands, 1.5 * cell),
            Started::Once
        );
        let mut mixer = mixer_again(&mixer);
        assert_eq!(
            heard_at(&mut mixer, &mut commands, 3.5 * cell),
            Started::Refused
        );
    }

    /// A sound of no mix group is the interface's: it sounds at its own level, not made up.
    #[test]
    fn an_event_of_no_group_sounds_at_its_own_volume_without_the_makeup_gain() {
        let mixer = mixer(vec![EventLine {
            volume: 0.3,
            ..event("Click", false, 1)
        }]);
        let heard = mixer.gain_for("Click", (0.0, 0.0), (0.0, 0.0));
        assert!((heard - 0.3).abs() < 1e-6, "{heard}");
    }

    /// A loop that follows its source and its listener keeps its event's volume and carries as
    /// the event says: half way to its `distance_max` it is half as loud.
    #[test]
    fn a_followed_loop_carries_by_its_events_distance_and_keeps_its_volume() {
        let mut world = World::new();
        let mut mixer = mixer(vec![EventLine {
            volume: 0.5,
            sound_level: 81.0,
            distance_max: Some(1000.0),
            ..event("loop", true, 1)
        }]);
        let mut commands = world.commands();
        let Started::Loop(voice) = mixer.start(
            &mut commands,
            Duration::ZERO,
            &request("loop", owner(1)),
            Handle::default(),
        ) else {
            panic!("a loop");
        };
        mixer.follow(voice, (500.0, 0.0), (0.0, 0.0));
        mixer.update_voices(Duration::from_millis(10), None);
        assert_eq!(mixer.loop_gain("loop"), Some(0.25));
        mixer.follow(voice, (1500.0, 0.0), (0.0, 0.0));
        mixer.update_voices(Duration::from_millis(20), None);
        assert_eq!(mixer.loop_gain("loop"), Some(0.0), "past its distance_max");
    }

    /// A loop whose source moves out of sight goes quiet over a second, and comes back as it
    /// comes into sight: the fog is looked at every frame, not only when it starts.
    #[test]
    fn a_loop_whose_source_moves_into_the_fog_goes_quiet_and_returns_in_sight() {
        let cell = crate::terrain::CELL;
        let fog = FogGrid::with_seen_cells(4, &[5]);
        let (seen, unseen) = ((1.5 * cell, 1.5 * cell), (3.5 * cell, 3.5 * cell));
        let mut world = World::new();
        let mut mixer = mixer(vec![EventLine {
            fog_radius: Some(0.0),
            ..event("loop", true, 1)
        }]);
        let mut commands = world.commands();
        let request = Request {
            at: Some(seen),
            fog: Some(&fog),
            ..request("loop", owner(1))
        };
        let Started::Loop(voice) =
            mixer.start(&mut commands, Duration::ZERO, &request, Handle::default())
        else {
            panic!("a loop in sight");
        };
        let mut now = Duration::ZERO;
        let mut look = |mixer: &mut Mixer, at: (f32, f32), seconds: f32| {
            mixer.follow(voice, at, at);
            now += Duration::from_secs_f32(seconds);
            mixer.update_voices(now, Some(&fog));
            mixer.fog_share(voice).expect("the voice")
        };
        assert_eq!(look(&mut mixer, seen, 0.5), 1.0);
        assert!(
            (look(&mut mixer, unseen, 0.5) - 0.5).abs() < 1e-4,
            "half a second out of sight"
        );
        assert_eq!(look(&mut mixer, unseen, 0.6), 0.0, "silent");
        assert!(
            (look(&mut mixer, seen, 0.25) - 0.25).abs() < 1e-4,
            "coming back"
        );
        assert_eq!(look(&mut mixer, seen, 2.0), 1.0, "audible again");
    }

    /// A nonzero fog radius hears a source out of sight whose circle touches seen ground, and not
    /// one whose circle does not.
    #[test]
    fn a_fog_radius_hears_a_source_beside_seen_ground_and_not_one_far_from_it() {
        let cell = crate::terrain::CELL;
        let fog = FogGrid::with_seen_cells(4, &[5]);
        let mut world = World::new();
        let mut commands = world.commands();
        let mut started = |radius: f32, x: f32| {
            let mut mixer = mixer(vec![EventLine {
                fog_radius: Some(radius),
                ..event("Seen", false, 1)
            }]);
            let request = Request {
                at: Some((x, 1.5 * cell)),
                fog: Some(&fog),
                ..request("Seen", Owner::None)
            };
            mixer.start(&mut commands, Duration::ZERO, &request, Handle::default())
        };
        let beside = 0.5 * cell; // half a cell west of the seen cell
        assert_eq!(started(0.0, beside), Started::Refused);
        assert_eq!(started(0.4 * cell, beside), Started::Refused);
        assert_eq!(started(0.6 * cell, beside), Started::Once);
    }

    /// A layer with several triggers moves each at its own attack: the layer is as on as the
    /// most-on of them.
    #[test]
    fn each_trigger_of_a_layer_ramps_at_its_own_attack() {
        let (groups, layers) = crate::cache_text::mixer_lines(
            "group Weapons 1 1 0 0 0 90 60\n\
             layer Duck Slow 0.1 1.0 1.0 1.0\n\
             layer Duck Fast 0.1 1.0 0.1 0.1\n\
             layerlevel Duck Weapons 0.5 1 0\n",
            64,
        );
        let mut mixer = mixer(vec![event("x", false, 1)]);
        mixer.groups = groups.into_iter().map(|g| (g.name.clone(), g)).collect();
        mixer.layers = layers.into_iter().map(LayerState::new).collect();
        let mut now = Duration::ZERO;
        for _ in 0..2 {
            mixer.note_activity("Fast", 1.0);
            now += Duration::from_millis(50);
            mixer.move_layers(now);
        }
        let amount = mixer.layer_amount("Duck").expect("the layer");
        assert!(
            (amount - 0.5).abs() < 1e-3,
            "the fast trigger's pace, not the slow one's: {amount}"
        );
        mixer.move_layers(now + Duration::from_millis(100));
        assert!(
            mixer.layer_amount("Duck").unwrap() < 0.01,
            "released at the fast trigger's pace"
        );
    }

    /// A loop with no owner would sound for ever with nothing to stop it: it plays once.
    #[test]
    fn a_loop_with_no_owner_plays_once_for_its_length() {
        let mut world = World::new();
        let mut mixer = mixer(vec![event("loop", true, 3)]);
        let mut commands = world.commands();
        let started = mixer.start(
            &mut commands,
            Duration::ZERO,
            &request("loop", Owner::None),
            Handle::default(),
        );
        assert_eq!(started, Started::Once);
        assert_eq!(mixer.voices[0].ends, Some(Duration::from_secs(3)));
    }

    /// An event's limit stops its oldest one-shot for a new voice, or refuses the new one; it
    /// never stops a loop.
    #[test]
    fn a_limit_steals_the_oldest_voice_or_refuses_the_new_one() {
        let mut world = World::new();
        let mut stealing = event("shot", false, 5);
        stealing.limit = 2;
        let mut keeping = event("kept", false, 5);
        keeping.limit = 1;
        keeping.stop_oldest = false;
        let mut mixer = mixer(vec![stealing, keeping]);
        let mut commands = world.commands();
        for second in 0..3 {
            let now = Duration::from_secs(second);
            let started = mixer.start(
                &mut commands,
                now,
                &request("shot", Owner::None),
                Handle::default(),
            );
            assert_eq!(started, Started::Once);
        }
        assert_eq!(mixer.sounding("shot"), 2);
        assert_eq!(
            mixer.voices[0].started,
            Duration::from_secs(1),
            "the oldest went"
        );
        let now = Duration::ZERO;
        let first = mixer.start(
            &mut commands,
            now,
            &request("kept", Owner::None),
            Handle::default(),
        );
        let second = mixer.start(
            &mut commands,
            now,
            &request("kept", Owner::None),
            Handle::default(),
        );
        assert_eq!((first, second), (Started::Once, Started::Refused));
    }

    /// A limit never ends a loop whose owner is still wearing the effect: the new voice is
    /// refused instead.
    #[test]
    fn a_limit_refuses_a_new_voice_rather_than_steal_a_loop() {
        let mut world = World::new();
        let mut limited = event("loop", true, 1);
        limited.limit = 1;
        let mut mixer = mixer(vec![limited]);
        let mut commands = world.commands();
        let now = Duration::ZERO;
        let first = mixer.start(
            &mut commands,
            now,
            &request("loop", owner(1)),
            Handle::default(),
        );
        let second = mixer.start(
            &mut commands,
            now,
            &request("loop", owner(2)),
            Handle::default(),
        );
        assert!(matches!(first, Started::Loop(_)));
        assert_eq!(second, Started::Refused);
        assert_eq!(mixer.sounding("loop"), 1, "the first loop was stolen");
    }
}
