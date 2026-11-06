//! The talking portrait, end to end in the real client: one's hero answers a move order and an
//! attack order with a voice line of its rules, and the bottom panel's portrait says it. The
//! face's morph weights as drawn (`--face-trace`, the idle expression held) open the jaw in step
//! with the line's phonemes (the lip sync the importer packed with the line in the hero's voice
//! pack, and Valve's phoneme table, through Source's box filter), and come back to the rest face,
//! still, once the line has ended. For each hero the game has. Drawn on the GPU in a headless
//! compositor, so it is left out of a plain `cargo test`; `tools/visual_check.sh` runs it.

mod common;

use std::collections::HashMap;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

/// Seconds a run may take.
const PATIENCE: Duration = Duration::from_secs(240);
/// The least correlation of the drawn jaw with the jaw the phonemes ask for, along a line.
const MIN_CORRELATION: f32 = 0.6;
/// Seconds of Source's box filter over the phonemes (`phonemefilter`), ahead of the time.
const FILTER: f32 = 0.08;
/// How far at least the jaw opens past its rest along a line.
const MIN_OPENING: f32 = 0.1;
/// Seconds past the end of its lip sync data a line may still be drawn: a frame at 30 a second,
/// and one of the game's.
const LATE: f32 = 0.1;

/// One frame of the trace: seconds into the line (none between lines) and the weights.
struct Frame {
    t: Option<f32>,
    weights: Vec<f32>,
}

/// A line the face said: its lip sync file and phoneme table, and the frames from the one
/// before it up to the next line.
struct Said {
    file: PathBuf,
    table: PathBuf,
    frames: Vec<Frame>,
}

/// What the trace says: the face mesh's target names and the lines said.
struct Trace {
    targets: Vec<String>,
    rest: Vec<f32>,
    lines: Vec<Said>,
}

/// The trace of the face mesh that has a jaw (`face-rig` and `face` lines of that mesh).
fn read_trace(said: &str) -> Trace {
    let mut mesh = None;
    let mut targets = Vec::new();
    let mut rest = None;
    let mut lines: Vec<Said> = Vec::new();
    for line in said.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["face-rig:", i, names @ ..] if mesh.is_none() && names.contains(&"jawOpen") => {
                mesh = Some(i.to_string());
                targets = names.iter().map(|n| n.to_string()).collect();
            }
            ["face-line:", file, table] => lines.push(Said {
                file: file.into(),
                table: table.into(),
                frames: Vec::new(),
            }),
            ["face:", i, t, weights @ ..] if Some(*i) == mesh.as_deref() => {
                let frame = Frame {
                    t: t.parse().ok(),
                    weights: weights
                        .iter()
                        .map(|w| w.parse().expect("a weight"))
                        .collect(),
                };
                match lines.last_mut() {
                    Some(said) => said.frames.push(frame),
                    None => rest = Some(frame.weights),
                }
            }
            _ => {}
        }
    }
    Trace {
        targets,
        rest: rest.expect("the face drawn at rest before it says a line"),
        lines,
    }
}

/// A record of a voice pack, named `PACK#RECORD` as the trace names a line's lip sync.
fn pack_record(file: &std::path::Path) -> (String, Vec<u8>) {
    let named = file.display().to_string();
    let (pack, record) = named.split_once('#').expect("a line of a voice pack");
    let bytes = std::fs::read(pack).expect("the hero's voice pack");
    let records = dbota_import::voice::pack::read(&bytes).expect("a voice pack");
    let found = records
        .iter()
        .find(|r| r.name == record)
        .expect("the record in the pack");
    (pack.to_owned(), bytes[found.data.clone()].to_vec())
}

/// The phonemes of a line's lip sync (start, end, code) and when its data ends.
fn lipsync(file: &std::path::Path) -> (Vec<(f32, f32, u32)>, f32) {
    let text = String::from_utf8(pack_record(file).1).expect("the line's lip sync");
    let mut phonemes = Vec::new();
    let mut end = 0.0f32;
    for words in text
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>())
    {
        match words.as_slice() {
            ["phoneme", start, stop, code] => {
                let stop: f32 = stop.parse().expect("a time");
                phonemes.push((
                    start.parse().expect("a time"),
                    stop,
                    code.parse().expect("a code"),
                ));
                end = end.max(stop);
            }
            ["key", t, ..] => end = end.max(t.parse().expect("a time")),
            _ => {}
        }
    }
    (phonemes, end)
}

/// Each phoneme's setting of `key` in Valve's phoneme table (as the importer writes it).
fn table_column(table: &PathBuf, key: &str) -> HashMap<u32, f32> {
    let text = std::fs::read_to_string(table).expect("the phoneme table");
    let mut column = None;
    let mut settings = HashMap::new();
    for words in text
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>())
    {
        match words.as_slice() {
            ["keys", keys @ ..] => column = keys.iter().position(|k| *k == key),
            ["phoneme", code, values @ ..] => {
                let at = column.expect("the table's keys first");
                settings.insert(
                    code.parse().expect("a code"),
                    values[at].parse().expect("a setting"),
                );
            }
            _ => {}
        }
    }
    settings
}

fn correlation(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len() as f32;
    let (ma, mb) = (a.iter().sum::<f32>() / n, b.iter().sum::<f32>() / n);
    let cov: f32 = a.iter().zip(b).map(|(x, y)| (x - ma) * (y - mb)).sum();
    let va: f32 = a.iter().map(|x| (x - ma).powi(2)).sum();
    let vb: f32 = b.iter().map(|y| (y - mb).powi(2)).sum();
    cov / (va * vb).sqrt().max(f32::MIN_POSITIVE)
}

/// Checks one line: along it the jaw follows its phonemes, after it the face is at rest.
fn check_line(trace: &Trace, said: &Said) -> Result<String, String> {
    let jaw = trace
        .targets
        .iter()
        .position(|n| n == "jawOpen")
        .expect("a jaw");
    let (phonemes, end) = lipsync(&said.file);
    let asked = table_column(&said.table, "jawOpen");
    let along: Vec<&Frame> = said.frames.iter().filter(|f| f.t.is_some()).collect();
    let (drawn, wanted): (Vec<f32>, Vec<f32>) = along
        .iter()
        .filter(|f| f.t.is_some_and(|t| t <= end))
        .map(|f| {
            let t = f.t.expect("along the line");
            // Each phoneme's share of the filter's window ahead of `t`.
            let wanted = phonemes
                .iter()
                .map(|&(start, stop, code)| {
                    let covered = (stop.min(t + FILTER) - start.max(t)).max(0.0) / FILTER;
                    covered * asked.get(&code).copied().unwrap_or(0.0)
                })
                .sum::<f32>();
            (f.weights[jaw], wanted.min(1.0))
        })
        .unzip();
    let r = correlation(&drawn, &wanted);
    let opening = drawn.iter().fold(0.0f32, |m, w| m.max(w - trace.rest[jaw]));
    let last = along.iter().filter_map(|f| f.t).fold(0.0f32, f32::max);
    let after: Vec<&Frame> = said
        .frames
        .iter()
        .skip_while(|f| f.t.is_none())
        .skip_while(|f| f.t.is_some())
        .collect();
    let off_rest = after
        .iter()
        .flat_map(|f| {
            f.weights
                .iter()
                .zip(&trace.rest)
                .map(|(w, r)| (w - r).abs())
        })
        .fold(0.0f32, f32::max);
    let report = format!(
        "{}: {} frames along {end:.2} s, jaw r = {r:.2} (>= {MIN_CORRELATION}), opens {opening:.2} \
         (>= {MIN_OPENING}), last drawn {last:.2} s in (<= {:.2}), {} frames after it {off_rest:.4} \
         off the rest face (0)",
        said.file.display(),
        drawn.len(),
        end + LATE,
        after.len(),
    );
    let pass = drawn.len() >= 10
        && r >= MIN_CORRELATION
        && opening >= MIN_OPENING
        && last <= end + LATE
        && after.len() >= 10
        && off_rest <= 1e-3;
    if pass { Ok(report) } else { Err(report) }
}

/// Plays `hero` (its pick button's name): a move order out of the fountain's reach, then an
/// attack order on an enemy hero made at the cursor beside it (`-createhero`), each answered
/// with a voice line; the client's trace.
fn play(hero: &str, enemy: &str) -> String {
    let scratch = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "talk-{}-{}",
        hero.replace(' ', "_"),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("a scratch folder");
    let server = common::serve(&[
        "--players",
        "1",
        "--pick-secs",
        "5",
        "--tactical-secs",
        "0",
        "--cheats",
    ]);
    // The hero first says its line entering battle and says nothing over it; it answers a move
    // and an attack each at most once in 6 s.
    let script = format!(
        "await:game;click:pick {hero};click:pick ready;await:level:1;wait:8;\
         mouse:0.9,0.15;rclick;wait:7;key:Space;wait:0.5;\
         mouse:0.56,0.46;say:-createhero {enemy} enemy;wait:0.4;rclick;wait:7;exit"
    );
    let mut child = common::client()
        .args(["--addr", &server.address, "--config"])
        .arg(scratch.join("settings.conf"))
        .args([
            "--script",
            &script,
            "--face-trace",
            "--set",
            "animate_portrait=true",
        ])
        .args(["--mute", "--no-vsync", "--display-mode", "windowed"])
        .data_dir(&scratch.join("data"))
        .env_remove("BOTA_SEAT_KEY")
        .stdout(Stdio::piped())
        .stderr(std::fs::File::create(scratch.join("client.log")).expect("a log file"))
        .spawn()
        .expect("dbota runs");
    let stdout = child.stdout.take().expect("its stdout");
    let said = std::thread::spawn(move || {
        let mut text = String::new();
        BufReader::new(stdout).read_to_string(&mut text).ok();
        text
    });
    let started = Instant::now();
    let code = loop {
        if let Some(status) = child.try_wait().expect("waited for") {
            break status.code();
        }
        if started.elapsed() > PATIENCE {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let log = std::fs::read_to_string(scratch.join("client.log")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&scratch);
    let said = said.join().unwrap_or_default();
    let steps = script.split(';').filter(|s| !s.trim().is_empty()).count();
    let done = said.lines().filter(|l| l.starts_with("script: ")).count();
    let tail: Vec<&str> = said.lines().filter(|l| !l.starts_with("face: ")).collect();
    let log: Vec<&str> = log.lines().rev().take(20).collect();
    assert!(
        code == Some(0) && done == steps,
        "{hero}: exit {code:?}, {done} of {steps} steps:\n{}\nthe client's log ends:\n{}",
        tail.join("\n"),
        log.into_iter().rev().collect::<Vec<_>>().join("\n")
    );
    said
}

/// The concepts (`TLK_DOTA_MOVE`, ...) a line of the trace answers: those of the rules of its pack
/// that may say its sound.
fn concepts_of(file: &std::path::Path) -> Vec<String> {
    let (pack, _) = pack_record(file);
    let named = file.display().to_string();
    let record = named.split_once('#').expect("a pack's record").1;
    let sound: usize = record
        .strip_prefix("lipsync/")
        .and_then(|r| r.strip_suffix(".txt"))
        .and_then(|n| n.parse().ok())
        .expect("a lip sync record");
    let hero = std::path::Path::new(&pack)
        .file_stem()
        .and_then(|s| s.to_str())
        .expect("the pack's hero");
    let bytes = std::fs::read(&pack).expect("the hero's voice pack");
    let rules = dbota_import::voice::open(hero, &bytes)
        .expect("the pack opens")
        .rules;
    let mut concepts = Vec::new();
    for rule in &rules.rules {
        let says = rule
            .lines
            .iter()
            .any(|l| rules.events[l.event].sounds.contains(&sound));
        for &c in rule.criteria.iter().filter(|_| says) {
            let c = &rules.criteria[c];
            if c.key.eq_ignore_ascii_case("concept") {
                concepts.push(c.expr.clone());
            }
        }
    }
    concepts
}

/// Checks that `hero`, after whatever it said entering battle, says a move line, then an attack
/// line, with its face.
fn says_its_lines(hero: &str, enemy: &str) {
    let trace = read_trace(&play(hero, enemy));
    let results: Vec<Result<String, String>> =
        trace.lines.iter().map(|s| check_line(&trace, s)).collect();
    for result in &results {
        println!("{hero}: {}", result.as_ref().unwrap_or_else(|e| e));
    }
    let kinds: Vec<Vec<String>> = trace.lines.iter().map(|s| concepts_of(&s.file)).collect();
    let last = kinds.len().saturating_sub(2);
    let answered = |at: usize, concept: &str| {
        kinds
            .get(at)
            .is_some_and(|k| k.iter().any(|c| c == concept))
    };
    assert!(
        kinds.len() >= 2
            && answered(last, "TLK_DOTA_MOVE")
            && answered(last + 1, "TLK_DOTA_ATTACK"),
        "{hero}: a move line then an attack line last, not {kinds:?}"
    );
    assert!(results.iter().all(Result::is_ok), "{hero}: {results:#?}");
}

#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn pudge_says_his_lines_with_his_face() {
    says_its_lines("pudge", "nevermore");
}

#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn shadow_fiend_says_his_lines_with_his_face() {
    says_its_lines("shadow fiend", "pudge");
}
