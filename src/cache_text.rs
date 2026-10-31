//! The asset cache's small text files as the client reads them: where the map puts its trees
//! and buildings, its lighting by day and by night, its soundscapes, and the portrait framings. Any program of
//! the user's may write the cache, so a line that does not read whole, or holds a number that
//! is not finite, is no line; the lists are capped by their callers. Plain numbers only, no
//! engine types: the fuzz targets take this file as it is.

/// A tree the map places (`map/mid/trees.txt`): its model, Dota position and yaw in degrees.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeLine {
    pub model: String,
    pub x: f32,
    pub y: f32,
    pub yaw: f32,
    pub scale: f32,
}

/// A building the map places (`map/mid/buildings.txt`).
#[derive(Clone, Debug, PartialEq)]
pub struct BuildingLine {
    pub model: String,
    pub x: f32,
    pub y: f32,
    pub yaw: f32,
    /// The model's material group the map draws it with.
    pub skin: u8,
    /// The map's scale of it on each of Dota's axes (x, y, up).
    pub scale: [f32; 3],
}

/// Dota's lighting (`map/mid/lighting.txt`): for the sun and the ambient light, by day and
/// by night, the colour (0 to 255) and its scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lighting {
    pub sun: [[f32; 4]; 2],
    pub ambient: [[f32; 4]; 2],
    /// Seconds the light takes to turn, more than zero.
    pub turn: f32,
}

/// Numbers of a framing line before its background: camera position and angles, field of
/// view, far plane, light position, angles and field of view, light colour, ambient colour,
/// light scale and ambient scale.
pub const FRAMING_NUMBERS: usize = 23;

/// A line of `models/portraits.txt`: a model's framing, and its background's image in the
/// cache with the background's four corner tints (rgb each), when it has one.
#[derive(Clone, Debug, PartialEq)]
pub struct FramingLine {
    pub logical: String,
    pub numbers: [f32; FRAMING_NUMBERS],
    pub background: Option<(String, [f32; 12])>,
}

/// A finite number.
fn number(word: &str) -> Option<f32> {
    word.parse::<f32>().ok().filter(|n| n.is_finite())
}

/// `N` finite numbers.
fn numbers<const N: usize>(words: &[&str]) -> Option<[f32; N]> {
    let words: &[&str; N] = words.try_into().ok()?;
    let mut out = [0.0; N];
    for (slot, word) in out.iter_mut().zip(words) {
        *slot = number(word)?;
    }
    Some(out)
}

/// The lines of a list file that are not comments, at most `max`.
fn lines(text: &str, max: usize) -> impl Iterator<Item = Vec<&str>> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .take(max)
        .map(|line| line.split_whitespace().collect())
}

/// The trees of `trees.txt` (`model x y height yaw scale`), of its first `max` lines.
pub fn trees(text: &str, max: usize) -> Vec<TreeLine> {
    lines(text, max)
        .filter_map(|w| {
            // The height goes unread: the client stands trees on its own ground.
            let [model, x, y, _, yaw, scale] = w.as_slice() else {
                return None;
            };
            let [x, y, yaw, scale] = numbers::<4>(&[x, y, yaw, scale])?;
            Some(TreeLine {
                model: (*model).to_owned(),
                x,
                y,
                yaw,
                scale,
            })
        })
        .collect()
}

/// The buildings of `buildings.txt` (`model x y yaw skin scale_x scale_y scale_z`), of its
/// first `max` lines. A scale that is not a positive number reads as 1.
pub fn buildings(text: &str, max: usize) -> Vec<BuildingLine> {
    lines(text, max)
        .filter_map(|w| {
            let [model, x, y, yaw, skin, sx, sy, sz] = w.as_slice() else {
                return None;
            };
            let [x, y, yaw] = numbers::<3>(&[*x, *y, *yaw])?;
            let scale = numbers::<3>(&[*sx, *sy, *sz])?.map(|s| if s > 0.0 { s } else { 1.0 });
            Some(BuildingLine {
                model: (*model).to_owned(),
                x,
                y,
                yaw,
                skin: skin.parse().ok()?,
                scale,
            })
        })
        .collect()
}

/// `lighting.txt`: `sun|ambient day|night r g b scale` lines and `turn seconds`. `None` when a
/// colour line has a number that does not read or a value is missing.
pub fn lighting(text: &str) -> Option<Lighting> {
    let mut colors = [[None; 2]; 2];
    let mut turn = None;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = line.split_whitespace().collect();
        match w.as_slice() {
            [
                light @ ("sun" | "ambient"),
                time @ ("day" | "night"),
                v @ ..,
            ] if v.len() == 4 => {
                let light = usize::from(*light == "ambient");
                let time = usize::from(*time == "night");
                colors[light][time] = Some(numbers::<4>(v)?);
            }
            ["turn", seconds] => turn = number(seconds).filter(|t| *t > 0.0),
            _ => {}
        }
    }
    let pair = |light: [Option<[f32; 4]>; 2]| Some([light[0]?, light[1]?]);
    Some(Lighting {
        sun: pair(colors[0])?,
        ambient: pair(colors[1])?,
        turn: turn?,
    })
}

/// A line of `portraits.txt`: the logical name, [`FRAMING_NUMBERS`] numbers, then twelve
/// tints and the background's path (`-` for none); a cache of an older import has neither.
pub fn framing(line: &str) -> Option<FramingLine> {
    if line.starts_with('#') {
        return None;
    }
    let mut words = line.split_whitespace();
    let logical = words.next()?.to_owned();
    let words: Vec<&str> = words.collect();
    let numbers_part = words.get(..FRAMING_NUMBERS)?;
    let background = match &words[FRAMING_NUMBERS..] {
        [tints @ .., path] if tints.len() == 12 && *path != "-" => {
            Some(((*path).to_owned(), numbers::<12>(tints)?))
        }
        _ => None,
    };
    Some(FramingLine {
        logical,
        numbers: numbers::<FRAMING_NUMBERS>(numbers_part)?,
        background,
    })
}

/// A particle system the map places (`map/mid/ambient.txt`): its effect's file stem, where
/// (Dota coordinates) and how it is turned (Source's pitch, yaw and roll, degrees), whether it
/// shows by day, by night and in the fog, and the value of its data control point, if it has
/// one.
#[derive(Clone, Debug, PartialEq)]
pub struct AmbientLine {
    pub effect: String,
    pub at: [f32; 3],
    pub angles: [f32; 3],
    pub day: bool,
    pub night: bool,
    pub fog: bool,
    pub data: Option<(usize, [f32; 3])>,
}

/// Control points a data point may be, as Source numbers them.
pub const MAX_CONTROL_POINT: usize = 63;

/// A flag of `0` or `1`.
fn flag(word: &str) -> Option<bool> {
    match word {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

/// The map's particle systems of `ambient.txt` (`effect x y z pitch yaw roll day night fog
/// [cp<n> x y z]`), of its first `max` lines; an effect's name is a plain file stem.
pub fn ambient(text: &str, max: usize) -> Vec<AmbientLine> {
    lines(text, max)
        .filter_map(|w| {
            let [effect, rest @ ..] = w.as_slice() else {
                return None;
            };
            let plain = effect.len() <= 64
                && effect
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_');
            let (place, flags, data) = match rest.len() {
                9 => (&rest[..6], &rest[6..9], None),
                13 => (&rest[..6], &rest[6..9], Some(&rest[9..13])),
                _ => return None,
            };
            let [x, y, z, pitch, yaw, roll] = numbers::<6>(place)?;
            let data = match data {
                Some([cp, v @ ..]) => {
                    let cp: usize = cp.strip_prefix("cp")?.parse().ok()?;
                    (cp <= MAX_CONTROL_POINT).then_some(())?;
                    Some((cp, numbers::<3>(v)?))
                }
                _ => None,
            };
            plain.then(|| AmbientLine {
                effect: (*effect).to_owned(),
                at: [x, y, z],
                angles: [pitch, yaw, roll],
                day: flag(flags[0]).unwrap_or(false),
                night: flag(flags[1]).unwrap_or(false),
                fog: flag(flags[2]).unwrap_or(false),
                data,
            })
        })
        .collect()
}

/// A soundscape the map places (`map/mid/soundscapes.txt`): its name, where it is (Dota
/// coordinates), how far it is heard and the seconds it takes to fade in or out.
#[derive(Clone, Debug, PartialEq)]
pub struct ScapeLine {
    pub name: String,
    pub at: [f32; 3],
    pub radius: f32,
    pub fade: f32,
}

/// A sound a soundscape plays: a loop, or one of its waves now and then (every `every` seconds,
/// drawn in the range); its volume and pitch (percent) drawn in their ranges, where it sounds
/// (Dota coordinates; everywhere when none) and its sound level in dB (0: none given).
#[derive(Clone, Debug, PartialEq)]
pub struct ScapeSound {
    pub scape: String,
    pub looped: bool,
    pub volume: [f32; 2],
    pub pitch: [f32; 2],
    pub every: [f32; 2],
    pub at: Option<[f32; 3]>,
    pub db: f32,
    pub waves: Vec<String>,
}

/// Waves one random sound picks among, at most.
pub const MAX_WAVES: usize = 16;

/// `lo,hi` (or one number for both) within `[min, max]`, `lo <= hi`.
fn range(word: &str, min: f32, max: f32) -> Option<[f32; 2]> {
    let (lo, hi) = word.split_once(',').unwrap_or((word, word));
    let [lo, hi] = [number(lo)?, number(hi)?];
    (min <= lo && lo <= hi && hi <= max).then_some([lo, hi])
}

/// A place `x,y,z`, or none for `-`.
fn place(word: &str) -> Option<Option<[f32; 3]>> {
    if word == "-" {
        return Some(None);
    }
    let words: Vec<&str> = word.split(',').collect();
    numbers::<3>(&words).map(Some)
}

/// A wave's path in the cache: relative, of plain names, a `.wav`.
fn wave(word: &str) -> Option<String> {
    let plain = word.split('/').all(|part| {
        !part.is_empty()
            && part != ".."
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    });
    (plain && word.len() <= 256 && word.ends_with(".wav")).then(|| word.to_owned())
}

/// The soundscapes and their sounds of `soundscapes.txt`, of its first `max` lines: `scape
/// <name> x y z radius fade`, `loop <scape> volume pitch <x,y,z|-> db wave` and `random <scape>
/// vol_min,vol_max pitch_min,pitch_max time_min,time_max <x,y,z|-> db wave...`.
pub fn soundscapes(text: &str, max: usize) -> (Vec<ScapeLine>, Vec<ScapeSound>) {
    let mut scapes = Vec::new();
    let mut sounds = Vec::new();
    for w in lines(text, max) {
        match w.as_slice() {
            ["scape", name, rest @ ..] => {
                let Some([x, y, z, radius, fade]) = numbers::<5>(rest) else {
                    continue;
                };
                if radius > 0.0 && (0.0..=60.0).contains(&fade) {
                    scapes.push(ScapeLine {
                        name: (*name).to_owned(),
                        at: [x, y, z],
                        radius,
                        fade,
                    });
                }
            }
            ["loop", scape, volume, pitch, at, db, path] => {
                sounds.extend(sound(scape, true, [volume, pitch, "1", at, db], &[path]));
            }
            ["random", scape, volume, pitch, every, at, db, paths @ ..] => {
                sounds.extend(sound(scape, false, [volume, pitch, every, at, db], paths));
            }
            _ => {}
        }
    }
    (scapes, sounds)
}

/// One sound line's sound: its volume, pitch, period, place and level, and its waves.
fn sound(scape: &str, looped: bool, w: [&str; 5], paths: &[&str]) -> Option<ScapeSound> {
    let [volume, pitch, every, at, db] = w;
    let waves: Vec<String> = paths.iter().filter_map(|p| wave(p)).collect();
    let db = number(db).filter(|db| (0.0..=150.0).contains(db))?;
    (!waves.is_empty() && waves.len() <= MAX_WAVES).then_some(())?;
    Some(ScapeSound {
        scape: scape.to_owned(),
        looped,
        volume: range(volume, 0.0, 4.0)?,
        pitch: range(pitch, 1.0, 400.0)?,
        every: range(every, 0.05, 3600.0)?,
        at: place(at)?,
        db,
        waves,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The numbers reach the renderer: `NaN` and `inf` (which Rust reads as numbers) made
    /// cameras, lights and trees of no finite place.
    #[test]
    fn a_number_that_is_not_finite_is_no_line() {
        assert_eq!(trees("tree_a 1 2 3 4 1\ntree_b 1 2 3 NaN 1\n", 16).len(), 1);
        let built = "tower 1 2 3 0 1 1 1\nbarracks inf 2 3 0 1 1 1\npool 1 2 3 1 1 1 NaN\n";
        assert_eq!(buildings(built, 16).len(), 1);
        let light = "sun day 255 250 240 1\nsun night 50 60 90 0.5\n\
            ambient day 90 90 90 1\nambient night 20 20 40 1\nturn 3\n";
        assert_eq!(lighting(light).map(|l| l.turn), Some(3.0));
        assert!(lighting(&light.replace("0.5", "NaN")).is_none());
        assert!(lighting(&light.replace("turn 3", "turn inf")).is_none());
        let numbers = ["1"; FRAMING_NUMBERS].join(" ");
        let line = format!("hero.x.model {numbers} 1 1 1 1 1 1 1 1 1 1 1 1 bg.png");
        let framed = framing(&line).unwrap();
        assert_eq!(
            framed.background.map(|(path, _)| path).as_deref(),
            Some("bg.png")
        );
        assert!(framing(&line.replacen(" 1 ", " -inf ", 1)).is_none());
        assert!(framing(&line.replace(" 1 1 bg.png", " NaN 1 bg.png")).is_none());
        let placed = "fireflies 1 2 3 0 90 0 0 1 0\nbad/name 1 2 3 0 0 0 1 1 1\n\
            surface 1 2 3 0 0 0 1 1 1 cp10 1 0 0\nnan 1 2 NaN 0 0 0 1 1 1\n\
            far 1 2 3 0 0 0 1 1 1 cp64 1 0 0\n";
        let placed = ambient(placed, 16);
        assert_eq!(placed.len(), 2);
        assert_eq!(placed[1].data, Some((10, [1.0, 0.0, 0.0])));
        let scapes = "scape day 1 2 3 16000 3\nscape bad 1 2 NaN 16000 3\n\
            loop day 0.5 100 1,2,3 81 a/x.wav\nloop day NaN 100 - 0 a/x.wav\n\
            random day 0.7,1 95,105 6,9 - 90 a/b.wav ../c.wav\nrandom day 1,0.5 100 6 - 0 a/b.wav\n";
        let (scapes, sounds) = soundscapes(scapes, 16);
        assert_eq!(scapes.len(), 1);
        assert_eq!(sounds.len(), 2);
        assert_eq!(
            sounds[1].waves,
            ["a/b.wav"],
            "a wave out of the cache is no wave"
        );
    }
}
