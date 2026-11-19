use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use bevy::prelude::*;
use rodio::cpal;
use rodio::mixer::MixerSource;
use rodio::{ChannelCount, SampleRate, Source};

/// Presentation time can lag the render callback by several buffers. Keep enough history to
/// interpolate only across samples the backend has scheduled, including the final audio tail.
const BUFFER_HISTORY: usize = 64;
const UNSET: u64 = u64::MAX;

thread_local! {
    static OUTPUT_FRAME: Cell<u64> = const { Cell::new(UNSET) };
}

#[derive(Clone)]
pub(super) struct Clock(Arc<Output>);

struct Output {
    buffers: [Slot; BUFFER_HISTORY],
    newest: AtomicU64,
    rate: SampleRate,
}

#[derive(Clone, Copy)]
struct Buffer {
    first: u64,
    frames: u64,
    at: f64,
}

#[derive(Default)]
struct Slot {
    sequence: AtomicU64,
    first: AtomicU64,
    frames: AtomicU64,
    at: AtomicU64,
}

impl Slot {
    fn publish(&self, sequence: u64, buffer: Buffer) {
        // Invalidate the old entry before replacing it. The callback must never wait on a
        // render thread that was preempted while reading the history.
        self.sequence.store(0, Ordering::SeqCst);
        self.first.store(buffer.first, Ordering::SeqCst);
        self.frames.store(buffer.frames, Ordering::SeqCst);
        self.at.store(buffer.at.to_bits(), Ordering::SeqCst);
        self.sequence.store(sequence, Ordering::SeqCst);
    }

    fn read(&self, sequence: u64) -> Option<Buffer> {
        if self.sequence.load(Ordering::SeqCst) != sequence {
            return None;
        }
        let buffer = Buffer {
            first: self.first.load(Ordering::SeqCst),
            frames: self.frames.load(Ordering::SeqCst),
            at: f64::from_bits(self.at.load(Ordering::SeqCst)),
        };
        (self.sequence.load(Ordering::SeqCst) == sequence).then_some(buffer)
    }
}

impl Clock {
    pub(super) fn new(rate: SampleRate) -> Self {
        Self(Arc::new(Output {
            buffers: std::array::from_fn(|_| Slot::default()),
            newest: AtomicU64::new(0),
            rate,
        }))
    }

    fn slot(&self, sequence: u64) -> &Slot {
        &self.0.buffers[(sequence % BUFFER_HISTORY as u64) as usize]
    }

    fn history(&self) -> impl Iterator<Item = Buffer> + '_ {
        let newest = self.0.newest.load(Ordering::Acquire);
        let oldest = newest.saturating_sub(BUFFER_HISTORY as u64);
        ((oldest + 1)..=newest)
            .rev()
            .filter_map(|sequence| self.slot(sequence).read(sequence))
    }

    pub(super) fn frame_at(&self, now: f64) -> Option<f64> {
        let buffer = self.history().find(|buffer| buffer.at <= now)?;
        let frames = (now - buffer.at) * f64::from(self.0.rate.get());
        Some(buffer.first as f64 + frames.min(buffer.frames as f64))
    }

    fn time_of(&self, frame: u64) -> Option<f64> {
        let buffer = self.history()
            .find(|buffer| frame >= buffer.first && frame - buffer.first < buffer.frames)?;
        Some(buffer.at + (frame - buffer.first) as f64 / f64::from(self.0.rate.get()))
    }

    fn publish(&self, buffer: Buffer) {
        // The stream's mixer callback is the only writer.
        let sequence = self.0.newest.load(Ordering::Relaxed) + 1;
        self.slot(sequence).publish(sequence, buffer);
        self.0.newest.store(sequence, Ordering::Release);
    }
}

/// Drives the regular mixer. The cursor identifies the output frame in which an individual
/// voice first contributed samples, independent of when its sink was created or polled.
pub(super) struct Render {
    source: MixerSource,
    clock: Clock,
    next_frame: u64,
    last_at: f64,
}

impl Render {
    pub(super) fn new(source: MixerSource, clock: Clock) -> Self {
        Self {
            source,
            clock,
            next_frame: 0,
            last_at: 0.0,
        }
    }

    pub(super) fn fill<T: cpal::Sample + cpal::FromSample<f32>>(
        &mut self,
        output: &mut [T],
        at: f64,
    ) {
        let first = self.next_frame;
        let channels = usize::from(self.source.channels().get());
        for frame in output.chunks_mut(channels) {
            OUTPUT_FRAME.set(self.next_frame);
            for sample in frame {
                *sample = self.source.next().map_or(T::EQUILIBRIUM, T::from_sample);
            }
            self.next_frame += 1;
        }
        OUTPUT_FRAME.set(UNSET);
        self.last_at = self.last_at.max(at);
        self.clock.publish(Buffer {
            first,
            frames: self.next_frame - first,
            at: self.last_at,
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Played {
    Waiting,
    Scheduled(f32),
    At(f32),
    Ended,
    Gone,
}

/// Identifies a one-shot's contribution to one stream, surviving its sink until the last
/// submitted samples have reached the listener.
#[derive(Component, Clone)]
pub struct VoiceClock(Arc<Voice>);

struct Voice {
    output: OnceLock<Clock>,
    first: AtomicU64,
    end: AtomicU64,
    speed: f32,
}

impl VoiceClock {
    pub fn new(speed: f32) -> Self {
        Self(Arc::new(Voice {
            output: OnceLock::new(),
            first: AtomicU64::new(UNSET),
            end: AtomicU64::new(UNSET),
            speed,
        }))
    }

    pub fn speed(&self) -> f32 {
        self.0.speed
    }

    pub(super) fn track<S: Source>(&self, output: Clock, source: S) -> Tracked<S> {
        let _ = self.0.output.set(output);
        Tracked {
            source,
            voice: self.clone(),
            started: false,
        }
    }

    pub(super) fn played(&self, clock: &Clock, now: f64, queued: bool) -> Played {
        let Some(output) = self.0.output.get() else {
            return if queued {
                Played::Waiting
            } else {
                Played::Gone
            };
        };
        if !Arc::ptr_eq(&output.0, &clock.0) {
            return Played::Gone;
        }

        let first = self.0.first.load(Ordering::Acquire);
        let end = self.0.end.load(Ordering::Acquire);
        let frame = clock.frame_at(now);
        let Some(frame) = frame.filter(|&frame| first != UNSET && frame > first as f64) else {
            if first != UNSET && let Some(start) = clock.time_of(first) {
                return Played::Scheduled(((now - start).min(0.0) as f32) * self.speed());
            }
            return if queued || first != UNSET { Played::Waiting } else { Played::Gone };
        };
        if end != UNSET && frame >= end as f64 {
            return Played::Ended;
        }
        Played::At(((frame - first as f64) / f64::from(clock.0.rate.get())) as f32 * self.speed())
    }
}

pub(super) struct Tracked<S> {
    source: S,
    voice: VoiceClock,
    started: bool,
}

impl<S: Source> Iterator for Tracked<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sample = self.source.next()?;
        if !self.started {
            let frame = OUTPUT_FRAME.get();
            if frame != UNSET {
                self.voice.0.first.store(frame, Ordering::Release);
                self.started = true;
            }
        }
        Some(sample)
    }
}

impl<S> Drop for Tracked<S> {
    fn drop(&mut self) {
        let frame = OUTPUT_FRAME.get();
        if self.started && frame != UNSET {
            self.voice.0.end.store(frame, Ordering::Release);
        }
    }
}

impl<S: Source> Source for Tracked<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.source.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.source.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.source.sample_rate()
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        self.source.total_duration()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZero;

    #[test]
    fn a_voice_starts_and_ends_at_the_buffers_presentation_times() {
        let rate = NonZero::new(1000).unwrap();
        let channels = NonZero::new(1).unwrap();
        let (input, source) = rodio::mixer::mixer(channels, rate);
        let clock = Clock::new(rate);
        let mut render = Render::new(source, clock.clone());
        let first = VoiceClock::new(1.0);
        input.add(first.track(
            clock.clone(),
            rodio::buffer::SamplesBuffer::new(channels, rate, vec![0.25; 150]),
        ));
        let heard = |voice: &VoiceClock, now| voice.played(&clock, now, true);

        let mut buffer = [0.0; 100];
        render.fill(&mut buffer, 0.25);
        assert_eq!(
            buffer, [0.25; 100],
            "the voice contributes from the buffer's first sample"
        );
        assert!(matches!(heard(&first, 0.249), Played::Scheduled(at) if (at + 0.001).abs() < 1e-5));
        assert_eq!(heard(&first, 0.275), Played::At(0.025));
        assert_eq!(
            heard(&first, 0.75),
            Played::At(0.1),
            "a stalled output holds the last sample"
        );

        let next = VoiceClock::new(1.0);
        input.add(next.track(
            clock.clone(),
            rodio::buffer::SamplesBuffer::new(channels, rate, vec![0.5; 100]),
        ));
        render.fill(&mut buffer, 0.8);
        assert_eq!(
            &buffer[..50],
            &[0.75; 50],
            "the voices mix during the handover"
        );
        assert_eq!(
            &buffer[50..],
            &[0.5; 50],
            "only the new voice remains after the old one ends"
        );
        assert!(matches!(heard(&next, 0.799), Played::Scheduled(at) if (at + 0.001).abs() < 1e-5));
        assert_eq!(heard(&first, 0.82), Played::At(0.12));
        assert_eq!(heard(&next, 0.82), Played::At(0.02));
        assert_eq!(
            heard(&first, 0.851),
            Played::Ended,
            "the decoder tail cleared after playback"
        );
    }
}
