//! What one's hero says, as Dota's hero speaks to its own player: the hero of the seat played
//! (in a replay the seat watched as, for a spectator the seat whose eyes are watched through)
//! answers the client's queries from its own response rules ([`dbota_import::voice`]). Its
//! voice pack, and only its, is read off the frame's thread (a task on the desktop, a Web Worker
//! in a browser) as soon as the hero is known (its pick, or the match's loading when it is
//! known then), and its compressed sounds stay resident: a line starts as any sound does,
//! decoded as it plays.
//!
//! Only the hero's own player hears it (and sees its portrait speak). The lines Dota makes
//! audible to others are not said here (see the report of the voice change).
//!
//! A pack that is missing or does not read leaves the hero silent and says why once, on the
//! log: a cache of this build has one for every hero, so a missing one is a cache made by
//! another build of the same import revision.

mod listen;
mod load;

#[cfg(test)]
pub use listen::game_time;

#[cfg(test)]
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use bevy::prelude::*;
use bota_proto::{EntityId, SlotId};
use dbota_import::voice::concept::{Concept, Fact, Query};
use dbota_import::voice::rules::HeroRules;
use dbota_import::voice::speaker::{Choice, Silence, Speaker};

use crate::manifest::Manifest;
use crate::mixer::{Mixer, Request};
use crate::session::Session;
use crate::talk::{LineFace, Speech};

pub use load::loaded;

/// Most answers [`Said`] keeps, the newest.
#[cfg(test)]
const MAX_SAID: usize = 256;

/// A hero's pack, resident: its rules, what it remembers having said and its sounds.
pub struct Resident {
    pub hero: String,
    pub rules: HeroRules,
    speaker: Speaker,
    pub sounds: Vec<Handle<AudioSource>>,
    faces: Vec<Option<Arc<LineFace>>>,
    /// The match clock the speaker last answered at: a replay sought back starts it afresh.
    last: f64,
    /// The line chosen and waiting for its time to start.
    waiting: Option<(f64, Choice)>,
}

/// Where the voice of the hero spoken for stands.
#[derive(Default)]
pub enum Pack {
    /// No hero to speak for.
    #[default]
    None,
    /// The hero's pack is being read.
    Loading {
        hero: String,
        job: load::Job,
    },
    Resident(Box<Resident>),
    /// The hero stays silent (the log says why).
    Failed {
        hero: String,
    },
}

/// The voice of the hero the client speaks for.
#[derive(Resource, Default)]
pub struct HeroVoice {
    pub pack: Pack,
}

impl Resident {
    /// The match clock (seconds) up to which the hero is still saying its last line.
    #[cfg(test)]
    pub fn speaking_until(&self) -> f64 {
        self.speaker.speaking_until()
    }
}

impl HeroVoice {
    /// The hero whose pack is resident, loading or failed.
    pub fn hero(&self) -> Option<&str> {
        match &self.pack {
            Pack::None => None,
            Pack::Loading { hero, .. } | Pack::Failed { hero, .. } => Some(hero),
            Pack::Resident(resident) => Some(&resident.hero),
        }
    }

    pub fn resident(&self) -> Option<&Resident> {
        match &self.pack {
            Pack::Resident(resident) => Some(resident),
            _ => None,
        }
    }
}

/// One answer of the hero: what was asked, when, and what it said or why it said nothing.
#[cfg(test)]
#[derive(Clone, Debug)]
pub struct Answer {
    /// The view's tick it was asked at.
    pub tick: u32,
    pub query: Query,
    pub said: Result<Line, Silence>,
}

/// A line said.
#[cfg(test)]
#[derive(Clone, Debug)]
pub struct Line {
    /// The rule that answered, its criteria as `name key expr` and its score.
    pub rule: String,
    pub criteria: Vec<(String, String, String)>,
    pub score: f32,
    /// The voice event and the pack's sound it played.
    pub event: String,
    pub sound: usize,
    /// The sound player, once the line started and if the mixer gave it a voice.
    pub player: Option<Entity>,
}

/// The newest answers of the hero, for the tests.
#[cfg(test)]
#[derive(Resource, Default)]
pub struct Said(pub VecDeque<Answer>);

pub fn plugin(app: &mut App) {
    #[cfg(test)]
    app.init_resource::<Said>();
    app.init_resource::<HeroVoice>()
        .add_systems(
            Update,
            (follow_seat, load::receive, speak.after(load::receive)).chain(),
        )
        .add_systems(PostUpdate, report.in_set(crate::loading::Report));
}

/// The seat whose hero speaks: the one played, or watched as or through.
fn seat(core: &bota_client_core::App) -> Option<SlotId> {
    core.my_slot.or(core.eyes)
}

/// The Dota name of the hero of the seat spoken for: the match's, else the one the seat picked
/// in the lobby (the pack is read from the pick on, before the battle).
fn seat_hero(core: &bota_client_core::App, manifest: &Manifest) -> Option<String> {
    let seat = seat(core)?;
    let hero = match core.view() {
        Some(view) => view.player(seat)?.hero,
        None => core.lobby.slots.iter().find(|s| s.slot == seat)?.hero?,
    };
    manifest.dota_name("hero", hero.0).map(str::to_owned)
}

/// Reads the pack of the hero spoken for when it changes, giving up the one before.
fn follow_seat(
    session: NonSend<Session>,
    manifest: Res<Manifest>,
    mut voice: ResMut<HeroVoice>,
    mut mixer: ResMut<Mixer>,
) {
    let wanted = seat_hero(&session.0, &manifest);
    if wanted.as_deref() == voice.hero() {
        return;
    }
    if voice.resident().is_some() {
        mixer.set_voice_events("");
    }
    voice.pack = match wanted {
        Some(hero) => {
            info!("voice: reading {hero}'s pack");
            let job = load::start(&manifest, &hero);
            Pack::Loading { hero, job }
        }
        None => Pack::None,
    };
}

/// The loading screen waits for the pack of the hero spoken for, read or failed, when the hero
/// is known as the match loads (a replay, a match joined with its hero); a match that loads
/// before its pick reads it from the pick on, during the hero selection.
fn report(voice: Res<HeroVoice>, mut parts: ResMut<crate::loading::Parts>) {
    let loading = matches!(voice.pack, Pack::Loading { .. });
    parts.set("hero voice", usize::from(!loading), 1);
}

/// Asks the hero what the match told it this frame, and says what it answers. Nothing is
/// asked before the game shows, and the match's loading waits for the pack.
#[allow(clippy::too_many_arguments)]
fn speak(
    mut commands: Commands,
    session: NonSend<Session>,
    mut voice: ResMut<HeroVoice>,
    #[cfg(test)] mut said: ResMut<Said>,
    mut mixer: ResMut<Mixer>,
    mut speech: ResMut<Speech>,
    mut memory: Local<listen::Memory>,
    settings: Res<crate::config::Settings>,
    loading: Res<crate::loading::Loading>,
    fog: Res<crate::fog::FogGrid>,
    time: Res<Time>,
    real: Res<Time<bevy::time::Real>>,
) {
    let core = &session.0;
    let (Some(view), Pack::Resident(resident), true) =
        (core.view(), &mut voice.pack, loading.done())
    else {
        *memory = listen::Memory::default();
        return;
    };
    let queries = memory.queries(core, seat(core), &fog);
    let rate = f64::from(core.tick_rate().max(1));
    let now = f64::from(view.tick) / rate;
    if now < resident.last {
        resident.speaker = Speaker::new(&resident.rules, load::seed());
        resident.waiting = None;
    }
    resident.last = now;
    let hero = seat(core).and_then(|s| view.player(s)).and_then(|p| p.unit);
    if settings.voice {
        for (concept, facts) in queries {
            let random = resident.speaker.draw(100) as u32;
            let query = facts.into_iter().fold(
                Query::new(
                    concept,
                    &resident.rules.class,
                    random,
                    listen::game_time(core),
                ),
                |query, (fact, value)| query.with(fact, value),
            );
            let answer = resident.speaker.respond(&resident.rules, &query, now);
            log(resident, view.tick, &query, &answer);
            if let Ok(choice) = &answer {
                resident.waiting = Some((now + f64::from(choice.delay), choice.clone()));
            }
            #[cfg(test)]
            keep(&mut said, resident, view.tick, query, &answer);
        }
    }
    // A line waits out its `predelay` on the match's clock; the speaker says nothing else
    // meanwhile.
    if let Some((_, choice)) = resident.waiting.take_if(|(at, _)| now >= *at) {
        let player = play(&mut commands, &mut mixer, time.elapsed(), resident, &choice);
        if let (Some(face), Some(_)) = (resident.faces[choice.sound].clone(), player) {
            speech.say(hero, face, player, real.elapsed_secs());
        }
        #[cfg(test)]
        started(&mut said, &choice, player);
    }
}

/// Tells the tests' record of the line that it started, and on which voice.
#[cfg(test)]
fn started(said: &mut Said, choice: &Choice, player: Option<Entity>) {
    let waited = said
        .0
        .iter_mut()
        .rev()
        .find_map(|answer| match &mut answer.said {
            Ok(line) if line.player.is_none() && line.sound == choice.sound => Some(line),
            _ => None,
        });
    if let Some(line) = waited {
        line.player = player;
    }
}

/// Starts the line's sound through the mixer, heard wherever the camera is; the voice playing
/// it, if the mixer gave it one.
fn play(
    commands: &mut Commands,
    mixer: &mut Mixer,
    now: Duration,
    resident: &Resident,
    choice: &Choice,
) -> Option<Entity> {
    let rules = &resident.rules;
    let line = &rules.rules[choice.rule].lines[choice.line];
    let event = &rules.events[line.event].name;
    let request = Request {
        event,
        on_player: true,
        ..Request::default()
    };
    let source = resident.sounds[choice.sound].clone();
    mixer.start_heard(commands, now, &request, source)
}

/// Tells the log what the hero answered.
fn log(resident: &Resident, tick: u32, query: &Query, answer: &Result<Choice, Silence>) {
    let concept = query.concept.dota_name();
    match answer {
        Ok(choice) => {
            let rules = &resident.rules;
            let rule = &rules.rules[choice.rule];
            let event = &rules.events[rule.lines[choice.line].event].name;
            debug!(
                "voice: {concept} at tick {tick}: {} says {event} (score {}, {} criteria)",
                rule.name,
                choice.score,
                choice.matched.len()
            );
        }
        Err(why) => debug!("voice: {concept} at tick {tick}: silent ({why:?})"),
    }
}

/// Keeps the answer for the tests, the newest [`MAX_SAID`].
#[cfg(test)]
fn keep(
    said: &mut Said,
    resident: &Resident,
    tick: u32,
    query: Query,
    answer: &Result<Choice, Silence>,
) {
    let rules = &resident.rules;
    let line = answer.clone().map(|choice| {
        let rule = &rules.rules[choice.rule];
        Line {
            rule: rule.name.clone(),
            criteria: choice
                .matched
                .iter()
                .map(|&c| {
                    let c = &rules.criteria[c];
                    (c.name.clone(), c.key.clone(), c.expr.clone())
                })
                .collect(),
            score: choice.score,
            event: rules.events[rule.lines[choice.line].event].name.clone(),
            sound: choice.sound,
            player: None,
        }
    });
    if said.0.len() == MAX_SAID {
        said.0.pop_front();
    }
    said.0.push_back(Answer {
        tick,
        query,
        said: line,
    });
}

/// The hero of `unit`, for the facts that name one.
fn hero_class(core: &bota_client_core::App, unit: EntityId) -> Option<&'static str> {
    let player = core.view()?.players.iter().find(|p| p.unit == Some(unit))?;
    Some(bota_core::hero_def(player.hero)?.key)
}

/// The facts [`listen`] finds for a concept, besides those every query carries.
type Facts = Vec<(Fact, String)>;
/// A concept raised, and its facts.
type Raised = (Concept, Facts);
