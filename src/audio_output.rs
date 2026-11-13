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
//!
//! A client that tests, checks or measures run never reaches a real device: the wrappers that
//! isolate those clients (`tools/client_env.sh`, `tests/common`) set [`SILENT_VARIABLE`], and
//! such a client plays to a [`Silent`] output instead, which takes the sounds in at the pace a
//! device would and plays them to nobody. The mixer, the players and their timing stay real.

use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bevy::audio::{
    AudioLoader, AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, GlobalVolume,
    PlaybackMode, PlaybackSettings, Source,
};
use bevy::prelude::*;
use rodio::cpal::StreamError;
use rodio::mixer::Mixer;
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player};

/// The variable that makes the client play to no device. Anything but empty or `0` asks for it.
pub const SILENT_VARIABLE: &str = "DBOTA_NULL_AUDIO";

/// Which output the client plays to.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Kind {
    /// The machine's default device.
    Device,
    /// Nobody: [`Silent`].
    Silent,
}

impl Kind {
    /// The output a value of [`SILENT_VARIABLE`] asks for.
    fn of(value: Option<&str>) -> Kind {
        match value {
            Some(value) if !value.is_empty() && value != "0" => Kind::Silent,
            _ => Kind::Device,
        }
    }

    /// What `dbota audio-output` says of it (a browser has no such command).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn name(self) -> &'static str {
        match self {
            Kind::Device => "device",
            Kind::Silent => "silent",
        }
    }

    /// The output this process was asked for.
    pub fn requested() -> Kind {
        Kind::of(std::env::var(SILENT_VARIABLE).ok().as_deref())
    }
}

/// Channels and rate of the silent output's mixer, those of a common device.
const SILENT_CHANNELS: u16 = 2;
const SILENT_RATE: u32 = 44_100;
/// How much of the sound the silent output takes in at a time, which is also how often it wakes.
const SILENT_STEP: Duration = Duration::from_millis(20);

/// An output with no device behind it. A worker takes the mixer's samples at the pace of the
/// sample rate, a step at a time, sleeping between steps, so that a sound ends when it would on a
/// device and the thread is idle in between.
struct Silent {
    mixer: Mixer,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Silent {
    fn open() -> Silent {
        let channels = NonZero::new(SILENT_CHANNELS).expect("some channels");
        let rate = NonZero::new(SILENT_RATE).expect("some rate");
        let (mixer, mut source) = rodio::mixer::mixer(channels, rate);
        let stop = Arc::new(AtomicBool::new(false));
        let ending = Arc::clone(&stop);
        let per_step = (u128::from(SILENT_RATE)
            * u128::from(SILENT_CHANNELS)
            * SILENT_STEP.as_millis()
            / 1000) as usize;
        let worker = std::thread::spawn(move || {
            let mut due = Instant::now();
            while !ending.load(Ordering::Acquire) {
                for _ in 0..per_step {
                    if source.next().is_none() {
                        break;
                    }
                }
                due += SILENT_STEP;
                std::thread::sleep(due.saturating_duration_since(Instant::now()));
            }
        });
        Silent {
            mixer,
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for Silent {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// What the client plays to.
enum Stream {
    Device(MixerDeviceSink),
    Silent(Silent),
}

impl Stream {
    fn mixer(&self) -> &Mixer {
        match self {
            Stream::Device(device) => device.mixer(),
            Stream::Silent(silent) => &silent.mixer,
        }
    }
}

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

/// The stream the client was asked for: the silent output, or the default device's stream, which
/// raises `failed` when it breaks.
fn open(failed: Arc<AtomicBool>) -> Option<Stream> {
    if Kind::requested() == Kind::Silent {
        info!("audio output: silent ({SILENT_VARIABLE})");
        return Some(Stream::Silent(Silent::open()));
    }
    open_device(failed).map(Stream::Device)
}

/// The default device's stream, which raises `failed` when it breaks.
fn open_device(failed: Arc<AtomicBool>) -> Option<MixerDeviceSink> {
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
struct Output(Device<Stream>);

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
    let mixer = output.0.stream.as_ref().map(Stream::mixer);
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
        // The cache's file, decoded only as the importer writes them (Bevy's decoder panics on
        // any other).
        let decoder = match crate::sound_file::decoder(source.clone()) {
            Ok(decoder) => decoder,
            Err(why) => {
                warn!("sound {:?} is not played: {why}", player.0.path());
                commands.entity(entity).despawn();
                continue;
            }
        };
        let player = Player::connect_new(mixer);
        if matches!(settings.mode, PlaybackMode::Loop) {
            player.append(decoder.repeat_infinite());
        } else {
            player.append(decoder);
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

    #[test]
    fn only_a_set_variable_asks_for_the_silent_output() {
        assert_eq!(Kind::of(None), Kind::Device);
        assert_eq!(Kind::of(Some("")), Kind::Device);
        assert_eq!(Kind::of(Some("0")), Kind::Device);
        assert_eq!(Kind::of(Some("1")), Kind::Silent);
    }

    /// The silent output keeps the pace of a device: a sound of a third of a second is not over
    /// when it is started, and is not over before a quarter of a second has passed, but it ends.
    #[test]
    fn a_silent_output_plays_a_sound_out_at_the_pace_of_a_device() {
        let silent = Silent::open();
        let samples = vec![0.0; (SILENT_RATE * u32::from(SILENT_CHANNELS) / 3) as usize];
        let sound = rodio::buffer::SamplesBuffer::new(
            NonZero::new(SILENT_CHANNELS).expect("channels"),
            NonZero::new(SILENT_RATE).expect("rate"),
            samples,
        );
        let player = Player::connect_new(&silent.mixer);
        let started = Instant::now();
        player.append(sound);
        assert!(!player.empty(), "a sound just started is playing");
        while !player.empty() {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the sound never ended"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            started.elapsed() >= Duration::from_millis(250),
            "a third of a second of sound ended after {:?}",
            started.elapsed()
        );
    }

    /// With no device at the start there is none to lose and none to wait for.
    #[test]
    fn a_machine_without_a_device_is_left_alone() {
        let mut device = Device::<Stream>::new(|_| None);
        assert_eq!(device.supervise(1000.0), None);
    }
}
