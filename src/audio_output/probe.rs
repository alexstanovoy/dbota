//! A deterministic voice used by the native sink check.
use std::io::{Cursor, Read};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::*;
use bevy::asset::AssetPlugin;

pub(crate) fn run() -> Result<(), Box<dyn std::error::Error>> {
    use clap::Parser as _;
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()));
    app.init_resource::<crate::config::Settings>()
        .insert_resource(crate::Args::parse_from(["dbota"]));
    app.add_plugins(super::plugin);
    app.update();
    if !matches!(
        app.world().resource::<Output>().0.stream,
        Some(Stream::Device(_))
    ) {
        return Err("the probe needs a real audio output".into());
    }
    println!("audio-probe-ready");
    let mut start = [0];
    std::io::stdin().read_exact(&mut start)?;
    if start != *b"1" {
        return Err("the probe start byte must be 1".into());
    }

    let mut bytes = Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wave = hound::WavWriter::new(&mut bytes, spec)?;
    let mut word = 0xc35a_29efu32;
    for _ in 0..spec.sample_rate * 10 {
        word ^= word << 13;
        word ^= word >> 17;
        word ^= word << 5;
        wave.write_sample(if word & 1 == 0 { -8192i16 } else { 8192i16 })?;
    }
    wave.finalize()?;

    let world = app.world_mut();
    let source = world
        .resource_mut::<Assets<AudioSource>>()
        .add(AudioSource {
            bytes: bytes.into_inner().into(),
        });
    let voice = VoiceClock::new(1.0);
    let player = world
        .spawn((
            AudioPlayer::new(source),
            PlaybackSettings::DESPAWN,
            voice.clone(),
        ))
        .id();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut reported = false;
    while Instant::now() < deadline {
        app.update();
        let world = app.world();
        let time = world.resource::<Time<Real>>().elapsed();
        let output = world.resource::<Output>();
        let presentation = output.presentation(time).ok_or("the device disappeared")?;
        match presentation(&voice, world.get::<AudioPlayer>(player).is_some()) {
            Played::At(position) if !reported => {
                let wall = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs_f64();
                println!("audio-probe-presented {wall:.9} {position:.9}");
                reported = true;
            }
            Played::Ended if reported => return Ok(()),
            Played::Gone => return Err("the voice disappeared".into()),
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Err("the probe voice did not finish".into())
}
