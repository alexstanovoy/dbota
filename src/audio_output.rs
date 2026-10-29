//! The sound device, owned here rather than by Bevy's audio plugin.
//!
//! When the device goes (unplugged, a sound server restarted), cpal's ALSA
//! worker reports the error to the stream's callback and polls again at once,
//! for as long as the stream lives: a thread spinning at full speed, and a
//! log filling at its pace. Bevy keeps its stream private with rodio's
//! logging callback, so nothing outside could end it. This module opens the
//! stream with a callback that raises a flag; the next frame drops the stream
//! (which stops the worker), clears the sounds that were playing on it and
//! tries the default device again every [`RETRY`] seconds.
//!
//! It plays what the game starts as Bevy would, minus what dbota never uses:
//! no spatial sound, no start offsets or clip durations.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::audio::{
    AudioLoader, AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, Decodable, GlobalVolume,
    PlaybackMode, PlaybackSettings, Source,
};
use bevy::prelude::*;
use rodio::cpal::StreamError;
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player};

/// Seconds between attempts to open the device again after it was lost.
const RETRY: f64 = 5.0;

/// A stream error after which the stream is of no more use. An underrun is a
/// glitch the stream goes on from.
fn is_fatal(error: &StreamError) -> bool {
    !matches!(error, StreamError::BufferUnderrun)
}

/// What became of the device in a frame.
#[derive(Debug, PartialEq, Eq)]
enum Change {
    /// The stream failed and was dropped.
    Lost,
    /// Time to open the device again.
    Reopen,
}

/// The stream being played on, if any, and when to try for one again.
struct Device<S> {
    stream: Option<S>,
    failed: Arc<AtomicBool>,
    retry_at: Option<f64>,
}

impl<S> Device<S> {
    /// A device whose first opening `open` made, handing it the flag its
    /// stream's error callback raises.
    fn new(open: impl FnOnce(Arc<AtomicBool>) -> Option<S>) -> Self {
        let failed = Arc::new(AtomicBool::new(false));
        Device {
            stream: open(Arc::clone(&failed)),
            failed,
            retry_at: None,
        }
    }

    /// Drops a stream whose callback raised the flag, and says when it is
    /// time to open another.
    fn supervise(&mut self, now: f64) -> Option<Change> {
        if self.stream.is_some() && self.failed.swap(false, Ordering::Acquire) {
            self.stream = None;
            self.retry_at = Some(now + RETRY);
            return Some(Change::Lost);
        }
        (self.stream.is_none() && self.retry_at.is_some_and(|at| now >= at))
            .then_some(Change::Reopen)
    }

    /// Opens another stream with `open`; a failure waits [`RETRY`] more.
    fn reopen(&mut self, now: f64, open: impl FnOnce(Arc<AtomicBool>) -> Option<S>) {
        assert!(self.stream.is_none(), "reopening a stream that is open");
        self.failed = Arc::new(AtomicBool::new(false));
        self.stream = open(Arc::clone(&self.failed));
        self.retry_at = self.stream.is_none().then_some(now + RETRY);
    }
}

/// The default device's stream, which raises `failed` when it breaks.
fn open(failed: Arc<AtomicBool>) -> Option<MixerDeviceSink> {
    let mut stream = DeviceSinkBuilder::from_default_device()
        .and_then(|builder| {
            builder
                .with_error_callback(move |error| {
                    if is_fatal(&error) && !failed.swap(true, Ordering::Release) {
                        warn!("audio stream error: {error}");
                    }
                })
                .open_stream()
        })
        .inspect_err(|error| warn!("no audio device: {error}"))
        .ok()?;
    stream.log_on_drop(false);
    Some(stream)
}

/// A sound that ends its entity when it has played out.
#[derive(Component)]
struct Despawn;

/// A sound that ends its audio components when it has played out.
#[derive(Component)]
struct Remove;

#[derive(Resource)]
struct Output(Device<MixerDeviceSink>);

pub fn plugin(app: &mut App) {
    // A test plays no sound on the machine it runs on: its client opens no device, and what it
    // would play is told by the players it spawns.
    #[cfg(test)]
    let device = Device::new(|_| None);
    #[cfg(not(test))]
    let device = Device::new(open);
    app.init_resource::<GlobalVolume>()
        .init_asset::<AudioSource>()
        .init_asset_loader::<AudioLoader>()
        .insert_resource(Output(device))
        .add_systems(
            PostUpdate,
            (supervise, play_queued, cleanup_finished).chain(),
        );
}

/// Notices a lost stream: the sounds that were playing on it are gone, a loop
/// starts again on the next stream and anything else is stale by then.
fn supervise(
    mut output: ResMut<Output>,
    time: Res<Time<Real>>,
    sinks: Query<(Entity, Has<Despawn>), With<AudioSink>>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs_f64();
    match output.0.supervise(now) {
        Some(Change::Lost) => {
            for (entity, once) in &sinks {
                if once {
                    commands.entity(entity).despawn();
                } else {
                    commands.entity(entity).remove::<AudioSink>();
                }
            }
        }
        Some(Change::Reopen) => output.0.reopen(now, open),
        None => {}
    }
}

/// Starts the sounds that were asked for and have loaded. With no stream a
/// loop waits for one; the rest is dropped, or they would pile up unheard.
fn play_queued(
    output: Res<Output>,
    sources: Res<Assets<AudioSource>>,
    global: Res<GlobalVolume>,
    queued: Query<(Entity, &AudioPlayer, &PlaybackSettings), Without<AudioSink>>,
    mut commands: Commands,
) {
    let mixer = output.0.stream.as_ref().map(MixerDeviceSink::mixer);
    for (entity, player, settings) in &queued {
        let Some(mixer) = mixer else {
            if !matches!(settings.mode, PlaybackMode::Loop) {
                commands.entity(entity).despawn();
            }
            continue;
        };
        let Some(source) = sources.get(&player.0) else {
            continue;
        };
        let player = Player::connect_new(mixer);
        if matches!(settings.mode, PlaybackMode::Loop) {
            player.append(source.decoder().repeat_infinite());
        } else {
            player.append(source.decoder());
        }
        let mut sink = AudioSink::new(player);
        if settings.muted {
            sink.mute();
        }
        sink.set_speed(settings.speed);
        sink.set_volume(settings.volume * global.volume);
        if settings.paused {
            sink.pause();
        }
        let mut entity = commands.entity(entity);
        match settings.mode {
            PlaybackMode::Loop | PlaybackMode::Once => entity.insert(sink),
            PlaybackMode::Despawn => entity.insert((sink, Despawn)),
            PlaybackMode::Remove => entity.insert((sink, Remove)),
        };
    }
}

/// Ends the sounds that have played out, as their mode says.
fn cleanup_finished(
    sinks: Query<(Entity, &AudioSink, Has<Despawn>, Has<Remove>)>,
    mut commands: Commands,
) {
    for (entity, sink, despawn, remove) in &sinks {
        if !(despawn || remove) || !sink.empty() {
            continue;
        }
        if despawn {
            commands.entity(entity).despawn();
        } else {
            commands
                .entity(entity)
                .remove::<(AudioPlayer, AudioSink, PlaybackSettings, Remove)>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream that counts how often it is dropped.
    struct Stream(Arc<AtomicBool>);
    impl Drop for Stream {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    /// The error callback as `open` builds it, driven by the errors a failing
    /// device reports.
    fn callback(failed: &Arc<AtomicBool>) -> impl FnMut(StreamError) + use<> {
        let failed = Arc::clone(failed);
        move |error| {
            if is_fatal(&error) {
                failed.store(true, Ordering::Release);
            }
        }
    }

    /// A device that goes is dropped at once, so its worker stops, and the
    /// default device is tried again every [`RETRY`] seconds until one opens.
    #[test]
    fn a_lost_device_is_dropped_and_then_tried_again_until_it_opens() {
        let dropped = Arc::new(AtomicBool::new(false));
        let mut report = None;
        let mut device = Device::new(|failed| {
            report = Some(callback(&failed));
            Some(Stream(Arc::clone(&dropped)))
        });
        let mut report = report.expect("opened");
        assert_eq!(device.supervise(1.0), None);
        report(StreamError::BufferUnderrun);
        assert_eq!(device.supervise(1.1), None, "an underrun is a glitch");

        report(StreamError::DeviceNotAvailable);
        assert_eq!(device.supervise(2.0), Some(Change::Lost));
        assert!(dropped.load(Ordering::SeqCst), "the stream is let go");
        assert!(device.stream.is_none());
        assert_eq!(device.supervise(2.0 + RETRY / 2.0), None);

        assert_eq!(device.supervise(2.0 + RETRY), Some(Change::Reopen));
        device.reopen(2.0 + RETRY, |_| None);
        assert_eq!(
            device.supervise(2.0 + RETRY),
            None,
            "a failed try waits again"
        );
        assert_eq!(device.supervise(2.0 + 2.0 * RETRY), Some(Change::Reopen));
        device.reopen(2.0 + 2.0 * RETRY, |_| Some(Stream(Arc::default())));
        assert_eq!(device.supervise(100.0), None);
    }

    /// With no device at the start there is none to lose and none to wait for.
    #[test]
    fn a_machine_without_a_device_is_left_alone() {
        let mut device = Device::<Stream>::new(|_| None);
        assert_eq!(device.supervise(1000.0), None);
    }
}
