//! The asset cache's small text files as the client reads them: where the map puts its trees
//! and buildings, its lighting by day and by night, and the portrait framings. Any program of
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

/// The buildings of `buildings.txt` (`model x y yaw`), of its first `max` lines.
pub fn buildings(text: &str, max: usize) -> Vec<BuildingLine> {
    lines(text, max)
        .filter_map(|w| {
            let [model, rest @ ..] = w.as_slice() else {
                return None;
            };
            let [x, y, yaw] = numbers::<3>(rest)?;
            Some(BuildingLine {
                model: (*model).to_owned(),
                x,
                y,
                yaw,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The numbers reach the renderer: `NaN` and `inf` (which Rust reads as numbers) made
    /// cameras, lights and trees of no finite place.
    #[test]
    fn a_number_that_is_not_finite_is_no_line() {
        assert_eq!(trees("tree_a 1 2 3 4 1\ntree_b 1 2 3 NaN 1\n", 16).len(), 1);
        assert_eq!(buildings("tower 1 2 3\nbarracks inf 2 3\n", 16).len(), 1);
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
    }
}
