//! The sound effects, made from sine waves, noise and envelopes when the
//! client starts. No recorded sound is used.

use std::f32::consts::TAU;

/// Samples per second of every effect.
pub const RATE: u32 = 44_100;

/// One sound effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sfx {
    /// One's own hero lets an attack go.
    Attack,
    /// One's own hero's attack lands.
    Hit,
    /// One's own hero lands the killing blow on a creep: a coin.
    LastHit,
    /// One's own hero denies an allied creep.
    Deny,
    /// A raze goes off.
    Raze,
    /// A requiem goes off.
    Requiem,
    /// One's own hero is low on health: a heartbeat.
    LowHp,
    /// An item bought.
    Buy,
    /// One's own hero gains a level.
    LevelUp,
    /// A tower strikes one's own hero.
    TowerShot,
    /// A hero falls.
    HeroDeath,
    /// One's own hero brings an enemy hero down.
    HeroKill,
    /// An order turned down.
    Refused,
    /// A click on the interface.
    Click,
}

/// Every effect, in the order [`Sfx::index`] counts them.
pub const ALL_SFX: [Sfx; 14] = [
    Sfx::Attack,
    Sfx::Hit,
    Sfx::LastHit,
    Sfx::Deny,
    Sfx::Raze,
    Sfx::Requiem,
    Sfx::LowHp,
    Sfx::Buy,
    Sfx::LevelUp,
    Sfx::TowerShot,
    Sfx::HeroDeath,
    Sfx::HeroKill,
    Sfx::Refused,
    Sfx::Click,
];

impl Sfx {
    /// The name a cached replacement of this effect is filed under.
    pub fn name(self) -> &'static str {
        match self {
            Sfx::Attack => "attack",
            Sfx::Hit => "hit",
            Sfx::LastHit => "last_hit",
            Sfx::Deny => "deny",
            Sfx::Raze => "raze",
            Sfx::Requiem => "requiem",
            Sfx::LowHp => "low_hp",
            Sfx::Buy => "buy",
            Sfx::LevelUp => "level_up",
            Sfx::TowerShot => "tower_shot",
            Sfx::HeroDeath => "hero_death",
            Sfx::HeroKill => "hero_kill",
            Sfx::Refused => "refused",
            Sfx::Click => "click",
        }
    }

    /// Where this effect sits in [`ALL_SFX`].
    pub fn index(self) -> usize {
        ALL_SFX
            .iter()
            .position(|sfx| *sfx == self)
            .expect("every effect is listed")
    }
}

/// The samples of one effect, mono, in `-1..=1`.
pub fn synth(sfx: Sfx) -> Vec<f32> {
    let (mut wave, peak) = match sfx {
        Sfx::Attack => (attack(), 0.30),
        Sfx::Hit => (hit(), 0.42),
        Sfx::LastHit => (coin(), 0.50),
        Sfx::Deny => (deny(), 0.42),
        Sfx::Raze => (raze(), 0.70),
        Sfx::Requiem => (requiem(), 0.75),
        Sfx::LowHp => (heartbeat(), 0.55),
        Sfx::Buy => (buy(), 0.40),
        Sfx::LevelUp => (level_up(), 0.40),
        Sfx::TowerShot => (tower_shot(), 0.42),
        Sfx::HeroDeath => (hero_death(), 0.50),
        Sfx::HeroKill => (hero_kill(), 0.45),
        Sfx::Refused => (refused(), 0.28),
        Sfx::Click => (click(), 0.22),
    };
    normalize(&mut wave, peak);
    wave
}

/// A fixed-seed noise source, the same every run.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// A one-pole low-pass filter.
struct LowPass(f32);

impl LowPass {
    fn step(&mut self, x: f32, cutoff: f32) -> f32 {
        let k = 1.0 - (-TAU * cutoff / RATE as f32).exp();
        self.0 += (x - self.0) * k;
        self.0
    }
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

/// A sine whose pitch glides exponentially from `f0` to `f1` over `glide`.
fn glide(wave: &mut [f32], f0: f32, f1: f32, glide: f32, tau: f32, gain: f32) {
    let mut phase = 0.0f32;
    for (i, s) in wave.iter_mut().enumerate() {
        let t = at(i);
        let k = (t / glide).min(1.0);
        let f = f0 * (f1 / f0).powf(k);
        phase += TAU * f / RATE as f32;
        *s += gain * phase.sin() * env(t, 0.003, tau);
    }
}

fn attack() -> Vec<f32> {
    let mut wave = vec![0.0; len(0.14)];
    let (mut noise, mut lp) = (Noise(0x9e37_79b9), LowPass(0.0));
    let mut phase = 0.0f32;
    for (i, s) in wave.iter_mut().enumerate() {
        let t = at(i);
        let f = 1500.0 * (0.35f32).powf(t / 0.12);
        phase += TAU * f / RATE as f32;
        let air = lp.step(noise.next(), 2500.0 - 12000.0 * t);
        *s = (0.55 * air + 0.45 * phase.sin()) * env(t, 0.006, 0.035);
    }
    wave
}

fn hit() -> Vec<f32> {
    let mut wave = vec![0.0; len(0.16)];
    glide(&mut wave, 190.0, 70.0, 0.07, 0.045, 1.0);
    let mut noise = Noise(0x1234_5678);
    for (i, s) in wave.iter_mut().enumerate().take(len(0.006)) {
        *s += 0.6 * noise.next() * (1.0 - at(i) / 0.006);
    }
    wave
}

fn coin() -> Vec<f32> {
    let mut wave = vec![0.0; len(0.75)];
    bell(&mut wave, 0.0, 1568.0, 0.12, 0.8);
    bell(&mut wave, 0.075, 2093.0, 0.30, 1.0);
    wave
}

fn deny() -> Vec<f32> {
    let mut wave = vec![0.0; len(0.22)];
    let mut phase = 0.0f32;
    for (i, s) in wave.iter_mut().enumerate() {
        let t = at(i);
        let f = 300.0 * (0.6f32).powf(t / 0.15);
        phase += TAU * f / RATE as f32;
        let tone = phase.sin() + 0.33 * (3.0 * phase).sin() + 0.2 * (5.0 * phase).sin();
        *s = tone * env(t, 0.003, 0.05);
    }
    let mut noise = Noise(0xdead_beef);
    for (i, s) in wave.iter_mut().enumerate().take(len(0.01)) {
        *s += 0.5 * noise.next() * (1.0 - at(i) / 0.01);
    }
    wave
}

fn raze() -> Vec<f32> {
    let mut wave = vec![0.0; len(0.75)];
    let (mut noise, mut lp, mut lp2) = (Noise(0x0bad_cafe), LowPass(0.0), LowPass(0.0));
    for (i, s) in wave.iter_mut().enumerate() {
        let t = at(i);
        let cutoff = 150.0 + 4000.0 * (-t / 0.12).exp();
        let roar = lp2.step(lp.step(noise.next(), cutoff), cutoff * 1.5);
        *s = 1.6 * roar * env(t, 0.008, 0.16);
        // Sparse crackle of embers.
        if noise.next() > 0.996 {
            *s += 0.35 * noise.next() * (-t / 0.3).exp();
        }
    }
    glide(&mut wave, 95.0, 38.0, 0.25, 0.22, 0.9);
    wave
}

fn requiem() -> Vec<f32> {
    let mut wave = vec![0.0; len(1.6)];
    glide(&mut wave, 70.0, 32.0, 0.6, 0.5, 1.0);
    let (mut noise, mut lp) = (Noise(0x5eed_0001), LowPass(0.0));
    for (i, s) in wave.iter_mut().enumerate() {
        let t = at(i);
        *s += 0.9 * lp.step(noise.next(), 200.0 + 2500.0 * (-t / 0.25).exp()) * env(t, 0.01, 0.35);
        // A dark, beating chord under the blast.
        let tremolo = 0.75 + 0.25 * (TAU * 7.0 * t).sin();
        let mut chord = 0.0;
        for f in [110.0f32, 116.54, 164.81, 220.0] {
            let p = (f * t).fract();
            chord += (2.0 * p - 1.0) * 0.25;
        }
        *s += 0.35 * chord * tremolo * env(t, 0.05, 0.55);
    }
    wave
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

fn buy() -> Vec<f32> {
    let mut wave = vec![0.0; len(0.45)];
    let (mut noise, mut lp) = (Noise(0x00c0_ffee), LowPass(0.0));
    for (i, s) in wave.iter_mut().enumerate().take(len(0.02)) {
        let x = noise.next();
        *s += 0.7 * (x - lp.step(x, 3000.0)) * (1.0 - at(i) / 0.02);
    }
    bell(&mut wave, 0.03, 2637.0, 0.10, 0.7);
    bell(&mut wave, 0.09, 3136.0, 0.20, 0.8);
    wave
}

fn level_up() -> Vec<f32> {
    let mut wave = vec![0.0; len(0.8)];
    for (n, f) in [523.25f32, 659.25, 783.99, 1046.5].into_iter().enumerate() {
        let start = n as f32 * 0.07;
        for (i, s) in wave.iter_mut().enumerate() {
            let t = at(i) - start;
            if t >= 0.0 {
                let p = (f * t).fract();
                let tri = 4.0 * (p - 0.5).abs() - 1.0;
                *s += 0.5 * tri * env(t, 0.004, 0.22);
            }
        }
    }
    bell(&mut wave, 0.28, 2093.0, 0.25, 0.3);
    wave
}

fn tower_shot() -> Vec<f32> {
    let mut wave = vec![0.0; len(0.25)];
    let mut noise = Noise(0x7077_e400);
    let mut phase = 0.0f32;
    for (i, s) in wave.iter_mut().enumerate() {
        let t = at(i);
        let f = 1100.0 * (0.2f32).powf((t / 0.18).min(1.0));
        phase += f / RATE as f32;
        let saw = 2.0 * phase.fract() - 1.0;
        *s = (0.7 * saw + 0.3 * noise.next()) * env(t, 0.002, 0.06);
    }
    wave
}

fn hero_death() -> Vec<f32> {
    let mut wave = vec![0.0; len(1.0)];
    let mut phase = 0.0f32;
    for (i, s) in wave.iter_mut().enumerate() {
        let t = at(i);
        let f = 392.0 * (0.5f32).powf((t / 0.7).min(1.0)) * (1.0 + 0.012 * (TAU * 6.0 * t).sin());
        phase += f / RATE as f32;
        let tri = 4.0 * (phase.fract() - 0.5).abs() - 1.0;
        *s = 0.6 * tri * env(t, 0.01, 0.35);
    }
    glide(&mut wave, 90.0, 45.0, 0.2, 0.15, 0.8);
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

fn refused() -> Vec<f32> {
    let mut wave = vec![0.0; len(0.17)];
    for (i, s) in wave.iter_mut().enumerate() {
        let t = at(i);
        let on = t < 0.06 || (0.09..0.15).contains(&t);
        if on {
            let p = (175.0 * t).fract();
            *s = if p < 0.5 { 0.6 } else { -0.6 };
        }
    }
    let mut lp = LowPass(0.0);
    for s in wave.iter_mut() {
        *s = lp.step(*s, 1800.0);
    }
    wave
}

fn click() -> Vec<f32> {
    let mut wave = vec![0.0; len(0.02)];
    let mut noise = Noise(0x0c11_c4ed);
    for (i, s) in wave.iter_mut().enumerate() {
        *s = noise.next() * env(at(i), 0.0005, 0.003);
    }
    wave
}
