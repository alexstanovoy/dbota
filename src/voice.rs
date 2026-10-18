//! The voices: one's hero answering its move and attack orders as in Dota
//! (now and then, not on every click), the announcer at hero selection and
//! when the battle begins with the horn, the rooster at daybreak and the
//! wolf at nightfall, and the click of the interface.
//! Silent where the cache has no such sound.

use bevy::{audio::Volume, prelude::*};
use bota_client_core::Phase;
use bota_proto::Order;

use crate::config::Settings;
use crate::manifest::Manifest;
use crate::session::Session;

/// Tenths of a second the start horn honks before it fades (Dota's
/// horn_radiant.wav is loud for its first 6.5 s).
const HORN_HONK_TENTHS: u32 = 65;
/// Tenths of a second "the battle begins" lasts (announcer_battle_begin_01,
/// 1.65 s decoded).
const BATTLE_BEGINS_TENTHS: u32 = 17;
/// Seconds a hero keeps quiet after answering an order.
const VOICE_REST: f32 = 6.0;

/// The sounds of one logical name, played in turn.
#[derive(Default)]
struct Lines {
    sources: Vec<Handle<AudioSource>>,
    next: usize,
}

impl Lines {
    fn load(manifest: &Manifest, server: &AssetServer, name: &str) -> Lines {
        Lines {
            sources: manifest
                .sounds(name)
                .into_iter()
                .map(|p| server.load(p))
                .collect(),
            next: 0,
        }
    }

    /// The next of the sounds, if there are any.
    fn take(&mut self) -> Option<Handle<AudioSource>> {
        let source = self
            .sources
            .get(self.next % self.sources.len().max(1))?
            .clone();
        self.next += 1;
        Some(source)
    }
}

#[derive(Resource, Default)]
struct Voices {
    /// By hero Dota name: its move and attack answers.
    heroes: Vec<(String, Lines, Lines)>,
    click: Lines,
    choose: Lines,
    begins: Lines,
    /// The horn at each side's ancient: Radiant's, then Dire's.
    horns: [Lines; 2],
    /// Daybreak's birds and rooster, and nightfall's howl.
    day: Lines,
    night: Lines,
}

/// What the voices remember between frames.
#[derive(Default)]
struct Heard {
    /// The last order sent that was looked at.
    seq: u32,
    /// Seconds before the hero may answer again.
    rest: f32,
    /// Whether the announcer has asked for a hero.
    asked: bool,
    /// Whether the horn has sounded.
    begun: bool,
    /// Whether the battle has been announced.
    announced: bool,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Voices>()
        .add_systems(Startup, load)
        .add_systems(Update, (answer, announce, herald, click));
}

fn load(mut voices: ResMut<Voices>, manifest: Res<Manifest>, server: Res<AssetServer>) {
    for id in 0..bota_core::HEROES.len() as u16 {
        let Some(name) = manifest.dota_name("hero", id) else {
            continue;
        };
        let moves = Lines::load(&manifest, &server, &format!("sound.voice.{name}.move"));
        let attacks = Lines::load(&manifest, &server, &format!("sound.voice.{name}.attack"));
        voices.heroes.push((name.to_string(), moves, attacks));
    }
    voices.click = Lines::load(&manifest, &server, "sound.ui.click");
    voices.choose = Lines::load(&manifest, &server, "sound.announcer.choose_hero");
    voices.begins = Lines::load(&manifest, &server, "sound.announcer.battle_begins");
    voices.day = Lines::load(&manifest, &server, "sound.day");
    voices.night = Lines::load(&manifest, &server, "sound.night");
    for (i, side) in ["radiant", "dire"].into_iter().enumerate() {
        voices.horns[i] = Lines {
            sources: manifest
                .sounds("sound.horn")
                .into_iter()
                .filter(|p| p.contains(side))
                .map(|p| server.load(p))
                .collect(),
            next: 0,
        };
    }
}

fn say(commands: &mut Commands, source: Option<Handle<AudioSource>>, gain: f32) {
    if let Some(source) = source {
        commands.spawn((
            AudioPlayer::new(source),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(gain)),
        ));
    }
}

/// One's hero answers a move or an attack order, then keeps quiet a while.
fn answer(
    mut commands: Commands,
    session: NonSend<Session>,
    manifest: Res<Manifest>,
    mut voices: ResMut<Voices>,
    settings: Res<Settings>,
    args: Res<crate::Args>,
    time: Res<Time>,
    mut heard: Local<Heard>,
) {
    let core = &session.0;
    heard.rest = (heard.rest - time.delta_secs()).max(0.0);
    if core.seq() == heard.seq {
        return;
    }
    heard.seq = core.seq();
    if !settings.voice || settings.muted || args.mute || heard.rest > 0.0 {
        return;
    }
    let (Some(slot), Some(view)) = (core.my_slot, core.view()) else {
        return;
    };
    let Some(order) = core
        .shown_orders
        .iter()
        .find(|s| s.slot == slot && s.unit.is_none_or(|u| Some(u) == core.my_hero()))
        .map(|s| s.order)
    else {
        return;
    };
    let attack = match order {
        Order::Move { .. } => false,
        Order::Attack { .. } => true,
        _ => return,
    };
    let Some(name) = view
        .player(slot)
        .and_then(|p| manifest.dota_name("hero", p.hero.0))
    else {
        return;
    };
    let Some((_, moves, attacks)) = voices.heroes.iter_mut().find(|(n, _, _)| n == name) else {
        return;
    };
    let source = if attack { attacks.take() } else { moves.take() };
    if source.is_some() {
        heard.rest = VOICE_REST;
    }
    say(&mut commands, source, 0.8);
}

/// The announcer asks for a hero once the selection opens, and the horn
/// and the announcer start the battle when the clock reaches zero.
fn announce(
    mut commands: Commands,
    session: NonSend<Session>,
    mut voices: ResMut<Voices>,
    settings: Res<Settings>,
    args: Res<crate::Args>,
    mut heard: Local<Heard>,
) {
    let core = &session.0;
    let silent = settings.muted || args.mute;
    if core.phase == Phase::Lobby && core.my_slot.is_some() && !heard.asked {
        heard.asked = true;
        if !silent {
            let source = voices.choose.take();
            say(&mut commands, source, 0.9);
        }
    }
    // The horn, then the announcer's "the battle begins" once the horn has
    // honked, timed to end as the clock reaches 0:00: each on its own game
    // tick, whatever the frame rate.
    let Some(view) = core.view() else {
        return;
    };
    let rate = u32::from(core.tick_rate().max(1));
    // The whole sequence ends on 0:00: the horn, then the line, then the
    // clock starts.
    let line_tick = core
        .pregame_ticks()
        .saturating_sub(BATTLE_BEGINS_TENTHS * rate / 10);
    let horn_tick = line_tick.saturating_sub(HORN_HONK_TENTHS * rate / 10);
    // Not for a match joined, or a replay opened, well after the start.
    let fresh = |at: u32| view.tick >= at && view.tick < at.saturating_add(2 * rate);
    if fresh(horn_tick) && !heard.begun {
        heard.begun = true;
        if !silent {
            // The horn of one's own side's ancient.
            let dire = core.my_team() == Some(bota_proto::Team::Dire);
            let horn = voices.horns[usize::from(dire)].take();
            say(&mut commands, horn, 0.8);
        }
    }
    if fresh(line_tick) && !heard.announced {
        heard.announced = true;
        if !silent {
            let line = voices.begins.take();
            say(&mut commands, line, 0.9);
        }
    }
}

/// Daybreak and nightfall, as Dota sounds them (`General.Morning`,
/// `General.Night`), when the time of day turns between two views of the
/// match; never for the first view, so joining or opening a replay at night
/// howls at nobody.
fn herald(
    mut commands: Commands,
    session: NonSend<Session>,
    mut voices: ResMut<Voices>,
    settings: Res<Settings>,
    args: Res<crate::Args>,
    mut was_night: Local<Option<bool>>,
) {
    let night = session
        .0
        .view()
        .map(|view| bota_core::time_of_day(view.tick).night);
    let turned = matches!((*was_night, night), (Some(was), Some(now)) if was != now);
    *was_night = night;
    if !turned || settings.muted || args.mute {
        return;
    }
    let source = if night == Some(true) {
        voices.night.take()
    } else {
        voices.day.take()
    };
    say(&mut commands, source, 0.5);
}

/// The click of any button of the interface.
fn click(
    mut commands: Commands,
    mut voices: ResMut<Voices>,
    buttons: Query<&Interaction, (Changed<Interaction>, With<Button>)>,
    settings: Res<Settings>,
    args: Res<crate::Args>,
) {
    if settings.muted || args.mute {
        return;
    }
    if buttons.iter().any(|i| *i == Interaction::Pressed) {
        let source = voices.click.take();
        say(&mut commands, source, 0.5);
    }
}
