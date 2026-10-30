//! A face as Dota moves one, out of the asset cache: flex controllers set by a facial scene
//! (a hero's idle expression) and by the line being spoken (its baked morph curves and its
//! phonemes, through Valve's phoneme table), then worked into each mesh's morph target weights
//! by the mesh's flex rules.
//!
//! The order is Source's: a scene's flex animation events lay their tracks over the controllers
//! by their ramp's weight; the line's morph curves are laid over them the same way; its
//! phonemes are added on top (`C_BaseFlex::AddVisemesForSentence`: a box filter of
//! [`PHONEME_FILTER`] seconds over the phonemes, each adding its table row); the sums are held
//! within each controller's range and the rules turn them into flex weights.
//!
//! Any program of the user's may write the cache: a file that does not read is no face, lists
//! are capped, numbers must be finite. Plain types only: the fuzz target takes this file as it is.

use std::collections::HashMap;

/// Seconds of the box filter over the phonemes (Source's `phonemefilter`).
pub const PHONEME_FILTER: f32 = 0.08;
/// Most lines of a face file read.
const MAX_LINES: usize = 1 << 15;
/// Most numbers on one line.
const MAX_NUMBERS: usize = 1 << 15;
/// Most controllers, flexes, rules, operations and targets of a mesh.
const MAX_FLEX: usize = 256;

/// Source 2's string token of a name, which a line's morph curves are keyed by.
pub use heatel::sound::token;

fn number(word: &str) -> Option<f32> {
    word.parse::<f32>().ok().filter(|n| n.is_finite())
}

/// The words of each line after a header line `header`, at most [`MAX_LINES`] lines.
fn body<'a>(text: &'a str, header: &str) -> Option<impl Iterator<Item = Vec<&'a str>>> {
    let mut lines = text.lines();
    (lines.next()? == header).then(|| {
        lines
            .take(MAX_LINES)
            .map(|l| l.split_whitespace().take(MAX_NUMBERS + 8).collect())
    })
}

/// Controller values by token, in each controller's own units.
pub type Controls = HashMap<u32, f32>;

/// Lays `value` over a controller by `weight` (Source's `orig * (1 - w) + value * w`).
fn lay(controls: &mut Controls, name: u32, value: f32, weight: f32) {
    let slot = controls.entry(name).or_insert(0.0);
    *slot = *slot * (1.0 - weight) + value * weight;
}

/// Valve's phoneme table: each phoneme's setting of each key controller.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Phonemes {
    keys: Vec<u32>,
    rows: HashMap<u32, Vec<f32>>,
}

impl Phonemes {
    /// `expressions/heroes/phonemes.txt` as the importer writes it.
    pub fn parse(text: &str) -> Option<Self> {
        let mut table = Self::default();
        for words in body(text, "dbota-phonemes 1")? {
            match words.as_slice() {
                ["keys", keys @ ..] if keys.len() <= MAX_FLEX => {
                    table.keys = keys.iter().map(|k| token(k)).collect();
                }
                ["phoneme", code, settings @ ..] if settings.len() == table.keys.len() => {
                    let row = settings.iter().map(|s| number(s)).collect::<Option<_>>()?;
                    table.rows.insert(code.parse().ok()?, row);
                }
                [] => {}
                _ => return None,
            }
        }
        (!table.keys.is_empty()).then_some(table)
    }

    /// Adds `amount` of a phoneme's settings.
    fn add(&self, code: u32, amount: f32, controls: &mut Controls) {
        let Some(row) = self.rows.get(&code) else {
            return;
        };
        for (&key, &setting) in self.keys.iter().zip(row) {
            if setting != 0.0 {
                *controls.entry(key).or_insert(0.0) += amount * setting;
            }
        }
    }
}

/// A line's baked controller curves (normalized to each controller's range).
#[derive(Clone, Debug, Default, PartialEq)]
struct Curves {
    names: Vec<u32>,
    times: Vec<f32>,
    values: Vec<Vec<f32>>,
    ease_in: f32,
    ease_out: f32,
}

/// What a voice line's face does: its phonemes, emphasis and morph curves.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lipsync {
    phonemes: Vec<(f32, f32, u32)>,
    emphasis: Vec<(f32, f32)>,
    curves: Option<Curves>,
}

impl Lipsync {
    /// A line's `.lipsync.txt` as the importer writes it.
    pub fn parse(text: &str) -> Option<Self> {
        let mut out = Self::default();
        for words in body(text, "dbota-lipsync 1")? {
            match words.as_slice() {
                ["phoneme", start, end, code] => {
                    let (start, end) = (number(start)?, number(end)?);
                    (start <= end).then_some(())?;
                    out.phonemes.push((start, end, code.parse().ok()?));
                }
                ["emphasis", t, v] => out.emphasis.push((number(t)?, number(v)?)),
                ["morph", ease_in, ease_out, names @ ..] if names.len() <= MAX_FLEX => {
                    out.curves = Some(Curves {
                        names: names
                            .iter()
                            .map(|n| n.parse().ok())
                            .collect::<Option<_>>()?,
                        ease_in: number(ease_in)?.max(0.0),
                        ease_out: number(ease_out)?.max(0.0),
                        ..Curves::default()
                    });
                }
                ["key", t, values @ ..] => {
                    let curves = out.curves.as_mut()?;
                    let t = number(t)?;
                    let fits = values.len() == curves.names.len()
                        && curves.times.last().is_none_or(|&last| last <= t);
                    fits.then_some(())?;
                    curves.times.push(t);
                    curves
                        .values
                        .push(values.iter().map(|v| number(v)).collect::<Option<_>>()?);
                }
                [] => {}
                _ => return None,
            }
        }
        Some(out)
    }

    /// Seconds into the line after which its face does nothing more (the phonemes' filter looks
    /// ahead of the time, never back).
    pub fn end(&self) -> f32 {
        let phonemes = self.phonemes.iter().map(|p| p.1);
        let curves = self.curves.iter().filter_map(|c| c.times.last().copied());
        phonemes.chain(curves).fold(0.0, f32::max)
    }

    /// Lays the line's face `t` seconds into it over `controls`: its curves (denormalized by
    /// `ranges`, a controller no mesh has being skipped), then its phonemes.
    pub fn apply(
        &self,
        t: f32,
        table: &Phonemes,
        ranges: &HashMap<u32, (f32, f32)>,
        controls: &mut Controls,
    ) {
        if let Some(c) = &self.curves {
            c.apply(t, ranges, controls);
        }
        self.visemes(t, table, controls);
    }

    /// Source's `AddVisemesForSentence` with its `phonemefilter` box filter: each phoneme adds
    /// the part of `[t, t + dt]` it covers. Inside a phoneme, the filter widens up to the next
    /// phoneme (its end when they touch), never past the phoneme's own length.
    fn visemes(&self, t: f32, table: &Phonemes, controls: &mut Controls) {
        let amount = emphasis_amount(intensity(&self.emphasis, t));
        let mut dt = PHONEME_FILTER;
        let n = self.phonemes.len();
        for (k, &(start, end, code)) in self.phonemes.iter().enumerate() {
            if t > start && t < end && k + 1 < n {
                let (next_start, next_end, _) = self.phonemes[k + 1];
                let until = if next_start == end {
                    next_end
                } else {
                    next_start
                };
                dt = dt.max((until - t).min(end - start));
            }
            let t1 = ((start - t) / dt).max(0.0);
            let t2 = ((end - t) / dt).min(1.0);
            if t1 < 1.0 && t2 > 0.0 {
                table.add(code, amount * (t2 - t1), controls);
            }
        }
    }
}

/// The emphasis at `t`: the samples' line, 0.5 (plain speech) without any.
fn intensity(samples: &[(f32, f32)], t: f32) -> f32 {
    match samples.iter().position(|&(at, _)| at > t) {
        None => samples.last().map_or(0.5, |s| s.1),
        Some(0) => samples[0].1,
        Some(i) => {
            let ((t0, v0), (t1, v1)) = (samples[i - 1], samples[i]);
            let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
            v0 + (v1 - v0) * f
        }
    }
}

/// How much of the phoneme table an emphasis says (`C_BaseFlex::ComputeBlendedSetting` with
/// only the normal table, which is all Dota ships): 1 for plain speech, between 0.8 and 1.2
/// outside 0.4 to 0.6.
fn emphasis_amount(e: f32) -> f32 {
    if (0.4..=0.6).contains(&e) {
        1.0
    } else {
        2.0 * e.clamp(0.4, 0.6)
    }
}

impl Curves {
    fn apply(&self, t: f32, ranges: &HashMap<u32, (f32, f32)>, controls: &mut Controls) {
        let (Some(&first), Some(&last)) = (self.times.first(), self.times.last()) else {
            return;
        };
        if t < first || t > last {
            return;
        }
        let ease = |span: f32, into: f32| {
            if span > 0.0 {
                (into / span).clamp(0.0, 1.0)
            } else {
                1.0
            }
        };
        let weight = ease(self.ease_in, t - first) * ease(self.ease_out, last - t);
        let i = self
            .times
            .partition_point(|&k| k <= t)
            .clamp(1, self.times.len());
        let (a, b) = (i - 1, i.min(self.times.len() - 1));
        let span = self.times[b] - self.times[a];
        let f = if span > 0.0 {
            (t - self.times[a]) / span
        } else {
            0.0
        };
        for (c, &name) in self.names.iter().enumerate() {
            let Some(&(min, max)) = ranges.get(&name) else {
                continue;
            };
            let v = self.values[a][c] + (self.values[b][c] - self.values[a][c]) * f;
            lay(
                controls,
                name,
                min + v.clamp(0.0, 1.0) * (max - min),
                weight,
            );
        }
    }
}

/// A flex controller track of a scene: its controller, range and samples (seconds into its
/// event, values normalized to the range).
#[derive(Clone, Debug, PartialEq)]
struct Track {
    name: u32,
    min: f32,
    max: f32,
    keys: Vec<(f32, f32)>,
}

impl Track {
    /// Its value `t` seconds into the event: Source's default sample curve, Catmull-Rom with
    /// its tangents scaled to the samples' spacing (`Catmull_Rom_Spline_NormalizeX`).
    fn value(&self, t: f32) -> f32 {
        let keys = &self.keys;
        let Some(&(last_t, last_v)) = keys.last() else {
            return 0.0;
        };
        let v = if t <= keys[0].0 {
            keys[0].1
        } else if t >= last_t {
            last_v
        } else {
            let i = keys
                .partition_point(|&(k, _)| k <= t)
                .clamp(1, keys.len() - 1);
            let (p2, p3) = (keys[i - 1], keys[i]);
            let p1 = keys[i.saturating_sub(2)];
            let p4 = keys[(i + 1).min(keys.len() - 1)];
            catmull_rom_normalize_x(p1, p2, p3, p4, t)
        };
        self.min + v.clamp(0.0, 1.0) * (self.max - self.min)
    }
}

fn catmull_rom_normalize_x(
    p1: (f32, f32),
    p2: (f32, f32),
    p3: (f32, f32),
    p4: (f32, f32),
    t: f32,
) -> f32 {
    let dt = p3.0 - p2.0;
    if dt <= 0.0 {
        return p2.1;
    }
    let lerp = |a: (f32, f32), b: (f32, f32), f: f32| a.1 + (b.1 - a.1) * f;
    let y1 = if p1.0 != p2.0 {
        lerp(p2, p1, dt / (p2.0 - p1.0))
    } else {
        p1.1
    };
    let y4 = if p4.0 != p3.0 {
        lerp(p3, p4, dt / (p4.0 - p3.0))
    } else {
        p4.1
    };
    let (y2, y3) = (p2.1, p3.1);
    let s = (t - p2.0) / dt;
    0.5 * (2.0 * y2
        + (y3 - y1) * s
        + (2.0 * y1 - 5.0 * y2 + 4.0 * y3 - y4) * s * s
        + (3.0 * y2 - y1 - 3.0 * y3 + y4) * s * s * s)
}

/// A scene's flex animation event: its stretch, its ramp (`x y slope_in slope_out` keys) and
/// its tracks.
#[derive(Clone, Debug, Default, PartialEq)]
struct Event {
    start: f32,
    end: f32,
    ramp: Vec<[f32; 4]>,
    tracks: Vec<Track>,
}

impl Event {
    /// Its weight `t` seconds in: its ramp, a Hermite curve through its keys with their slopes;
    /// 1 without one.
    fn weight(&self, t: f32) -> f32 {
        let ramp = &self.ramp;
        let Some(last) = ramp.last() else {
            return 1.0;
        };
        let y = if t <= ramp[0][0] {
            ramp[0][1]
        } else if t >= last[0] {
            last[1]
        } else {
            let i = ramp.partition_point(|k| k[0] <= t).clamp(1, ramp.len() - 1);
            let ([x0, y0, _, m0], [x1, y1, m1, _]) = (ramp[i - 1], ramp[i]);
            let h = x1 - x0;
            let s = if h > 0.0 { (t - x0) / h } else { 0.0 };
            let (s2, s3) = (s * s, s * s * s);
            (2.0 * s3 - 3.0 * s2 + 1.0) * y0
                + (s3 - 2.0 * s2 + s) * h * m0
                + (-2.0 * s3 + 3.0 * s2) * y1
                + (s3 - s2) * h * m1
        };
        y.clamp(0.0, 1.0)
    }
}

/// A facial scene: its loop and its flex animation events.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Expression {
    looping: Option<(f32, f32)>,
    events: Vec<Event>,
}

impl Expression {
    /// A scene's `.txt` as the importer writes it.
    pub fn parse(text: &str) -> Option<Self> {
        let mut out = Self::default();
        for words in body(text, "dbota-expression 1")? {
            match words.as_slice() {
                ["loop", from, to] => {
                    let (from, to) = (number(from)?, number(to)?);
                    out.looping = (from < to).then_some((from, to));
                }
                ["event", start, end] => out.events.push(Event {
                    start: number(start)?,
                    end: number(end)?,
                    ..Event::default()
                }),
                ["ramp", keys @ ..] if keys.len() % 4 == 0 => {
                    let numbers: Vec<f32> =
                        keys.iter().map(|k| number(k)).collect::<Option<_>>()?;
                    out.events.last_mut()?.ramp = numbers.as_chunks::<4>().0.to_vec();
                }
                ["track", name, min, max, keys @ ..] if keys.len() % 2 == 0 => {
                    let numbers: Vec<f32> =
                        keys.iter().map(|k| number(k)).collect::<Option<_>>()?;
                    let keys: Vec<(f32, f32)> = numbers
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|k| (k[0], k[1]))
                        .collect();
                    let event = out.events.last_mut()?;
                    (event.tracks.len() < MAX_FLEX).then_some(())?;
                    event.tracks.push(Track {
                        name: token(name),
                        min: number(min)?,
                        max: number(max)?,
                        keys,
                    });
                }
                [] => {}
                _ => return None,
            }
        }
        Some(out)
    }

    /// Lays the scene `t` seconds after it began over `controls` (looped as it loops).
    pub fn apply(&self, t: f32, controls: &mut Controls) {
        let t = match self.looping {
            Some((from, to)) if t >= to => from + (t - from).rem_euclid(to - from),
            _ => t,
        };
        for event in &self.events {
            if t < event.start || t > event.end {
                continue;
            }
            let into = t - event.start;
            let weight = event.weight(into);
            for track in &event.tracks {
                lay(controls, track.name, track.value(into), weight);
            }
        }
    }
}

/// One step of a flex rule (see the importer's `face::Op`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Const(f32),
    Fetch1(usize),
    Fetch2(usize),
    Add,
    Sub,
    Mul,
    Div,
    Neg,
    Max,
    Min,
}

/// How a mesh's morph target weights follow the controllers: its controllers (token and
/// range), its rules over its flexes, and the flex of each morph target.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rig {
    pub controllers: Vec<(u32, f32, f32)>,
    /// The morph targets' names (the flexes they are), for a trace.
    pub names: Vec<String>,
    rules: Vec<(usize, Vec<Op>)>,
    flexes: usize,
    targets: Vec<usize>,
}

impl Rig {
    /// A mesh's `flex` extras as the importer writes them (see its `face_extras`).
    pub fn from_extras(extras: &str) -> Option<Self> {
        let json: serde_json::Value = serde_json::from_str(extras).ok()?;
        let flex = json.get("flex")?;
        let list = |key: &str| {
            flex.get(key)
                .and_then(serde_json::Value::as_array)
                .filter(|l| l.len() <= MAX_FLEX)
        };
        let flexes = list("descs")?.len();
        let index =
            |v: &serde_json::Value, of: usize| v.as_u64().map(|i| i as usize).filter(|&i| i < of);
        let controllers: Vec<(u32, f32, f32)> = list("controllers")?
            .iter()
            .map(|c| {
                let name = c.get(0)?.as_str()?;
                let (min, max) = (c.get(1)?.as_f64()? as f32, c.get(2)?.as_f64()? as f32);
                (min.is_finite() && max.is_finite() && min <= max).then(|| (token(name), min, max))
            })
            .collect::<Option<_>>()?;
        let rules = list("rules")?
            .iter()
            .map(|r| {
                let flex = index(r.get(0)?, flexes)?;
                let ops = r.get(1)?.as_array().filter(|o| o.len() <= MAX_FLEX)?;
                let ops = ops
                    .iter()
                    .map(|o| op(o, flexes, controllers.len()))
                    .collect::<Option<Vec<_>>>()?;
                balanced(&ops).then_some((flex, ops))
            })
            .collect::<Option<_>>()?;
        let targets: Vec<usize> = list("targets")?
            .iter()
            .map(|t| index(t, flexes))
            .collect::<Option<_>>()?;
        let names = json
            .get("targetNames")
            .and_then(serde_json::Value::as_array)
            .filter(|l| l.len() == targets.len())
            .map(|l| {
                l.iter()
                    .map(|n| n.as_str().unwrap_or_default().to_owned())
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            controllers,
            names,
            rules,
            flexes,
            targets,
        })
    }

    /// The weight of each morph target for `controls`, each controller held within its range.
    pub fn weights(&self, controls: &Controls, out: &mut [f32]) {
        let src: Vec<f32> = self
            .controllers
            .iter()
            .map(|&(name, min, max)| controls.get(&name).copied().unwrap_or(0.0).clamp(min, max))
            .collect();
        let mut dest = vec![0.0; self.flexes];
        let mut stack = Vec::with_capacity(8);
        for (flex, ops) in &self.rules {
            stack.clear();
            for op in ops {
                run(*op, &src, &dest, &mut stack);
            }
            dest[*flex] = stack.first().copied().unwrap_or(0.0);
        }
        for (slot, &flex) in out.iter_mut().zip(&self.targets) {
            let w = dest[flex];
            *slot = if w.is_finite() {
                w.clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
    }
}

/// One operation of Source's flex rule machine (`RunFlexRules`).
fn run(op: Op, src: &[f32], dest: &[f32], stack: &mut Vec<f32>) {
    let binary = |stack: &mut Vec<f32>, f: fn(f32, f32) -> f32| {
        if let (Some(b), Some(a)) = (stack.pop(), stack.pop()) {
            stack.push(f(a, b));
        }
    };
    match op {
        Op::Const(v) => stack.push(v),
        Op::Fetch1(i) => stack.push(src[i]),
        Op::Fetch2(i) => stack.push(dest[i]),
        Op::Add => binary(stack, |a, b| a + b),
        Op::Sub => binary(stack, |a, b| a - b),
        Op::Mul => binary(stack, |a, b| a * b),
        Op::Div => binary(stack, |a, b| if b > 0.0001 { a / b } else { 0.0 }),
        Op::Neg => {
            if let Some(v) = stack.last_mut() {
                *v = -*v;
            }
        }
        Op::Max => binary(stack, f32::max),
        Op::Min => binary(stack, f32::min),
    }
}

fn op(o: &serde_json::Value, flexes: usize, controllers: usize) -> Option<Op> {
    let name = o.get(0)?.as_str()?;
    let data = o.get(1);
    let slot = |of: usize| data?.as_u64().map(|i| i as usize).filter(|&i| i < of);
    Some(match name {
        "const" => Op::Const(Some(data?.as_f64()? as f32).filter(|v| v.is_finite())?),
        "fetch1" => Op::Fetch1(slot(controllers)?),
        "fetch2" => Op::Fetch2(slot(flexes)?),
        "add" => Op::Add,
        "sub" => Op::Sub,
        "mul" => Op::Mul,
        "div" => Op::Div,
        "neg" => Op::Neg,
        "max" => Op::Max,
        "min" => Op::Min,
        _ => return None,
    })
}

/// Whether a rule leaves one value and never pops an empty stack.
fn balanced(ops: &[Op]) -> bool {
    let mut depth: usize = 0;
    for op in ops {
        depth = match op {
            Op::Const(_) | Op::Fetch1(_) | Op::Fetch2(_) => depth + 1,
            Op::Neg if depth >= 1 => depth,
            Op::Neg => return false,
            _ if depth >= 2 => depth - 1,
            _ => return false,
        };
    }
    depth == 1
}
