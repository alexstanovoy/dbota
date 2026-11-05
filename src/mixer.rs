//! The voices of the sounds in the world, kept as Dota keeps them. The install's sound events
//! (`sounds/events.txt`, read from the `soundevents` and the event types of the install by the
//! importer) say how many instances of an event may sound at once and which repeats are
//! blocked; a sound that loops sounds until its source stops it. Every sound of the world starts
//! through [`Mixer::start`] and a looping one ends through [`Mixer::stop`], so that nothing
//! sounds without a bound and nothing outlives what it sounds for.

use std::collections::HashMap;

use bevy::{
    audio::{AudioSink, AudioSinkPlayback, PlaybackSettings, Volume},
    prelude::*,
};
use bota_proto::EntityId;

use crate::cache_text::EventLine;
use crate::manifest::Manifest;

/// Voices sounding at once at most, whatever the events allow: the oldest is stopped for a new one.
pub const MAX_VOICES: usize = 64;
/// Seconds within which the blows on one unit sound once: the owner's rule, 500 ms of a unit's
/// pain are one sound, not each blow in full.
pub const MERGE_WINDOW: f64 = 0.5;
/// Seconds a sound of an event the cache does not know counts as sounding.
const UNKNOWN_LENGTH: f64 = 10.0;
/// Most events read from the cache.
const MAX_EVENTS: usize = 1 << 14;

/// The sound events of the cache, by name, and the voices sounding now.
#[derive(Resource, Default)]
pub struct Mixer {
    events: HashMap<String, EventLine>,
    voices: Vec<Voice>,
}

/// One sound sounding.
struct Voice {
    entity: Entity,
    event: String,
    /// Whose sound it is, for the limits that count an entity's own.
    source: Option<EntityId>,
    /// Whom it is about, for the blows that merge.
    target: Option<EntityId>,
    at: Option<(f32, f32)>,
    started: f64,
    /// When it ends by itself: its length, a loop never; a sound of unknown length is forgotten
    /// after [`UNKNOWN_LENGTH`] and left to finish by itself.
    ends: f64,
    /// Whether the voice's entity is despawned at the end (its length is known, or it loops).
    despawn: bool,
    /// The start of its fade-out, once stopped, and the length of the fade.
    stopped: Option<(f64, f32)>,
    gain: f32,
}

/// What asks to be sounded.
pub struct Request<'a> {
    /// The Dota event the sound plays; one the cache does not know has no limits.
    pub event: &'a str,
    pub source: Option<EntityId>,
    /// Whom the sound is about: blows on one unit merge.
    pub target: Option<EntityId>,
    /// Where it sounds, in the world; none for a sound heard wherever the camera is.
    pub at: Option<(f32, f32)>,
    pub gain: f32,
    pub speed: f32,
}

impl Mixer {
    /// Reads `sounds/events.txt` of the cache; a cache without it limits nothing.
    pub fn load(manifest: &Manifest) -> Mixer {
        let events = crate::cachefs::read_small(&manifest.root, "sounds/events.txt")
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .map(|text| crate::cache_text::sound_events(&text, MAX_EVENTS))
            .unwrap_or_default();
        Mixer {
            events: events.into_iter().map(|e| (e.name.clone(), e)).collect(),
            voices: Vec::new(),
        }
    }

    /// Sounds `source` as the event asks: not at all if a blocked repeat or a blow merged into one
    /// sounding, after stopping the oldest voice if the limit is reached. Returns the voice, which
    /// a looping sound's source stops with [`Mixer::stop`].
    pub fn start(
        &mut self,
        commands: &mut Commands,
        now: f64,
        request: &Request,
        source: Handle<AudioSource>,
    ) -> Option<Entity> {
        let rules = self.events.get(request.event).cloned();
        self.drop_ended(commands, now);
        if self.blocked(request, rules.as_ref(), now) {
            return None;
        }
        if let Some(rules) = rules.as_ref().filter(|r| r.limit > 0) {
            let counted = |v: &&Voice| {
                v.event == request.event
                    && v.stopped.is_none()
                    && (!rules.per_entity || v.source == request.source)
            };
            if self.voices.iter().filter(counted).count() >= rules.limit as usize {
                if !rules.stop_oldest {
                    return None;
                }
                let oldest = self.voices.iter().position(|v| counted(&v));
                self.stop_at(commands, oldest, now);
            }
        }
        if self.voices.iter().filter(|v| v.stopped.is_none()).count() >= MAX_VOICES {
            let oldest = self.voices.iter().position(|v| v.stopped.is_none());
            self.stop_at(commands, oldest, now);
        }
        let looping = rules.as_ref().is_some_and(|r| r.looping);
        let length = rules.as_ref().map_or(0.0, |r| f64::from(r.duration));
        let by_time = rules
            .as_ref()
            .filter(|r| r.stop_at >= 0.0)
            .map(|r| f64::from(r.stop_at));
        let ends = match (looping, by_time) {
            (true, None) => f64::INFINITY,
            (true, Some(stop)) => stop,
            (false, Some(stop)) if length > 0.0 => stop.min(length),
            (false, Some(stop)) => stop,
            (false, None) if length > 0.0 => length,
            (false, None) => UNKNOWN_LENGTH,
        };
        let despawn = looping || by_time.is_some() || length > 0.0;
        let playback = if looping {
            PlaybackSettings::LOOP
        } else {
            PlaybackSettings::DESPAWN
        };
        let entity = commands
            .spawn((
                AudioPlayer::new(source),
                playback
                    .with_volume(Volume::Linear(request.gain))
                    .with_speed(request.speed),
            ))
            .id();
        self.voices.push(Voice {
            entity,
            event: request.event.to_owned(),
            source: request.source,
            target: request.target,
            at: request.at,
            started: now,
            ends: now + ends,
            despawn,
            stopped: None,
            gain: request.gain,
        });
        Some(entity)
    }

    /// Whether the event's block (a repeat within its seconds and units of one sounding) or the
    /// merge of blows on one unit keeps the request from sounding.
    fn blocked(&self, request: &Request, rules: Option<&EventLine>, now: f64) -> bool {
        let block = rules.map_or(0.0, |r| f64::from(r.block));
        let units = rules.map_or(0.0, |r| r.block_units);
        self.voices
            .iter()
            .filter(|v| v.event == request.event && v.stopped.is_none())
            .any(|v| {
                let age = now - v.started;
                let near = match (v.at, request.at) {
                    (Some(a), Some(b)) => Vec2::from(a).distance(Vec2::from(b)) <= units,
                    _ => true,
                };
                let merged = request.target.is_some() && v.target == request.target;
                (age < block && near) || (age < MERGE_WINDOW && merged)
            })
    }

    /// Stops a voice: it fades out over its event's fade-out, or at once without one.
    pub fn stop(&mut self, commands: &mut Commands, now: f64, entity: Entity) {
        let at = self.voices.iter().position(|v| v.entity == entity);
        self.stop_at(commands, at, now);
    }

    fn stop_at(&mut self, commands: &mut Commands, at: Option<usize>, now: f64) {
        let Some(at) = at else {
            return;
        };
        let fade = self
            .events
            .get(&self.voices[at].event)
            .map_or(0.0, |e| e.fade_out);
        if fade > 0.0 && self.voices[at].stopped.is_none() {
            self.voices[at].stopped = Some((now, fade));
            return;
        }
        let voice = self.voices.remove(at);
        commands.entity(voice.entity).try_despawn();
    }

    /// Takes the voices that ended out, and fades those being stopped.
    pub fn tick(&mut self, commands: &mut Commands, now: f64, sinks: &mut Query<&mut AudioSink>) {
        self.drop_ended(commands, now);
        for voice in &self.voices {
            if let Some((since, fade)) = voice.stopped
                && let Ok(mut sink) = sinks.get_mut(voice.entity)
            {
                let left = 1.0 - ((now - since) as f32 / fade).clamp(0.0, 1.0);
                sink.set_volume(Volume::Linear(voice.gain * left));
            }
        }
    }

    fn drop_ended(&mut self, commands: &mut Commands, now: f64) {
        self.voices.retain(|voice| {
            let faded = voice
                .stopped
                .is_some_and(|(since, fade)| now - since >= f64::from(fade));
            let over = faded || now >= voice.ends;
            if over && (voice.despawn || voice.stopped.is_some()) {
                commands.entity(voice.entity).try_despawn();
            }
            !over
        });
    }
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
                mixer.tick(&mut commands, time.elapsed_secs_f64(), &mut sinks);
            },
        );
}
