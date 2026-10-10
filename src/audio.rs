//! Audio: a hit sound per simulated attack impact, from the cache (`cache://...wav/ogg`)
//! or, if none was exported yet, a WAV synthesized in memory. `[`/`]` change volume.

use std::sync::Arc;

use bevy::{audio::Volume, prelude::*};

use crate::{Args, fallback::FoundAssets, hero::AttackImpact};

const SAMPLE_RATE: u32 = 22_050;
const HIT_SECONDS: f32 = 0.09;

#[derive(Resource)]
struct HitSound(Handle<AudioSource>);

pub fn plugin(app: &mut App) {
    // GlobalVolume is applied when a sink is created; already-playing sounds keep theirs
    // (adjust those through their `AudioSink`).
    app.insert_resource(GlobalVolume::new(Volume::Linear(0.3)))
        .add_systems(Startup, load_sound)
        .add_systems(Update, (play_hits, volume_keys, log_first_sink));
}

fn load_sound(
    mut commands: Commands,
    server: Res<AssetServer>,
    found: Res<FoundAssets>,
    mut sources: ResMut<Assets<AudioSource>>,
) {
    let handle = match &found.sound {
        Some(path) => server.load(path.clone()),
        // AudioSource is just encoded bytes; rodio sniffs the format when decoding.
        None => sources.add(AudioSource {
            bytes: Arc::from(hit_wav()),
        }),
    };
    commands.insert_resource(HitSound(handle));
}

fn play_hits(
    mut commands: Commands,
    args: Res<Args>,
    sound: Res<HitSound>,
    mut impacts: MessageReader<AttackImpact>,
) {
    for _ in impacts.read() {
        if args.mute {
            continue;
        }
        // DESPAWN: the entity is removed once playback ends (no leaked sink entities).
        commands.spawn((
            AudioPlayer::new(sound.0.clone()),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(0.8)),
        ));
    }
}

fn volume_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut global: ResMut<GlobalVolume>,
    mut sinks: Query<&mut AudioSink>,
) {
    let pct = if keys.just_pressed(KeyCode::BracketRight) {
        25.0
    } else if keys.just_pressed(KeyCode::BracketLeft) {
        -20.0
    } else {
        return;
    };
    global.volume = global.volume.increase_by_percentage(pct);
    for mut sink in &mut sinks {
        let v = sink.volume().increase_by_percentage(pct);
        sink.set_volume(v);
    }
    info!("global volume: {:.2}", global.volume.to_linear());
}

/// 16-bit mono PCM WAV: a decaying 180 Hz thump with a short noise transient.
fn hit_wav() -> Vec<u8> {
    let n = (SAMPLE_RATE as f32 * HIT_SECONDS) as u32;
    let mut pcm = Vec::with_capacity(n as usize * 2);
    let mut noise = 0x1234_5678u32;
    for i in 0..n {
        let t = i as f32 / SAMPLE_RATE as f32;
        noise ^= noise << 13;
        noise ^= noise >> 17;
        noise ^= noise << 5;
        let white = noise as f32 / u32::MAX as f32 * 2.0 - 1.0;
        let tone = (t * 180.0 * std::f32::consts::TAU).sin() * (-t * 40.0).exp();
        let click = white * (-t * 400.0).exp();
        let s = (0.7 * tone + 0.3 * click).clamp(-1.0, 1.0);
        pcm.extend(((s * i16::MAX as f32) as i16).to_le_bytes());
    }
    let mut wav = Vec::with_capacity(44 + pcm.len());
    let data_len = u32::try_from(pcm.len()).expect("short clip");
    wav.extend(b"RIFF");
    wav.extend((36 + data_len).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes()); // fmt chunk size
    wav.extend(1u16.to_le_bytes()); // PCM
    wav.extend(1u16.to_le_bytes()); // mono
    wav.extend(SAMPLE_RATE.to_le_bytes());
    wav.extend((SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    wav.extend(2u16.to_le_bytes()); // block align
    wav.extend(16u16.to_le_bytes()); // bits per sample
    wav.extend(b"data");
    wav.extend(data_len.to_le_bytes());
    wav.extend(pcm);
    wav
}

/// A sink exists only if the output device opened and the source decoded.
fn log_first_sink(q: Query<&AudioSink, Added<AudioSink>>, mut done: Local<bool>) {
    if !*done && let Some(sink) = q.iter().next() {
        *done = true;
        info!(
            "audio: first sink playing, volume {:.2}",
            sink.volume().to_linear()
        );
    }
}
