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
//! the voice lines (`voice.rs`) and the announcer.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use bevy::{
    audio::{AudioSink, AudioSinkPlayback, PlaybackSettings, Volume},
    prelude::*,
};
use bota_proto::EntityId;

use crate::cache_text::EventLine;
use crate::manifest::Manifest;

/// Voices sounding at once at most, fading ones included: the oldest one-shot is stopped for a
/// new voice, and a voice is refused when only loops remain.
pub const MAX_VOICES: usize = 64;
/// How long the blows on one unit merge into one sound: the owner's rule, half a second of a
/// unit's pain is one sound, not each blow in full.
pub const MERGE_WINDOW: Duration = Duration::from_millis(500);
/// How long a sound of an event the cache does not know counts as sounding.
const UNKNOWN_LENGTH: Duration = Duration::from_secs(10);
/// Most events read from the cache.
const MAX_EVENTS: usize = 1 << 14;

/// Whom a looping sound sounds for: it ends with that unit's wearing of the effect it sounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// Nobody: a loop that has none plays once for its length, and is told as a fault.
    None,
    Unit(EntityId),
}

/// What asks to be sounded.
pub struct Request<'a> {
    /// The Dota event the sound plays; one the cache does not know has no limits.
    pub event: &'a str,
    pub owner: Owner,
    /// The unit a blow lands on: blows on one unit merge.
    pub merge: Option<EntityId>,
    /// Where it sounds, in the world; none for a sound heard wherever the camera is.
    pub at: Option<(f32, f32)>,
    pub gain: f32,
    pub speed: f32,
}

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
    /// The events a loop with no owner was refused for, told once each.
    warned: HashSet<String>,
}

/// One sound sounding.
struct Voice {
    entity: Entity,
    event: String,
    looping: bool,
    /// Whose sound it is, for the limits that count an entity's own.
    owner: Owner,
    merge: Option<EntityId>,
    at: Option<(f32, f32)>,
    started: Duration,
    /// When it ends by itself; a loop never.
    ends: Option<Duration>,
    /// Whether its entity is despawned at the end (its length is known, or it loops).
    despawn: bool,
    /// The start of its fade-out once stopped, and how long the fade lasts.
    stopped: Option<(Duration, Duration)>,
    /// The event's own volume, which every gain the voice has is of.
    volume: f32,
    gain: f32,
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

    /// Reads `sounds/events.txt` of the cache; a cache without it limits nothing, and says so.
    pub fn load(manifest: &Manifest) -> Mixer {
        let text = crate::cachefs::read_small(&manifest.root, "sounds/events.txt")
            .and_then(|bytes| String::from_utf8(bytes).ok());
        let Some(text) = text else {
            warn!("sounds/events.txt is missing from the cache: no sound is limited");
            return Mixer::default();
        };
        let events = crate::cache_text::sound_events(&text, MAX_EVENTS);
        Mixer {
            events: events.into_iter().map(|e| (e.name.clone(), e)).collect(),
            ..Mixer::default()
        }
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
        self.drop_ended(commands, now);
        let rules = self.events.get(request.event).cloned();
        let looping = rules.as_ref().is_some_and(|r| r.looping);
        if looping && request.owner == Owner::None && self.warned.insert(request.event.to_owned()) {
            warn!("{}: a loop with no owner plays once", request.event);
        }
        let kept = looping && request.owner != Owner::None;
        if self.blocked(request, rules.as_ref(), now)
            || !self.make_room(commands, now, request, rules.as_ref())
        {
            return Started::Refused;
        }
        let (ends, despawn) = lifetime(rules.as_ref(), kept);
        let volume = rules.as_ref().map_or(1.0, |r| r.volume);
        let gain = request.gain * volume;
        let playback = if kept {
            PlaybackSettings::LOOP
        } else {
            PlaybackSettings::DESPAWN
        };
        let entity = commands
            .spawn((
                AudioPlayer::new(source),
                playback
                    .with_volume(Volume::Linear(gain))
                    .with_speed(request.speed),
            ))
            .id();
        self.voices.push(Voice {
            entity,
            event: request.event.to_owned(),
            looping: kept,
            owner: request.owner,
            merge: request.merge,
            at: request.at,
            started: now,
            ends: ends.map(|length| now + length),
            despawn,
            stopped: None,
            volume,
            gain,
        });
        if kept {
            Started::Loop(entity)
        } else {
            Started::Once
        }
    }

    /// Whether the event's block (a repeat within its time and distance of one sounding) or the
    /// merge of blows on one unit keeps the request from sounding.
    fn blocked(&self, request: &Request, rules: Option<&EventLine>, now: Duration) -> bool {
        let block = rules.map_or(Duration::ZERO, |r| r.block);
        let units = rules.map_or(0.0, |r| r.block_units);
        self.voices
            .iter()
            .filter(|v| v.event == request.event && v.sounding())
            .any(|v| {
                let age = now.saturating_sub(v.started);
                let near = match (v.at, request.at) {
                    (Some(a), Some(b)) if units > 0.0 => {
                        Vec2::from(a).distance(Vec2::from(b)) <= units
                    }
                    _ => true,
                };
                let merged = request.merge.is_some() && v.merge == request.merge;
                (age < block && near) || (age < MERGE_WINDOW && merged)
            })
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
    ) -> bool {
        if let Some(rules) = rules.filter(|r| r.limit > 0) {
            let counted = |v: &Voice| {
                v.event == request.event
                    && v.sounding()
                    && (!rules.per_entity || v.owner == request.owner)
            };
            if self.voices.iter().filter(|v| counted(v)).count() >= rules.limit as usize {
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
        // A fading voice is the first to go, then the oldest one-shot; a loop is never stolen.
        let stolen = self
            .voices
            .iter()
            .position(|v| !v.sounding())
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

    /// Moves a loop with its source: where it sounds, and how loud that place is to the listener.
    pub fn follow(&mut self, entity: Entity, at: (f32, f32), gain: f32) {
        if let Some(voice) = self.voices.iter_mut().find(|v| v.entity == entity) {
            voice.at = Some(at);
            voice.gain = gain * voice.volume;
        }
    }

    /// Takes the voices that ended out, sets the loops' volumes and fades those being stopped.
    pub fn tick(
        &mut self,
        commands: &mut Commands,
        now: Duration,
        sinks: &mut Query<&mut AudioSink>,
    ) {
        self.drop_ended(commands, now);
        for voice in self.voices.iter().filter(|v| v.looping || !v.sounding()) {
            let Ok(mut sink) = sinks.get_mut(voice.entity) else {
                continue;
            };
            let left = voice.stopped.map_or(1.0, |(since, fade)| {
                1.0 - (now.saturating_sub(since).as_secs_f32() / fade.as_secs_f32()).clamp(0.0, 1.0)
            });
            sink.set_volume(Volume::Linear(voice.gain * left));
        }
    }

    fn drop_ended(&mut self, commands: &mut Commands, now: Duration) {
        self.voices.retain(|voice| {
            let faded = voice
                .stopped
                .is_some_and(|(since, fade)| now.saturating_sub(since) >= fade);
            let over = faded || voice.ends.is_some_and(|ends| now >= ends);
            if over && (voice.despawn || voice.stopped.is_some()) {
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

/// How long a voice lasts by itself (none for a loop) and whether its entity is despawned then:
/// the event's length, cut short by its stop time; a sound of unknown length is forgotten after
/// [`UNKNOWN_LENGTH`] and left to finish by itself.
fn lifetime(rules: Option<&EventLine>, kept: bool) -> (Option<Duration>, bool) {
    let length = rules.map(|r| r.duration).filter(|d| !d.is_zero());
    let stop = rules.and_then(|r| r.stop_at);
    let ends = match (kept, length, stop) {
        (true, _, stop) => stop,
        (false, Some(length), Some(stop)) => Some(length.min(stop)),
        (false, Some(length), None) => Some(length),
        (false, None, Some(stop)) => Some(stop),
        (false, None, None) => Some(UNKNOWN_LENGTH),
    };
    (ends, kept || length.is_some() || stop.is_some())
}

/// Reads the cache's events at startup.
pub fn plugin(app: &mut App) {
    app.init_resource::<Mixer>()
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
             mut sinks: Query<&mut AudioSink>| {
                mixer.tick(&mut commands, time.elapsed(), &mut sinks);
            },
        );
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
        }
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
        }
    }

    fn owner(n: u32) -> Owner {
        Owner::Unit(EntityId {
            idx: n,
            generation: 1,
        })
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

    /// A loop that follows its source keeps its event's volume: the camera's distance is of the
    /// event's level, not of the file's.
    #[test]
    fn a_followed_loop_keeps_the_volume_of_its_event() {
        let mut world = World::new();
        let mut quiet = event("loop", true, 1);
        quiet.volume = 0.5;
        let mut mixer = mixer(vec![quiet]);
        let mut commands = world.commands();
        let started = mixer.start(
            &mut commands,
            Duration::ZERO,
            &request("loop", owner(1)),
            Handle::default(),
        );
        let Started::Loop(voice) = started else {
            panic!("a loop");
        };
        mixer.follow(voice, (0.0, 0.0), 0.8);
        assert_eq!(mixer.loop_gain("loop"), Some(0.4));
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
