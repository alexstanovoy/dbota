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
    AudioLoader, AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackMode,
    PlaybackSettings, Source, Volume,
};
use bevy::prelude::*;
use rodio::Player;
use rodio::cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rodio::cpal::{self, StreamError};
use rodio::mixer::{Mixer, MixerSource};

mod presentation;
use presentation::{Clock, Render};
pub use presentation::{Played, VoiceClock};

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

/// When the silent output takes its next step, kept apart from the clock so that it can be driven
/// by a simulated one. Steps are due one [`SILENT_STEP`] apart; one a little late is made up by
/// the next, but after a stall (more than a step behind) the missed time is not made up: a device
/// that was not served plays nothing for it, and sounds queued after the stall must not be
/// drained in a burst, ending sooner than their length.
struct Pace {
    due: Duration,
}

impl Pace {
    fn new(now: Duration) -> Pace {
        Pace { due: now }
    }

    /// How long to sleep after a step taken at `now` (time since the output opened).
    fn after_step(&mut self, now: Duration) -> Duration {
        self.due += SILENT_STEP;
        if now > self.due + SILENT_STEP {
            self.due = now + SILENT_STEP;
        }
        self.due.saturating_sub(now)
    }
}

/// What a step of the silent output takes: the mixer's samples for [`SILENT_STEP`].
#[cfg(test)]
struct Drain {
    source: MixerSource,
    per_step: usize,
}

#[cfg(test)]
impl Drain {
    fn new(source: MixerSource) -> Drain {
        let samples =
            u128::from(SILENT_RATE) * u128::from(SILENT_CHANNELS) * SILENT_STEP.as_millis() / 1000;
        Drain {
            source,
            per_step: usize::try_from(samples).expect("a step's samples fit"),
        }
    }

    fn step(&mut self) {
        for _ in 0..self.per_step {
            if self.source.next().is_none() {
                break;
            }
        }
    }
}

/// An output with no device behind it. A worker takes the mixer's samples at the pace of the
/// sample rate, a step at a time, sleeping between steps, so that a sound ends when it would on a
/// device and the thread is idle in between.
struct Silent {
    mixer: Mixer,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    presentation: Clock,
    opened: Instant,
}

impl Silent {
    #[allow(
        clippy::disallowed_methods,
        reason = "chosen only by an environment variable, which a browser never has"
    )]
    fn open() -> Silent {
        let (mixer, source) = silent_mixer();
        let stop = Arc::new(AtomicBool::new(false));
        let ending = Arc::clone(&stop);
        let opened = Instant::now();
        let presentation = Clock::new(source.sample_rate());
        let mut render = Render::new(source, presentation.clone());
        let worker = std::thread::spawn(move || {
            let samples = SILENT_RATE as usize
                * usize::from(SILENT_CHANNELS)
                * SILENT_STEP.as_millis() as usize
                / 1000;
            let mut buffer = vec![0.0f32; samples];
            let mut pace = Pace::new(Duration::ZERO);
            while !ending.load(Ordering::Acquire) {
                render.fill(&mut buffer, opened.elapsed().as_secs_f64());
                std::thread::sleep(pace.after_step(opened.elapsed()));
            }
        });
        Silent {
            mixer,
            stop,
            worker: Some(worker),
            presentation,
            opened,
        }
    }
}

/// The mixer of a silent output and the source its steps drain.
fn silent_mixer() -> (Mixer, MixerSource) {
    let channels = NonZero::new(SILENT_CHANNELS).expect("some channels");
    let rate = NonZero::new(SILENT_RATE).expect("some rate");
    rodio::mixer::mixer(channels, rate)
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
    Device(DeviceStream),
    Silent(Silent),
    #[cfg(test)]
    Pulled(Pulled),
}

impl Stream {
    fn mixer(&self) -> &Mixer {
        match self {
            Stream::Device(device) => &device.mixer,
            Stream::Silent(silent) => &silent.mixer,
            #[cfg(test)]
            Stream::Pulled(pulled) => &pulled.mixer,
        }
    }
    fn clock(&self) -> &Clock {
        match self {
            Self::Device(device) => &device.presentation,
            Self::Silent(silent) => &silent.presentation,
            #[cfg(test)]
            Self::Pulled(pulled) => &pulled.presentation,
        }
    }

    fn clock_time(&self, _now: Duration) -> f64 {
        match self {
            Self::Device(device) => device.clock_time(),
            Self::Silent(silent) => silent.opened.elapsed().as_secs_f64(),
            #[cfg(test)]
            Self::Pulled(pulled) => _now.saturating_sub(pulled.opened).as_secs_f64(),
        }
    }
}

/// A test's output pulled as a device pulls one, a buffer at a time on the app's own clock: a
/// device's callback takes [`DeviceBuffers::buffer`] of sound at once, [`DeviceBuffers::ahead`]
/// buffers ahead of what is heard, so a sound's reported position moves in those steps. With
/// the resource present, a test's client plays to it instead of to nothing.
#[cfg(test)]
#[derive(Resource, Clone, Copy, Debug)]
pub struct DeviceBuffers {
    pub buffer: Duration,
    pub ahead: u32,
    pub sample_rate: u32,
}

#[cfg(test)]
struct Pulled {
    mixer: Mixer,
    render: std::sync::Mutex<Render>,
    presentation: Clock,
    buffer: Vec<f32>,
    /// How much sound the device has taken since it opened, and when it opened.
    taken: Duration,
    opened: Duration,
}

/// Pulls the test's output up to [`DeviceBuffers::ahead`] buffers past now, a whole buffer at a
/// time, opening it first.
#[cfg(test)]
fn pull_buffers(
    buffers: Option<Res<DeviceBuffers>>,
    time: Res<Time<Real>>,
    mut output: ResMut<Output>,
) {
    let Some(buffers) = buffers else {
        return;
    };
    let now = time.elapsed();
    let stream = output.0.stream.get_or_insert_with(|| {
        let channels = NonZero::new(SILENT_CHANNELS).expect("some channels");
        let rate = NonZero::new(buffers.sample_rate).expect("a device sample rate");
        let (mixer, source) = rodio::mixer::mixer(channels, rate);
        let presentation = Clock::new(rate);
        Stream::Pulled(Pulled {
            mixer,
            render: std::sync::Mutex::new(Render::new(source, presentation.clone())),
            presentation,
            buffer: Vec::new(),
            taken: Duration::ZERO,
            opened: now,
        })
    });
    let Stream::Pulled(pulled) = stream else {
        return;
    };
    let frames = (f64::from(buffers.sample_rate) * buffers.buffer.as_secs_f64()).round();
    let duration = Duration::from_secs_f64(frames / f64::from(buffers.sample_rate));
    pulled
        .buffer
        .resize(frames as usize * usize::from(SILENT_CHANNELS), 0.0);
    let mut render = pulled.render.lock().expect("the pulled source");
    let wanted = now - pulled.opened + duration * buffers.ahead;
    for _ in 0..1_000 {
        if pulled.taken >= wanted {
            break;
        }
        render.fill(&mut pulled.buffer, pulled.taken.as_secs_f64());
        pulled.taken += duration;
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

struct DeviceStream {
    #[cfg_attr(
        not(target_arch = "wasm32"),
        expect(dead_code, reason = "owns the output callback")
    )]
    stream: cpal::Stream,
    mixer: Mixer,
    presentation: Clock,
    #[cfg(not(target_arch = "wasm32"))]
    opened: Instant,
}

impl DeviceStream {
    fn clock_time(&self) -> f64 {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.opened.elapsed().as_secs_f64()
        }
        #[cfg(target_arch = "wasm32")]
        {
            let cpal::platform::StreamInner::WebAudio(stream) = self.stream.as_inner();
            let context = stream.audio_context();
            let read = |object: &wasm_bindgen::JsValue, key: &str| {
                js_sys::Reflect::get(object, &key.into()).ok()
            };
            use wasm_bindgen::JsCast as _;
            if let Some(time) = read(context.as_ref(), "getOutputTimestamp")
                .and_then(|method| method.dyn_into::<js_sys::Function>().ok())
                .and_then(|method| method.call0(context.as_ref()).ok())
                .and_then(|stamp| read(&stamp, "contextTime"))
                .and_then(|time| time.as_f64())
            {
                return time;
            }
            let latency = |key| {
                read(context.as_ref(), key)
                    .and_then(|time| time.as_f64())
                    .unwrap_or(0.0)
            };
            (context.current_time() - latency("baseLatency") - latency("outputLatency")).max(0.0)
        }
    }
}

/// Preserve the default device's format and the existing buffer sizing policy while keeping
/// the callback's playback timestamp, which the mixer's decoder position does not include.
fn open_device(failed: Arc<AtomicBool>) -> Option<DeviceStream> {
    let open = || -> Result<DeviceStream, Box<dyn std::error::Error>> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("no default output")?;
        let supported = device.default_output_config()?;
        let mut config = supported.config();
        let target = (config.sample_rate / 20).max(1);
        let upper = target.next_power_of_two();
        let lower = (upper / 2).max(1);
        config.buffer_size = cpal::BufferSize::Fixed(if target - lower <= upper - target {
            lower
        } else {
            upper
        });
        let channels = NonZero::new(config.channels).ok_or("no output channels")?;
        let rate = NonZero::new(config.sample_rate).ok_or("zero output sample rate")?;
        let (mixer, samples) = rodio::mixer::mixer(channels, rate);
        let presentation = Clock::new(rate);
        let mut render = Render::new(samples, presentation.clone());
        #[cfg(not(target_arch = "wasm32"))]
        let opened = Instant::now();
        let error = move |error| {
            if is_fatal(&error) && !failed.swap(true, Ordering::Release) {
                warn!("audio stream error: {error}");
            }
        };
        macro_rules! open_format {
            ($($format:ident => $sample:ty),+ $(,)?) => {
                match supported.sample_format() {
                    $(cpal::SampleFormat::$format => device.build_output_stream(
                        &config,
                        move |data: &mut [$sample], info| {
                            let stamp = info.timestamp();
                            #[cfg(not(target_arch = "wasm32"))]
                            let at = opened.elapsed().as_secs_f64()
                                + stamp.playback.duration_since(&stamp.callback)
                                    .map_or(0.0, |latency| latency.as_secs_f64());
                            #[cfg(target_arch = "wasm32")]
                            let at = stamp.playback.max(stamp.callback)
                                .duration_since(&cpal::StreamInstant::new(0, 0))
                                .map_or(0.0, |time| time.as_secs_f64());
                            render.fill(data, at);
                        },
                        error,
                        None,
                    )?,)+
                    _ => return Err("unsupported output sample format".into()),
                }
            }
        }
        let stream = open_format!(
            F32 => f32, F64 => f64,
            I8 => i8, I16 => i16, I24 => cpal::I24, I32 => i32, I64 => i64,
            U8 => u8, U16 => u16, U24 => cpal::U24, U32 => u32, U64 => u64,
        );
        stream.play()?;
        Ok(DeviceStream {
            stream,
            mixer,
            presentation,
            #[cfg(not(target_arch = "wasm32"))]
            opened,
        })
    };
    open()
        .inspect_err(|error| warn!("no audio device: {error}"))
        .ok()
}

/// How long a loop takes to rise when it starts and to fall when it ends: a few hundred samples,
/// too short to be heard as a fade and long enough that a loop is never cut at a sample far from
/// zero (a click).
pub const TAPER: Duration = Duration::from_millis(12);

/// The end of a loop that is to be told to fall: the loop's source reads the flag, so the fall is
/// made by the sample, not by the frame.
#[derive(Component, Clone, Default)]
pub struct Taper(Arc<AtomicBool>);

impl Taper {
    fn is_ending(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// A loop that is falling: it is no longer tuned (the balance does not reach it, so that a mute
/// does not cut it before its source has fallen) and it is dropped once [`TAPER`] has passed.
#[derive(Component)]
pub struct Ending {
    since: Duration,
}

/// Ends the loop `entity` at `now` without a cut. One that has not begun sounding has nothing to
/// let fall and goes at once.
pub fn end_softly(
    commands: &mut Commands,
    entity: Entity,
    taper: &Taper,
    sounding: bool,
    now: Duration,
) {
    if !sounding {
        commands.entity(entity).despawn();
        return;
    }
    taper.0.store(true, Ordering::Relaxed);
    commands
        .entity(entity)
        .remove::<(crate::channels::Tuned, AudioPlayer)>()
        .insert(Ending { since: now });
}

/// Drops the loops that have fallen.
fn finish_endings(endings: Query<(Entity, &Ending)>, time: Res<Time>, mut commands: Commands) {
    for (entity, ending) in &endings {
        if time.elapsed().saturating_sub(ending.since) >= TAPER {
            commands.entity(entity).despawn();
        }
    }
}

/// A loop's samples with a rise at its start and a fall once its [`Taper`] says it ends, after
/// which it is exhausted.
struct Tapered<S> {
    inner: S,
    ending: Taper,
    gain: f32,
    /// How much the gain moves by each sample.
    step: f32,
}

impl<S: Source<Item = f32>> Tapered<S> {
    fn new(inner: S, ending: Taper) -> Self {
        let samples = f32::from(inner.channels().get())
            * inner.sample_rate().get() as f32
            * TAPER.as_secs_f32();
        Tapered {
            inner,
            ending,
            gain: 0.0,
            step: 1.0 / samples.max(1.0),
        }
    }
}

impl<S: Source<Item = f32>> Iterator for Tapered<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.ending.is_ending() {
            self.gain -= self.step;
            if self.gain <= 0.0 {
                return None;
            }
        } else if self.gain < 1.0 {
            self.gain = (self.gain + self.step).min(1.0);
        }
        self.inner.next().map(|sample| sample * self.gain)
    }
}

impl<S: Source<Item = f32>> Source for Tapered<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// A sound that ends its entity when it has played out.
#[derive(Component)]
struct Despawn;

/// A sound that ends its audio components when it has played out.
#[derive(Component)]
struct Remove;

#[derive(Resource)]
pub(crate) struct Output(Device<Stream>);

impl Output {
    pub(crate) fn presentation(
        &self,
        now: Duration,
    ) -> Option<impl Fn(&VoiceClock, bool) -> Played + '_> {
        let stream = self.0.stream.as_ref()?;
        let clock = stream.clock();
        let frame = clock.frame_at(stream.clock_time(now));
        Some(move |voice: &VoiceClock, queued| voice.played(clock, frame, queued))
    }
}

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct BeforePortrait;

pub fn plugin(app: &mut App) {
    // A test plays no sound on the machine it runs on: its client opens no device, and what it
    // would play is told by the players it spawns.
    #[cfg(test)]
    let device = Device::new(|_| None);
    #[cfg(not(test))]
    let device = Device::new(open);
    app.init_asset::<AudioSource>()
        .init_asset_loader::<AudioLoader>()
        .insert_resource(Output(device))
        .add_systems(
            PostUpdate,
            (
                supervise,
                play_queued,
                crate::channels::follow_levels,
                finish_endings,
                cleanup_finished,
            )
                .chain()
                .before(BeforePortrait),
        )
        .configure_sets(PostUpdate, BeforePortrait.before(crate::portrait::Aimed));
    #[cfg(test)]
    app.add_systems(PostUpdate, pull_buffers.before(supervise));
}

/// Notices a lost stream: the sounds that were playing on it are gone, a loop
/// starts again on the next stream and anything else is stale by then.
fn supervise(
    mut output: ResMut<Output>,
    time: Res<Time<Real>>,
    sinks: Query<(Entity, Has<Despawn>), With<AudioSink>>,
    mut speech: Option<ResMut<crate::talk::Speech>>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs_f64();
    match output.0.supervise(now) {
        Some(Change::Lost) => {
            if let Some(speech) = &mut speech {
                speech.stop();
            }
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
    settings: Res<crate::config::Settings>,
    args: Option<Res<crate::Args>>,
    queued: Query<
        (
            Entity,
            &AudioPlayer,
            &PlaybackSettings,
            Option<&crate::channels::Tuned>,
            Option<&Taper>,
            Option<&VoiceClock>,
        ),
        Without<AudioSink>,
    >,
    mut commands: Commands,
) {
    let muted_by_run = args.is_some_and(|args| args.mute);
    for (entity, player, playback, tuned, taper, voice) in &queued {
        let Some(stream) = output.0.stream.as_ref() else {
            if !matches!(playback.mode, PlaybackMode::Loop) {
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
        let player = Player::connect_new(stream.mixer());
        // The sound begins at the loudness the balance gives it now, not at its own and then the
        // balance's a system later: the output's worker does not wait for the frame's systems,
        // and a sound started under a mute must not be heard for the length of a pull.
        let loudness = tuned.map_or(playback.volume.to_linear(), |tuned| {
            tuned.base * crate::channels::live_gain(&settings, muted_by_run, tuned.channel)
        });
        player.set_volume(loudness);
        if playback.paused {
            player.pause();
        }
        if matches!(playback.mode, PlaybackMode::Loop) {
            player.append(Tapered::new(
                decoder.repeat_infinite(),
                taper.cloned().unwrap_or_default(),
            ));
        } else if let Some(voice) = voice {
            player.set_speed(voice.speed());
            player.append(voice.track(stream.clock().clone(), decoder));
        } else {
            player.append(decoder);
        }
        let mut sink = AudioSink::new(player);
        if playback.muted {
            sink.mute();
        }
        sink.set_speed(playback.speed);
        // Later changes of the balance are [`crate::channels::follow_levels`]'s.
        sink.set_volume(Volume::Linear(loudness));
        let mut entity = commands.entity(entity);
        match playback.mode {
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

/// A sound begins at the balance: the first samples a worker pulls from a sound that started
/// under a mute are silent, and those of one that did not are at the loudness the balance gives.
#[cfg(test)]
mod start_under_the_balance {
    use super::*;
    use crate::channels::{Channel, Tuned};
    use crate::config::Settings;
    use bevy::ecs::system::RunSystemOnce;

    /// How loud the test sound is of itself: a quarter of full scale.
    const SAMPLE: f32 = 0.25;
    /// What a worker pulls between the start of a sound and the next system of the frame.
    const PULL: usize = 4096;

    fn constant_source() -> AudioSource {
        let mut bytes = std::io::Cursor::new(Vec::new());
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: SILENT_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut wav = hound::WavWriter::new(&mut bytes, spec).expect("a wav writer");
        for _ in 0..SILENT_RATE {
            wav.write_sample((SAMPLE * f32::from(i16::MAX)) as i16)
                .expect("a sample");
        }
        wav.finalize().expect("a finished wav");
        AudioSource {
            bytes: bytes.into_inner().into(),
        }
    }

    /// The samples a pull takes from a sound of `channel` that [`play_queued`] started in `mode`,
    /// with the balance `settings` and the run's mute.
    fn first_samples(
        settings: Settings,
        muted_by_run: bool,
        channel: Channel,
        mode: PlaybackSettings,
    ) -> Vec<f32> {
        let (mixer, mut samples) = silent_mixer();
        let silent = Silent {
            mixer,
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
            presentation: Clock::new(samples.sample_rate()),
            opened: Instant::now(),
        };
        let mut world = World::new();
        world.insert_resource(Output(Device::new(|_| Some(Stream::Silent(silent)))));
        let mut sources = Assets::<AudioSource>::default();
        let handle = sources.add(constant_source());
        world.insert_resource(sources);
        world.insert_resource(settings);
        if muted_by_run {
            let args = <crate::Args as clap::Parser>::parse_from(["dbota", "--mute"]);
            assert!(args.mute, "the run is muted");
            world.insert_resource(args);
        }
        world.spawn((AudioPlayer::new(handle), mode, Tuned { channel, base: 0.8 }));
        world.run_system_once(play_queued).expect("play queued");
        // The worker pulls here, before `follow_levels` has run.
        let pulled: Vec<f32> = samples.by_ref().take(PULL).collect();
        assert_eq!(pulled.len(), PULL, "the output pulls audio");
        pulled
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().copied().map(f32::abs).fold(0.0, f32::max)
    }

    #[test]
    fn an_audible_voice_resumes_after_the_mixer_clock_passes_its_normal_end() {
        use crate::mixer::{Mixer as GameMixer, Request};

        let (mixer, samples) = silent_mixer();
        let clock = Clock::new(samples.sample_rate());
        let output = Silent {
            mixer,
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
            presentation: clock.clone(),
            opened: Instant::now(),
        };
        let mut render = Render::new(samples, clock.clone());
        let tracked = VoiceClock::new(1.0);
        let mut world = World::new();
        world.insert_resource(Output(Device::new(|_| Some(Stream::Silent(output)))));
        let mut sources = Assets::<AudioSource>::default();
        let source = sources.add(constant_source());
        world.insert_resource(sources);
        world.insert_resource(Settings::default());
        world.insert_resource(GameMixer::default());
        world.resource_scope(|world, mut mixer: Mut<GameMixer>| {
            let mut commands = world.commands();
            let request = Request {
                event: "spoken",
                channel: Channel::Voices,
                ..Request::default()
            };
            let player = mixer
                .start_heard(&mut commands, Duration::ZERO, &request, source)
                .expect("the voice admitted");
            commands.entity(player).insert(tracked.clone());
        });
        world.flush();
        world.run_system_once(play_queued).expect("play queued");
        let mut buffer = [0.0; PULL];
        render.fill(&mut buffer, 0.0);
        let channels = usize::from(SILENT_CHANNELS);
        let first = buffer
            .chunks_exact(channels)
            .position(|frame| peak(frame) > 0.01)
            .expect("the first buffer had voice PCM");
        let before = peak(&buffer);
        world
            .run_system_once(
                |mut commands: Commands,
                 mut mixer: ResMut<GameMixer>,
                 mut tuned: Query<&mut Tuned>| {
                    mixer.tick(&mut commands, Duration::from_secs(11), &mut tuned, None);
                },
            )
            .expect("advance the mixer while the output is stalled");
        render.fill(&mut buffer, 11.0);
        assert!(
            (peak(&buffer[buffer.len() / 2..]) - before).abs() < 0.002,
            "the mixer clock clipped the resumed voice's PCM"
        );
        let played = tracked.played(&clock, clock.frame_at(11.01), true);
        let expected = (buffer.len() / channels - first) as f32 / SILENT_RATE as f32 + 0.01;
        assert!(
            matches!(played, Played::At(at) if (at - expected).abs() < 0.0002),
            "the resumed buffer's audible media position should be {expected:.4}, got {played:?}"
        );
    }

    #[test]
    fn a_sound_started_under_a_mute_is_silent_from_its_first_sample() {
        for channel in Channel::ALL {
            for mode in [PlaybackSettings::ONCE, PlaybackSettings::LOOP] {
                let mutes: [(&str, Box<dyn Fn(&mut Settings)>, bool); 5] = [
                    ("mute all", Box::new(|s| s.muted = true), false),
                    (
                        "the channel's mute",
                        Box::new(move |s| s.mix.set_muted(channel, true)),
                        false,
                    ),
                    (
                        "the channel's level at nothing",
                        Box::new(move |s| s.mix.set_level(channel, 0.0)),
                        false,
                    ),
                    ("the master at nothing", Box::new(|s| s.volume = 0.0), false),
                    ("the run's mute", Box::new(|_| {}), true),
                ];
                for (what, mute, by_run) in mutes {
                    let mut settings = Settings::default();
                    mute(&mut settings);
                    let pulled = first_samples(settings, by_run, channel, mode);
                    assert_eq!(
                        peak(&pulled),
                        0.0,
                        "{channel:?} {mode:?} under {what}: the first samples are heard"
                    );
                }
            }
        }
    }

    #[test]
    fn a_sound_starts_at_its_loudness_times_the_channels_level_and_the_master() {
        for channel in Channel::ALL {
            let mut settings = Settings {
                volume: 0.5,
                ..Settings::default()
            };
            settings.mix.set_level(channel, 0.4);
            let pulled = first_samples(settings, false, channel, PlaybackSettings::ONCE);
            let wanted = SAMPLE * 0.8 * 0.5 * 0.4;
            let at_end = pulled[PULL - 1].abs();
            assert!(
                (at_end - wanted).abs() < 0.002,
                "{channel:?}: the sound plays at {at_end}, not {wanted}"
            );
            assert!(
                peak(&pulled) <= wanted + 0.002,
                "{channel:?}: no sample is louder than the balance gives: {}",
                peak(&pulled)
            );
        }
    }
}

/// A loop rises when it starts and falls when it ends, by the sample.
#[cfg(test)]
mod tapered_loops {
    use super::*;

    fn constant() -> rodio::buffer::SamplesBuffer {
        rodio::buffer::SamplesBuffer::new(
            NonZero::new(1).expect("a channel"),
            NonZero::new(SILENT_RATE).expect("a rate"),
            vec![1.0_f32; 4 * SILENT_RATE as usize],
        )
    }

    /// Samples in [`TAPER`].
    fn taper_samples() -> usize {
        (SILENT_RATE as f32 * TAPER.as_secs_f32()).ceil() as usize
    }

    #[test]
    fn a_loop_rises_from_silence_to_full_in_one_taper_without_a_step() {
        let taper = Taper::default();
        let heard: Vec<f32> = Tapered::new(constant(), taper)
            .take(3 * taper_samples())
            .collect();
        assert!(
            heard[0] < 0.01,
            "the first sample is near silence: {}",
            heard[0]
        );
        assert_eq!(heard[taper_samples() + 1], 1.0, "full after one taper");
        let widest = heard
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(widest < 2.0 / taper_samples() as f32, "no step: {widest}");
    }

    #[test]
    fn a_loop_that_ends_falls_to_silence_in_one_taper_and_is_then_exhausted() {
        let taper = Taper::default();
        let mut loop_ = Tapered::new(constant(), taper.clone());
        let before: Vec<f32> = loop_.by_ref().take(3 * taper_samples()).collect();
        assert_eq!(before.last(), Some(&1.0), "it was at full");
        taper.0.store(true, Ordering::Relaxed);
        let after: Vec<f32> = loop_.by_ref().take(3 * taper_samples()).collect();
        assert!(
            after.len() <= taper_samples() + 1,
            "it ends within a taper: {}",
            after.len()
        );
        assert!(
            after.first().is_some_and(|first| *first > 0.9),
            "it falls from full"
        );
        assert!(after.windows(2).all(|w| w[1] <= w[0]), "it only falls");
        let widest = std::iter::once(1.0)
            .chain(after.iter().copied())
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(widest < 2.0 / taper_samples() as f32, "no step: {widest}");
        assert_eq!(loop_.next(), None, "exhausted");
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

    /// A sound of a third of a second, as the silent output's mixer takes it.
    fn a_third_of_a_second(mixer: &Mixer) -> Player {
        let samples = vec![0.0; (SILENT_RATE * u32::from(SILENT_CHANNELS) / 3) as usize];
        let sound = rodio::buffer::SamplesBuffer::new(
            NonZero::new(SILENT_CHANNELS).expect("channels"),
            NonZero::new(SILENT_RATE).expect("rate"),
            samples,
        );
        let player = Player::connect_new(mixer);
        player.append(sound);
        player
    }

    /// Runs the silent output's steps on a simulated clock from `from` until `player` has played
    /// out, a step at the time its pace says; the time it ended.
    fn until_over(
        drain: &mut Drain,
        pace: &mut Pace,
        mut now: Duration,
        player: &Player,
    ) -> Duration {
        for _ in 0..1000 {
            drain.step();
            if player.empty() {
                return now;
            }
            now += pace.after_step(now).max(Duration::from_millis(1));
        }
        panic!("the sound never ended");
    }

    /// The silent output keeps the pace of a device: a sound of a third of a second is playing
    /// when it is started and over only once a third of a second of the clock has passed.
    #[test]
    fn a_silent_output_plays_a_sound_out_in_its_own_length() {
        let (mixer, source) = silent_mixer();
        let mut drain = Drain::new(source);
        let mut pace = Pace::new(Duration::ZERO);
        let player = a_third_of_a_second(&mixer);
        assert!(!player.empty(), "a sound just started is playing");
        let ended = until_over(&mut drain, &mut pace, Duration::ZERO, &player);
        assert!(
            ended >= Duration::from_millis(300) && ended <= Duration::from_millis(360),
            "a third of a second of sound ended at {ended:?}"
        );
    }

    /// A stall of the worker does not make up its time on what is queued after it: a sound
    /// started after a pause of seconds plays out in its own length, not in a burst.
    #[test]
    fn a_stalled_silent_output_does_not_drain_a_sound_started_after_the_stall_early() {
        let (mixer, source) = silent_mixer();
        let mut drain = Drain::new(source);
        let mut pace = Pace::new(Duration::ZERO);
        drain.step();
        let stalled_until = Duration::from_secs(5);
        let sleep = pace.after_step(stalled_until);
        assert_eq!(sleep, SILENT_STEP, "a step after a stall waits a full step");
        let player = a_third_of_a_second(&mixer);
        let began = stalled_until + sleep;
        let ended = until_over(&mut drain, &mut pace, began, &player);
        assert!(
            ended - began >= Duration::from_millis(300),
            "a sound queued after a stall ended after {:?}",
            ended - began
        );
    }

    /// A step a little late is made up by the next; the pace is still one step to a step.
    #[test]
    fn a_step_a_little_late_is_made_up_by_the_next() {
        let mut pace = Pace::new(Duration::ZERO);
        let first = pace.after_step(Duration::ZERO);
        assert_eq!(first, SILENT_STEP);
        let late = pace.after_step(SILENT_STEP + Duration::from_millis(15));
        assert_eq!(late, Duration::from_millis(5));
    }

    /// With no device at the start there is none to lose and none to wait for.
    #[test]
    fn a_machine_without_a_device_is_left_alone() {
        let mut device = Device::<Stream>::new(|_| None);
        assert_eq!(device.supervise(1000.0), None);
    }
}
