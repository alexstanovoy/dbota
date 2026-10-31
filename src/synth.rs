//! The sound effects of the cues Dota has no recorded sound for (the low-health alarm, a hero
//! kill's chime), made from sine waves, noise and envelopes when the client starts.

use std::f32::consts::TAU;

/// Samples per second of every effect.
pub const RATE: u32 = 44_100;

/// One sound effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sfx {
    /// One's own hero is low on health: a heartbeat.
    LowHp,
    /// One's own hero brings an enemy hero down.
    HeroKill,
}

/// The samples of one effect, mono, in `-1..=1`.
pub fn synth(sfx: Sfx) -> Vec<f32> {
    let (mut wave, peak) = match sfx {
        Sfx::LowHp => (heartbeat(), 0.55),
        Sfx::HeroKill => (hero_kill(), 0.45),
    };
    normalize(&mut wave, peak);
    wave
}

/// Seconds to samples.
fn len(seconds: f32) -> usize {
    (seconds * RATE as f32) as usize
}

/// Seconds of sample `i`.
fn at(i: usize) -> f32 {
    i as f32 / RATE as f32
}

/// A quick rise and an exponential fall with time constant `tau`.
fn env(t: f32, rise: f32, tau: f32) -> f32 {
    if t < 0.0 {
        0.0
    } else if t < rise {
        t / rise
    } else {
        (-(t - rise) / tau).exp()
    }
}

/// Scales a wave to a peak and fades its last few milliseconds out.
fn normalize(wave: &mut [f32], peak: f32) {
    let top = wave.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if top > 0.0 {
        for s in wave.iter_mut() {
            *s *= peak / top;
        }
    }
    let fade = len(0.004).min(wave.len());
    let n = wave.len();
    for (k, s) in wave[n - fade..].iter_mut().enumerate() {
        *s *= 1.0 - k as f32 / fade as f32;
    }
}

/// A bell: partials over a fundamental, each fading on its own.
fn bell(wave: &mut [f32], start: f32, freq: f32, tau: f32, gain: f32) {
    const PARTIALS: [(f32, f32); 4] = [(1.0, 1.0), (2.0, 0.45), (3.01, 0.22), (4.2, 0.10)];
    for (i, s) in wave.iter_mut().enumerate() {
        let t = at(i) - start;
        if t < 0.0 {
            continue;
        }
        for (ratio, amp) in PARTIALS {
            *s += gain * amp * (TAU * freq * ratio * t).sin() * env(t, 0.002, tau / ratio.sqrt());
        }
    }
}

fn heartbeat() -> Vec<f32> {
    let mut wave = vec![0.0; len(0.5)];
    for (beat, gain) in [(0.0f32, 1.0f32), (0.17, 0.7)] {
        for (i, s) in wave.iter_mut().enumerate() {
            let t = at(i) - beat;
            if t >= 0.0 {
                let f = 65.0 - 20.0 * (t / 0.1).min(1.0);
                *s += gain * (TAU * f * t).sin() * env(t, 0.006, 0.05);
            }
        }
    }
    wave
}

fn hero_kill() -> Vec<f32> {
    let mut wave = vec![0.0; len(0.9)];
    for (n, f) in [523.25f32, 659.25, 783.99, 1046.5].into_iter().enumerate() {
        bell(&mut wave, n as f32 * 0.025, f, 0.35, 0.5);
    }
    bell(&mut wave, 0.16, 2093.0, 0.3, 0.5);
    wave
}
