//! Sounds: Dota's own from the cache, each cue of the client core matched to
//! one, synthesized stand-ins for what the cache lacks; quieter the farther
//! from the camera, under the player's volume.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::{audio::Volume, prelude::*};
use bota_client_core::{Cue, CueKind};
use bota_proto::{SlotId, UnitKind};

use crate::camera::RtsCamera;
use crate::config::Settings;
use crate::manifest::Manifest;
use crate::session::Session;
use crate::synth::{RATE, Sfx, synth};

/// World units from the camera within which a sound is heard at full.
const EARSHOT: f32 = 1200.0;
/// World units over which a sound fades out past [`EARSHOT`].
const FADE: f32 = 3000.0;
/// Ticks before its end within which a vanished teleport counts as landed
/// rather than broken off, since a view may skip a tick.
const LANDING_SLACK: u32 = 3;
/// Seconds a kind of sound rests before it may sound again.
const REST: f32 = 0.05;
/// New missiles a tick looked into at most: each is matched against every
/// unit, and a snapshot may hold thousands of them.
const SHOTS_PER_TICK: usize = 32;

/// One kind of sound and the sources it may be played from.
struct Voice {
    sources: Vec<Handle<AudioSource>>,
    rest: f32,
    next: usize,
}

/// Every kind of sound the client plays.
#[derive(Resource, Default)]
struct Voices(Vec<(Kind, Voice)>);

/// A kind of sound.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Cue(CueKind),
    CreepShot,
    TowerShot,
    /// The hum over a teleport's landing.
    PortalArriving,
    /// The hum where a hero is leaving.
    PortalLeaving,
    /// A hero landing from a teleport.
    HeroArrived,
    /// A hero gone by a teleport.
    HeroLeft,
}

/// The loops of one teleport under way, and what its landing needs.
struct Humming {
    loops: Vec<Entity>,
    landing: (f32, f32),
    origin: Option<(f32, f32)>,
    ticks_left: u32,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Voices>()
        .add_systems(Startup, load)
        .add_systems(Update, (volume, play, portals));
}

/// The cached sounds a cue plays, by logical name prefix, and its stand-in.
fn sources_of(kind: Kind) -> (&'static [&'static str], Option<Sfx>) {
    match kind {
        Kind::Cue(cue) => match cue {
            CueKind::Attack => (&["sound.nevermore.attack."], Some(Sfx::Attack)),
            CueKind::Hit => (&["sound.nevermore.attack_impact"], Some(Sfx::Hit)),
            CueKind::LastHit => (&["sound.last_hit"], Some(Sfx::LastHit)),
            CueKind::Deny => (&["sound.deny"], Some(Sfx::Deny)),
            CueKind::Raze => (&["sound.raze"], Some(Sfx::Raze)),
            CueKind::Requiem => (&["sound.requiem"], Some(Sfx::Requiem)),
            CueKind::LowHp => (&[], Some(Sfx::LowHp)),
            CueKind::Buy => (&["sound.buy"], Some(Sfx::Buy)),
            CueKind::LevelUp => (&["sound.level_up"], Some(Sfx::LevelUp)),
            CueKind::TowerShot => (&["sound.tower.hero_impact"], Some(Sfx::TowerShot)),
            CueKind::HeroDeath => (&["sound.nevermore.death"], Some(Sfx::HeroDeath)),
            CueKind::HeroKill => (&[], Some(Sfx::HeroKill)),
            CueKind::Refused => (&["sound.no_gold"], Some(Sfx::Refused)),
        },
        Kind::CreepShot => (
            &[
                "sound.creep.radiant.ranged.attack.",
                "sound.creep.dire.ranged.attack.",
            ],
            None,
        ),
        Kind::TowerShot => (
            &["sound.tower.radiant.attack", "sound.tower.dire.attack"],
            None,
        ),
        Kind::PortalArriving => (&["sound.portal.loop_appear"], None),
        Kind::PortalLeaving => (&["sound.portal.loop_disappear"], None),
        Kind::HeroArrived => (&["sound.portal.hero_appear"], None),
        Kind::HeroLeft => (&["sound.portal.hero_disappear"], None),
    }
}

fn load(
    mut voices: ResMut<Voices>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    mut audio: ResMut<Assets<AudioSource>>,
) {
    let kinds = [
        CueKind::Attack,
        CueKind::Hit,
        CueKind::LastHit,
        CueKind::Deny,
        CueKind::Raze,
        CueKind::Requiem,
        CueKind::LowHp,
        CueKind::Buy,
        CueKind::LevelUp,
        CueKind::TowerShot,
        CueKind::HeroDeath,
        CueKind::HeroKill,
        CueKind::Refused,
    ]
    .map(Kind::Cue)
    .into_iter()
    .chain([
        Kind::CreepShot,
        Kind::TowerShot,
        Kind::PortalArriving,
        Kind::PortalLeaving,
        Kind::HeroArrived,
        Kind::HeroLeft,
    ]);
    for kind in kinds {
        let (names, stand_in) = sources_of(kind);
        let mut sources: Vec<Handle<AudioSource>> = names
            .iter()
            .flat_map(|prefix| manifest.sounds(prefix))
            .map(|path| server.load(path))
            .collect();
        if sources.is_empty()
            && let Some(sfx) = stand_in
        {
            sources.push(audio.add(AudioSource {
                bytes: Arc::from(wav(&synth(sfx))),
            }));
        }
        voices.0.push((
            kind,
            Voice {
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
) {
    for (_, voice) in &mut voices.0 {
        voice.rest = (voice.rest - time.delta_secs()).max(0.0);
    }
    let core = &mut session.0;
    let cues: Vec<Cue> = std::mem::take(&mut core.cues);
    let silent = settings.muted || args.mute;
    let focus = Vec2::new(cam.focus.x, -cam.focus.z);
    let mut sound = |kind: Kind, gain: f32, at: Option<(f32, f32)>| {
        let fall = at.map_or(1.0, |at| fall_off(at, focus));
        let Some((_, voice)) = voices.0.iter_mut().find(|(k, _)| *k == kind) else {
            return;
        };
        if silent || voice.rest > 0.0 || voice.sources.is_empty() || fall * gain < 0.03 {
            return;
        }
        voice.rest = REST;
        let source = voice.sources[voice.next % voice.sources.len()].clone();
        voice.next += 1;
        commands.spawn((
            AudioPlayer::new(source),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(gain * fall)),
        ));
    };
    for cue in cues {
        sound(Kind::Cue(cue.kind), cue.gain, cue.at);
    }
    shots(core, &mut sound);
}

/// How much of its volume a sound at a place keeps for a listener at `focus`.
fn fall_off((x, y): (f32, f32), focus: Vec2) -> f32 {
    let far = Vec2::new(x, y).distance(focus);
    (1.0 - (far - EARSHOT).max(0.0) / FADE).clamp(0.0, 1.0)
}

/// Hums over every teleport under way, at the landing and, when the hero is
/// in sight, where it stands; a teleport that runs out lands with a sound at
/// both ends, one that vanishes sooner was broken off and only goes quiet.
fn portals(
    mut commands: Commands,
    session: NonSend<Session>,
    voices: Res<Voices>,
    cam: Single<&RtsCamera>,
    settings: Res<Settings>,
    args: Res<crate::Args>,
    mut humming: Local<HashMap<SlotId, Humming>>,
) {
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
    let ended: Vec<SlotId> = humming
        .keys()
        .filter(|slot| !view.teleports.iter().any(|t| t.slot == **slot))
        .copied()
        .collect();
    for slot in ended {
        let Some(hum) = humming.remove(&slot) else {
            continue;
        };
        for entity in hum.loops {
            commands.entity(entity).despawn();
        }
        if hum.ticks_left <= LANDING_SLACK {
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
                ticks_left: teleport.ticks_left,
            }
        });
        hum.ticks_left = teleport.ticks_left;
        hum.origin = crate::teleport::origin(view, teleport).or(hum.origin);
    }
}

/// The shots of creeps and towers that let a missile go this tick.
fn shots(core: &bota_client_core::App, sound: &mut impl FnMut(Kind, f32, Option<(f32, f32)>)) {
    let (Some(view), Some(prev)) = (core.view(), core.prev_view()) else {
        return;
    };
    if view.tick == prev.tick || core.smooth.since > 0.0 {
        return;
    }
    let new = view.projectiles.iter().filter(|p| {
        p.ability.is_none()
            && prev
                .projectiles
                .binary_search_by_key(&p.id, |q| q.id)
                .is_err()
    });
    for p in new.take(SHOTS_PER_TICK) {
        let (px, py) = (p.pos.x.to_f32(), p.pos.y.to_f32());
        let from = view
            .units
            .iter()
            .filter(|u| u.team == p.team)
            .map(|u| (u, (u.pos.x.to_f32() - px).hypot(u.pos.y.to_f32() - py)))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match from {
            Some((u, d)) if u.kind == UnitKind::Tower && d < 300.0 => {
                sound(Kind::TowerShot, 0.7, Some((px, py)))
            }
            Some((u, d)) if u.kind == UnitKind::CreepRanged && d < 150.0 => {
                sound(Kind::CreepShot, 0.35, Some((px, py)))
            }
            _ => {}
        }
    }
}

/// A mono 16-bit PCM WAV file of samples at [`RATE`].
fn wav(samples: &[f32]) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    out
}
