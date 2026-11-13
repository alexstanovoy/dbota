//! The channels the player balances in the settings' mixer, and what the client does to let
//! them hear one: every sound the client plays belongs to exactly one channel, whose level
//! (under the master volume) and mute the settings hold, and a sound played to try a level.
//!
//! The channels are the ones the sound code routes: the world's effects ([`crate::sound`],
//! [`crate::mixer`]), the hero's own voice ([`crate::hero_voice`]), the announcer and the
//! day and night calls ([`crate::voice`]), the map's ambience ([`crate::soundscape`]) and the
//! interface's clicks. The game has no music, so the mixer has none.

use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackSettings, Volume};
use bevy::prelude::*;

use crate::config::Settings;
use crate::manifest::{Manifest, cache_path};

/// A kind of sound, with a level of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel {
    /// Attacks, abilities, items, buildings and creeps: everything the world makes.
    Effects,
    /// What the player's hero says.
    Voices,
    /// The announcer, the ancients' horns and the calls of day and night.
    Announcer,
    /// The map's soundscapes.
    Ambience,
    /// The clicks of the menu and the interface.
    Interface,
}

impl Channel {
    pub const COUNT: usize = 5;
    /// In the order the mixer lays them out.
    pub const ALL: [Channel; Channel::COUNT] = [
        Channel::Effects,
        Channel::Voices,
        Channel::Announcer,
        Channel::Ambience,
        Channel::Interface,
    ];

    fn index(self) -> usize {
        Channel::ALL
            .iter()
            .position(|channel| *channel == self)
            .expect("every channel is listed")
    }

    /// The word the settings file builds its names from: `volume_effects`, `mute_effects`.
    pub fn key(self) -> &'static str {
        match self {
            Channel::Effects => "effects",
            Channel::Voices => "voices",
            Channel::Announcer => "announcer",
            Channel::Ambience => "ambience",
            Channel::Interface => "interface",
        }
    }

    /// What the mixer calls it.
    pub fn label(self) -> &'static str {
        match self {
            Channel::Effects => "Effects",
            Channel::Voices => "Hero voices",
            Channel::Announcer => "Announcer",
            Channel::Ambience => "Ambience",
            Channel::Interface => "Interface",
        }
    }

    /// The channel a settings key (`volume_effects`, `mute_effects`) names, and whether it is
    /// the level's or the mute's.
    pub fn of_key(name: &str) -> Option<(Channel, bool)> {
        let (level, key) = match (name.strip_prefix("volume_"), name.strip_prefix("mute_")) {
            (Some(key), _) => (true, key),
            (_, Some(key)) => (false, key),
            _ => return None,
        };
        Channel::ALL
            .into_iter()
            .find(|channel| channel.key() == key)
            .map(|channel| (channel, level))
    }
}

/// The level and the mute of every channel, 0 to 1 and on or off; full and on by default, so
/// the master volume alone sets how loud the game is until the player balances it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mix {
    level: [f32; Channel::COUNT],
    muted: [bool; Channel::COUNT],
}

impl Default for Mix {
    fn default() -> Mix {
        Mix {
            level: [1.0; Channel::COUNT],
            muted: [false; Channel::COUNT],
        }
    }
}

impl Mix {
    pub fn level(&self, channel: Channel) -> f32 {
        self.level[channel.index()]
    }

    pub fn set_level(&mut self, channel: Channel, level: f32) {
        assert!((0.0..=1.0).contains(&level), "a level is 0 to 1: {level}");
        self.level[channel.index()] = level;
    }

    pub fn muted(&self, channel: Channel) -> bool {
        self.muted[channel.index()]
    }

    pub fn set_muted(&mut self, channel: Channel, muted: bool) {
        self.muted[channel.index()] = muted;
    }

    /// What the channel's sounds are multiplied by: its level, nothing while it is muted.
    pub fn gain(&self, channel: Channel) -> f32 {
        if self.muted(channel) {
            0.0
        } else {
            self.level(channel)
        }
    }
}

/// What a sound is, apart from the player's balance: the channel it sounds in and its own
/// loudness (its event's volume, its distance, its fade). Every sound the client plays carries
/// one, and [`follow_levels`] is the one place that turns it into what the sink plays: the
/// master volume and the channel's level and mute, applied to sounds already playing as to
/// those that start.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Tuned {
    pub channel: Channel,
    pub base: f32,
}

/// How loud the channel's sounds are of their own loudness: the master volume (nothing while
/// all sound is muted, by the player or by the run's `--mute`) times the channel's level (nothing
/// while it is muted).
pub fn live_gain(settings: &Settings, muted_by_run: bool, channel: Channel) -> f32 {
    if settings.muted || muted_by_run {
        0.0
    } else {
        settings.volume * settings.mix.gain(channel)
    }
}

/// Sets every sink to its sound's loudness times the balance now, so that a mute, a fader or the
/// master moved while a sound plays reaches it at once.
pub fn follow_levels(
    settings: Res<Settings>,
    args: Res<crate::Args>,
    mut sinks: Query<(&Tuned, &mut AudioSink)>,
) {
    for (tuned, mut sink) in &mut sinks {
        let wanted = tuned.base * live_gain(&settings, args.mute, tuned.channel);
        if (sink.volume().to_linear() - wanted).abs() > 1e-6 {
            sink.set_volume(Volume::Linear(wanted));
        }
    }
}

/// A level being tried: the sound of its channel plays once, so that the player hears where the
/// level stands.
#[derive(Message, Clone, Copy, Debug)]
pub struct Preview(pub Channel);

/// The sound that plays a level: one at a time, the next replacing the one before.
#[derive(Component)]
struct PreviewSound;

/// What the previews played, for the tests: the channel and the loudness asked of the sink before
/// the master volume.
#[cfg(test)]
#[derive(Resource, Default, Debug)]
pub struct Heard(pub Vec<(Channel, f32)>);

/// How loud a try sounds before the channel's level: the loudness of the interface's click.
const PREVIEW_GAIN: f32 = 0.5;
/// Previews a run may keep in the test log.
#[cfg(test)]
const MAX_HEARD: usize = 256;

pub fn plugin(app: &mut App) {
    app.add_message::<Preview>().add_systems(Update, preview);
    #[cfg(test)]
    app.init_resource::<Heard>();
}

/// The logical name of the sound that stands for a channel, where one file does.
fn sample_name(channel: Channel) -> Option<&'static str> {
    match channel {
        Channel::Effects => Some("sound.set.creep_good_melee.attack"),
        Channel::Announcer => Some("sound.announcer.battle_begins"),
        Channel::Interface => Some("sound.ui.click"),
        Channel::Voices | Channel::Ambience => None,
    }
}

/// Plays the sound of the channel last asked for this frame, in place of the one before.
fn preview(
    mut asked: MessageReader<Preview>,
    mut commands: Commands,
    settings: Res<Settings>,
    args: Res<crate::Args>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    ambience: Res<crate::soundscape::Ambience>,
    mut voices: ResMut<crate::hero_voice::HeroVoice>,
    old: Query<Entity, With<PreviewSound>>,
    mut waiting: Local<bool>,
    #[cfg(test)] mut heard: ResMut<Heard>,
) {
    let mut channel = asked.read().last().map(|Preview(channel)| *channel);
    // The hero's voice is read when first asked for, outside a match: it sounds once it is here.
    if channel.is_none() && *waiting && voices.resident().is_some() {
        channel = Some(Channel::Voices);
    }
    let Some(channel) = channel else {
        return;
    };
    *waiting = channel == Channel::Voices && voices.resident().is_none();
    if *waiting {
        voices.preview = true;
        return;
    }
    for sound in &old {
        commands.entity(sound).despawn();
    }
    let gain = PREVIEW_GAIN * settings.mix.gain(channel);
    if settings.muted || args.mute || gain <= 0.0 {
        return;
    }
    let source = match sample_name(channel) {
        Some(name) => manifest
            .sound_files(name)
            .next()
            .map(|file| server.load(cache_path(file))),
        None if channel == Channel::Ambience => ambience
            .sounds
            .iter()
            .find(|(_, _, looped)| *looped)
            .or(ambience.sounds.first())
            .map(|(_, wave, _)| server.load(cache_path(wave))),
        None => voices
            .resident()
            .and_then(|resident| resident.sounds.first().cloned()),
    };
    let Some(source) = source else {
        warn!("the mixer has no sound to try the {channel:?} channel with");
        return;
    };
    commands.spawn((
        PreviewSound,
        Tuned {
            channel,
            base: PREVIEW_GAIN,
        },
        AudioPlayer::new(source),
        PlaybackSettings::DESPAWN.with_volume(Volume::Linear(PREVIEW_GAIN)),
    ));
    #[cfg(test)]
    {
        heard.0.push((channel, gain));
        let over = heard.0.len().saturating_sub(MAX_HEARD);
        heard.0.drain(..over);
    }
}
