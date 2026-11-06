//! A hero's voice pack ([`crate::voice`]) out of the install: the hero's response rules and
//! the files they include, pruned to the rules a query of this client can match, the voice
//! events their lines name (from the hero's `VoiceFile` and the scripts the rules name), and
//! the sound and lip sync of each.
//!
//! Pruning only drops what can never be said here; each line dropped is counted under one
//! reason ([`Prune`], see [`verdict`]). Each criterion is judged on its own: a rule kept may
//! still never match (two of its criteria may contradict each other), but no rule dropped
//! could ever have matched.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use heatel::sound::{Lipsync, SoundEvent, read_sound_events, read_sound_mixer};
use heatel::talker::{Criterion, ResponseGroup, ResponseKind, read_response_rules};
use heatel::{Resource, Result, Sound, Source, Vpk};

use super::{Out, SOUND_MIXER_SRC, malformed, read_from, resolve_event, safe_path, sound_stacks};
use crate::text::{lipsync_txt, sound_events_txt};
use crate::voice::concept::{Concept, Fact, GAME_START, powerup_type};
use crate::voice::expr::Matcher;
use crate::voice::rules::{self, Event, HeroRules, Line, Rule};
use crate::voice::{EVENTS, RULES, lipsync_record, pack, sound_record};

/// Most rule files one hero's rules read, its own and those included: the install's largest
/// include list (an announcer's aggregator) names 134.
const MAX_RULE_FILES: usize = 256;
/// Most sound event scripts read for one hero: its voice file and those its rules name.
const MAX_EVENT_SCRIPTS: usize = 16;
/// Most bytes of one sound: the install's largest voice line is 2,118,182.
const MAX_SOUND_BYTES: usize = 4 << 20;

/// Why a line can never be said here.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Prune {
    /// Its rule requires a criterion no file defines.
    UnknownCriterion,
    /// Its rule's concept is none the client raises (`TLK_DOTA_SELECT`).
    ConceptNotRaised(String),
    /// Its rule requires a fact the client never supplies with the concept: `customresponse`
    /// is what a cosmetic item says of its wearer (its `response_criteria` asset modifier in
    /// `scripts/items/items_game.txt`: the Feast of Abscession arcana's is `pudarc_chains`), and
    /// the client draws heroes in their default items only; others are of events the client has
    /// no fact of (Pudge's `hookedtarget`, a taunt's `taunt_type`).
    FactNotSupplied(String),
    /// Its rule requires a value the game never has (a rival hero, an item the game lacks).
    AbsentValue(String),
    /// It is not a line said (`SCENE`, `PRINT`, `RESPONSE`), or names nothing.
    NotSpeech,
    /// Its voice event is not in the hero's scripts, or its sound does not read.
    NoSound,
}

impl Prune {
    /// The reason's name, as the pack and the log count it.
    pub(crate) fn name(&self) -> String {
        match self {
            Prune::UnknownCriterion => "unknown criterion".to_owned(),
            Prune::ConceptNotRaised(c) => format!("concept not raised: {c}"),
            Prune::FactNotSupplied(key) => format!("fact not supplied: {key}"),
            Prune::AbsentValue(key) => format!("value absent from the game: {key}"),
            Prune::NotSpeech => "not speech".to_owned(),
            Prune::NoSound => "no sound".to_owned(),
        }
    }
}

/// The values a fact takes in this game, when there are few enough to list.
fn domain(fact: Fact, concept: Concept, class: &str) -> Option<Vec<String>> {
    let keys = |keys: &mut dyn Iterator<Item = &'static str>| keys.map(str::to_owned).collect();
    Some(match fact {
        Fact::Concept => vec![concept.dota_name().to_owned()],
        Fact::ClassName => vec![class.to_owned()],
        Fact::RandomNum => (0..100).map(|n: u32| n.to_string()).collect(),
        Fact::AbilityName => keys(&mut bota_core::ABILITIES.iter().map(|a| a.key)),
        Fact::TargetClassName => keys(&mut bota_core::HEROES.iter().map(|h| h.key)),
        Fact::ItemName => keys(&mut bota_core::ITEMS.iter().map(|i| i.key)),
        Fact::FirstBlood => vec!["0".to_owned(), "1".to_owned()],
        Fact::Level => (1..=bota_core::rules::HERO_MAX_LEVEL)
            .map(|n| n.to_string())
            .collect(),
        Fact::PowerupType => bota_proto::RuneKind::ALL.map(powerup_type).into(),
        Fact::AnnounceEvent => vec![GAME_START.to_owned()],
        Fact::GameTime | Fact::LastHitCount => return None,
    })
}

/// Why a query of `concept` can never match `criterion`, if it cannot.
fn unmatchable(concept: Concept, criterion: &Criterion, class: &str) -> Option<Prune> {
    let key = criterion.match_key.as_deref().unwrap_or(&criterion.name);
    let expr = criterion.match_expr.as_deref().unwrap_or_default();
    let matcher = Matcher::parse(expr);
    let Some(fact) = Fact::of_key(key).filter(|&fact| concept.carries(fact)) else {
        return (!matcher.matches(None)).then(|| Prune::FactNotSupplied(key.to_ascii_lowercase()));
    };
    let values = domain(fact, concept, class)?;
    if values.iter().any(|v| matcher.matches(Some(v))) {
        return None;
    }
    Some(match fact {
        Fact::Concept => Prune::ConceptNotRaised(expr.to_owned()),
        _ => Prune::AbsentValue(key.to_ascii_lowercase()),
    })
}

/// Why no query of this client can match a rule requiring `criteria`, if none can. Of the
/// reasons that rule it out for a concept the rule allows, the first in [`Prune`]'s order
/// counts (whatever order the rule lists its criteria in); a rule no raised concept can match
/// counts under its own concept.
fn verdict(criteria: &[&Criterion], class: &str) -> Option<Prune> {
    // A rule of no criteria scores nothing, and Source takes no rule that scores nothing.
    if criteria.is_empty() {
        return Some(Prune::ConceptNotRaised("none".to_owned()));
    }
    let mut best: Option<Prune> = None;
    for concept in Concept::ALL {
        let why = criteria
            .iter()
            .filter_map(|c| unmatchable(concept, c, class))
            .min()?;
        if !matches!(why, Prune::ConceptNotRaised(_)) && best.as_ref().is_none_or(|b| why < *b) {
            best = Some(why);
        }
    }
    Some(best.unwrap_or_else(|| {
        let named = criteria
            .iter()
            .find(|c| c.match_key.as_deref().and_then(Fact::of_key) == Some(Fact::Concept))
            .and_then(|c| c.match_expr.clone());
        Prune::ConceptNotRaised(named.unwrap_or_else(|| "none".to_owned()))
    }))
}

/// The rule files of a hero: its own, then those it includes, breadth first, each once.
struct RuleFiles {
    criteria: Vec<Criterion>,
    groups: Vec<ResponseGroup>,
    /// The sound event scripts the files name, as `.vsndevts_c` paths.
    scripts: Vec<String>,
}

fn rule_path(path: &str) -> Result<String> {
    let plain = path.strip_suffix("_c").unwrap_or(path);
    if !plain.starts_with("scripts/talker/") || !plain.ends_with(".vrr") || !safe_path(plain) {
        return Err(malformed("voice", format!("rule file {path:?}")));
    }
    Ok(format!("{plain}_c"))
}

fn script_path(path: &str) -> Result<String> {
    let plain = path.strip_suffix("_c").unwrap_or(path);
    if !plain.starts_with("soundevents/") || !plain.ends_with(".vsndevts") || !safe_path(plain) {
        return Err(malformed("voice", format!("sound event script {path:?}")));
    }
    Ok(format!("{plain}_c"))
}

fn read_rule_files(src: &impl Source, pak: &Vpk, hero: &str) -> Result<RuleFiles> {
    let mut queue = VecDeque::from([rule_path(&format!(
        "scripts/talker/response_rules_{hero}.vrr"
    ))?]);
    let mut seen = BTreeSet::new();
    let mut files = RuleFiles {
        criteria: Vec::new(),
        groups: Vec::new(),
        scripts: Vec::new(),
    };
    while let Some(path) = queue.pop_front() {
        if !seen.insert(path.clone()) {
            continue;
        }
        if seen.len() > MAX_RULE_FILES {
            return Err(malformed(
                "voice",
                "more included rule files than any hero has",
            ));
        }
        let read = read_response_rules(&Resource::parse(&read_from(src, &[pak], &path)?)?)?;
        for include in &read.includes {
            queue.push_back(rule_path(include)?);
        }
        for script in &read.sound_event_scripts {
            files.scripts.push(script_path(script)?);
        }
        files.criteria.extend(read.criteria);
        files.groups.extend(read.groups);
    }
    Ok(files)
}

/// A rule kept, its criteria by name, and the lines it may say by voice event name.
struct Kept {
    name: String,
    criteria: Vec<String>,
    lines: Vec<(String, heatel::talker::Response)>,
}

/// What pruning keeps, and how many lines it drops by why.
struct Pruned {
    rules: Vec<Kept>,
    counts: BTreeMap<Prune, u32>,
    total: u32,
}

fn bump(counts: &mut BTreeMap<Prune, u32>, why: Prune, lines: usize) {
    let lines = u32::try_from(lines).unwrap_or(u32::MAX);
    let count = counts.entry(why).or_default();
    *count = count.saturating_add(lines);
}

/// The criteria of the rule files by name, in any case: a name's first definition wins, the
/// hero's own file before what it includes.
fn criteria_by_name(files: &RuleFiles) -> HashMap<String, &Criterion> {
    let mut by_name = HashMap::new();
    for c in &files.criteria {
        by_name.entry(c.name.to_ascii_lowercase()).or_insert(c);
    }
    by_name
}

/// The rules a query of this client can match for a hero of `class`.
fn prune(files: &RuleFiles, class: &str) -> Pruned {
    let by_name = criteria_by_name(files);
    let mut pruned = Pruned {
        rules: Vec::new(),
        counts: BTreeMap::new(),
        total: 0,
    };
    for group in &files.groups {
        let lines = group.responses.len();
        pruned.total = pruned
            .total
            .saturating_add(u32::try_from(lines).unwrap_or(u32::MAX));
        let criteria: Option<Vec<&Criterion>> = group
            .requirements
            .iter()
            .map(|name| by_name.get(&name.to_ascii_lowercase()).copied())
            .collect();
        let why = match &criteria {
            None => Some(Prune::UnknownCriterion),
            Some(criteria) => verdict(criteria, class),
        };
        if let Some(why) = why {
            bump(&mut pruned.counts, why, lines);
            continue;
        }
        let (speech, other): (Vec<_>, Vec<_>) = group.responses.iter().partition(|r| {
            r.kind == ResponseKind::Speak && r.value.as_deref().is_some_and(|v| !v.is_empty())
        });
        if !other.is_empty() {
            bump(&mut pruned.counts, Prune::NotSpeech, other.len());
        }
        pruned.rules.push(Kept {
            name: group.name.clone(),
            criteria: group.requirements.clone(),
            lines: speech
                .into_iter()
                .map(|r| (r.value.clone().unwrap_or_default(), r.clone()))
                .collect(),
        });
    }
    pruned
}

/// The hero's class and its voice file, from its script (`scripts/npc/heroes/`).
fn hero_script(src: &impl Source, pak: &Vpk, hero: &str) -> Result<(String, String)> {
    let class = format!("npc_dota_hero_{hero}");
    let bytes = read_from(src, &[pak], &format!("scripts/npc/heroes/{class}.txt"))?;
    let tree = heatel::keyvalues::parse_kv(heatel::keyvalues::text(&bytes)?)?;
    let voice = tree
        .get("DOTAHeroes")
        .and_then(|heroes| heroes.get(&class))
        .and_then(|definition| definition.get("VoiceFile"));
    match voice {
        Some(heatel::keyvalues::Kv::Text(path)) => Ok((class, script_path(path)?)),
        _ => Err(malformed("voice", format!("{class} names no VoiceFile"))),
    }
}

/// The voice events `wanted` as the hero's scripts state them; one no script has is left out.
fn voice_events(
    src: &impl Source,
    pak: &Vpk,
    scripts: &[String],
    wanted: &BTreeSet<&str>,
) -> Result<BTreeMap<String, SoundEvent>> {
    let mut found = BTreeMap::new();
    let scripts: BTreeSet<&String> = scripts.iter().collect();
    if scripts.len() > MAX_EVENT_SCRIPTS {
        return Err(malformed(
            "voice",
            "more sound event scripts than any hero has",
        ));
    }
    for script in scripts {
        let events = read_sound_events(&Resource::parse(&read_from(src, &[pak], script)?)?)?;
        for event in events {
            if wanted.contains(event.name.as_str()) && !found.contains_key(&event.name) {
                found.insert(event.name.clone(), event);
            }
        }
    }
    Ok(found)
}

/// The sounds of a pack as it is being made: each sound file once, by its path.
#[derive(Default)]
struct Sounds {
    index: HashMap<String, usize>,
    /// Per sound: its audio, its kind, its lip sync, its seconds.
    files: Vec<(Vec<u8>, &'static str, Option<String>, f32)>,
}

impl Sounds {
    /// The pack's index of the sound at `path` (a `.vsnd`), read the first time it is asked.
    fn add(
        &mut self,
        src: &impl Source,
        pak: &Vpk,
        path: &str,
        out: &mut Out<'_>,
    ) -> Result<usize> {
        if let Some(&at) = self.index.get(path) {
            return Ok(at);
        }
        if !path.starts_with("sounds/") || !path.ends_with(".vsnd") || !safe_path(path) {
            return Err(malformed("voice", format!("sound path {path:?}")));
        }
        let bytes = read_from(src, &[pak], &format!("{path}_c"))?;
        if bytes.len() > MAX_SOUND_BYTES {
            return Err(malformed("voice", format!("{path}: {} bytes", bytes.len())));
        }
        let sound = Sound::parse(&bytes)?;
        let lipsync = match Lipsync::parse(&bytes) {
            Ok(lipsync) if !lipsync.is_empty() => Some(lipsync_txt(&lipsync)),
            Ok(_) => None,
            // Heard with a still face; told as a failure of its own.
            Err(err) => {
                out.report
                    .failed
                    .push((format!("{path} lip sync"), err.to_string()));
                None
            }
        };
        let seconds = sound.duration();
        self.files.push((
            sound.file().into_owned(),
            sound.extension(),
            lipsync,
            seconds,
        ));
        self.index.insert(path.to_owned(), self.files.len() - 1);
        Ok(self.files.len() - 1)
    }
}

/// The voice events of the pack: each found event whose sounds read, by its index, with its
/// mixer line. An event whose sounds do not read is a failure of its own; its lines are pruned.
struct PackEvents {
    events: Vec<Event>,
    index: HashMap<String, usize>,
    resolved: Vec<crate::text::ResolvedEvent>,
}

fn pack_events(
    src: &impl Source,
    pak: &Vpk,
    found: BTreeMap<String, SoundEvent>,
    sounds: &mut Sounds,
    out: &mut Out<'_>,
) -> Result<PackEvents> {
    let mixer = read_sound_mixer(&read_from(src, &[pak], SOUND_MIXER_SRC)?)?;
    let stacks = sound_stacks(src, pak)?;
    let mut made = PackEvents {
        events: Vec::new(),
        index: HashMap::new(),
        resolved: Vec::new(),
    };
    for (name, event) in found {
        let indices: Result<Vec<usize>> = event
            .files
            .iter()
            .map(|file| sounds.add(src, pak, file, out))
            .collect();
        match indices {
            Ok(indices) if !indices.is_empty() && indices.len() <= rules::MAX_EVENT_SOUNDS => {
                let seconds = indices
                    .iter()
                    .map(|&i| sounds.files[i].3)
                    .fold(0.0, f32::max);
                made.index.insert(name.clone(), made.events.len());
                made.events.push(Event {
                    name,
                    sounds: indices,
                    seconds: seconds.min(rules::MAX_SECONDS),
                });
                made.resolved
                    .push(resolve_event(event, false, &stacks, &mixer));
            }
            Ok(_) => out.report.failed.push((
                name,
                "a voice event of no sound, or of more sounds than any".to_owned(),
            )),
            Err(err) => out.report.failed.push((name, err.to_string())),
        }
    }
    Ok(made)
}

/// The pack's bytes: the rules, the events' mixer lines, then each sound and its lip sync.
fn pack_bytes(json: &[u8], events_txt: &str, sounds: &Sounds) -> Result<Vec<u8>> {
    let names: Vec<(String, Option<String>)> = sounds
        .files
        .iter()
        .enumerate()
        .map(|(i, (_, ext, lipsync, _))| {
            (
                sound_record(i, ext),
                lipsync.as_ref().map(|_| lipsync_record(i)),
            )
        })
        .collect();
    let mut records: Vec<(&str, &[u8])> = vec![(RULES, json), (EVENTS, events_txt.as_bytes())];
    for ((sound, lipsync), (audio, _, lipsync_text, _)) in names.iter().zip(&sounds.files) {
        records.push((sound, audio));
        if let (Some(name), Some(text)) = (lipsync, lipsync_text) {
            records.push((name, text.as_bytes()));
        }
    }
    pack::write(records).map_err(|e| malformed("voice", e.to_string()))
}

/// Writes `hero`'s voice pack at `dest`.
pub(super) fn voice(
    src: &impl Source,
    pak: &Vpk,
    hero: &str,
    dest: &str,
    out: &mut Out<'_>,
) -> Result<()> {
    let (class, voice_file) = hero_script(src, pak, hero)?;
    let files = read_rule_files(src, pak, hero)?;
    let mut pruned = prune(&files, &class);
    let wanted: BTreeSet<&str> = pruned
        .rules
        .iter()
        .flat_map(|r| r.lines.iter().map(|(event, _)| event.as_str()))
        .collect();
    let mut scripts = vec![voice_file];
    scripts.extend(files.scripts.iter().cloned());
    let found = voice_events(src, pak, &scripts, &wanted)?;
    let mut sounds = Sounds::default();
    let events = pack_events(src, pak, found, &mut sounds, out)?;
    let rules = hero_rules(
        hero,
        &class,
        &files,
        &mut pruned,
        &events.index,
        events.events,
    );
    tell(hero, &rules, &pruned, out);
    let json = rules.to_json();
    // What the client reads back is what was meant: a pack over a bound fails here, not there.
    HeroRules::parse(&json, sounds.files.len()).map_err(|why| malformed("voice", why))?;
    let bytes = pack_bytes(&json, &sound_events_txt(&events.resolved), &sounds)?;
    out.put(dest, &bytes)
}

/// The pack's rules: the kept rules whose lines have a sound, their criteria renumbered, the
/// lines without a sound counted as pruned.
fn hero_rules(
    hero: &str,
    class: &str,
    files: &RuleFiles,
    pruned: &mut Pruned,
    events: &HashMap<String, usize>,
    event_list: Vec<Event>,
) -> HeroRules {
    let by_name = criteria_by_name(files);
    let mut criteria: Vec<rules::Criterion> = Vec::new();
    let mut criterion_index: HashMap<String, usize> = HashMap::new();
    let mut kept_rules = Vec::new();
    for kept in &pruned.rules {
        let lines = lines_of(kept, events);
        let silent = kept.lines.len() - lines.len();
        if silent > 0 {
            bump(&mut pruned.counts, Prune::NoSound, silent);
        }
        if lines.is_empty() {
            continue;
        }
        let indices = kept
            .criteria
            .iter()
            .map(|name| {
                let key = name.to_ascii_lowercase();
                let next = criteria.len();
                *criterion_index.entry(key.clone()).or_insert_with(|| {
                    criteria.push(criterion_of(by_name[&key]));
                    next
                })
            })
            .collect();
        kept_rules.push(Rule {
            name: kept.name.clone(),
            criteria: indices,
            lines,
        });
    }
    HeroRules {
        hero: hero.to_owned(),
        class: class.to_owned(),
        criteria,
        rules: kept_rules,
        events: event_list,
        pruned: pruned
            .counts
            .iter()
            .map(|(why, n)| (why.name(), *n))
            .collect(),
    }
}

/// The lines of a kept rule whose voice event the pack has, with their own delays.
fn lines_of(kept: &Kept, events: &HashMap<String, usize>) -> Vec<Line> {
    let seconds = |s: Option<f32>| s.unwrap_or(0.0).clamp(0.0, rules::MAX_SECONDS);
    kept.lines
        .iter()
        .filter_map(|(event, response)| {
            Some(Line {
                event: *events.get(event)?,
                once: response.speak_once,
                respeak: seconds(response.respeak_delay),
                predelay: seconds(response.pre_delay),
            })
        })
        .collect()
}

/// A criterion as the pack keeps it: its key is its name when it names none, its weight 1 when
/// it states none (Source's default).
fn criterion_of(c: &Criterion) -> rules::Criterion {
    let expr = c.match_expr.clone().unwrap_or_default();
    rules::Criterion {
        name: c.name.clone(),
        key: c.match_key.clone().unwrap_or_else(|| c.name.clone()),
        matcher: Matcher::parse(&expr),
        expr,
        weight: c
            .weight
            .unwrap_or(1.0)
            .clamp(-rules::MAX_WEIGHT, rules::MAX_WEIGHT),
    }
}

/// Tells what the pack keeps and prunes, and checks that every line is one or the other.
fn tell(hero: &str, rules: &HeroRules, pruned: &Pruned, out: &mut Out<'_>) {
    let kept = u32::try_from(rules.lines()).unwrap_or(u32::MAX);
    let dropped: u32 = pruned.counts.values().sum();
    assert_eq!(
        kept + dropped,
        pruned.total,
        "{hero}: every response line is kept or pruned"
    );
    let mut note = format!("voice {hero}: {kept} of {} lines kept", pruned.total);
    for (why, n) in &pruned.counts {
        note.push_str(&format!("; {n} {}", why.name()));
    }
    out.report.notes.push(note);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn criterion(name: &str, key: &str, expr: &str) -> Criterion {
        Criterion {
            name: name.into(),
            match_key: Some(key.into()),
            match_expr: Some(expr.into()),
            weight: None,
            required: false,
        }
    }

    fn group(name: &str, requirements: &[&str], lines: usize) -> ResponseGroup {
        ResponseGroup {
            name: name.into(),
            rule: name.into(),
            requirements: requirements.iter().map(|r| (*r).to_owned()).collect(),
            responses: (0..lines)
                .map(|i| heatel::talker::Response {
                    kind: ResponseKind::Speak,
                    value: Some(format!("{name}_{i}")),
                    speak_once: false,
                    respeak_delay: None,
                    pre_delay: None,
                    weapon_delay: None,
                    followup: None,
                })
                .collect(),
        }
    }

    fn files() -> RuleFiles {
        RuleFiles {
            criteria: vec![
                criterion("Move", "Concept", "TLK_DOTA_MOVE"),
                criterion("Select", "Concept", "TLK_DOTA_SELECT"),
                criterion("Custom", "Concept", "TLK_DOTA_CUSTOM"),
                criterion("Ispudge", "classname", "npc_dota_hero_pudge"),
                criterion("IsPudgeChain", "customresponse", "pudarc_chains"),
                criterion("IsNotChain", "customresponse", "!=pudarc_chains"),
                criterion("HookCatchNothing", "hookedtarget", "0"),
                criterion("IsGameStart", "announce_event", "game_start"),
                criterion("IsMeepo", "targetclassname", "npc_dota_hero_meepo"),
                criterion("Kill", "Concept", "TLK_DOTA_KILL"),
            ],
            groups: vec![
                group("pudge_Move", &["Move", "Ispudge"], 3),
                group(
                    "pudge_MoveIsPudgeChain",
                    &["Move", "Ispudge", "IsPudgeChain"],
                    4,
                ),
                group(
                    "pudge_MoveIsNotChain",
                    &["Move", "Ispudge", "IsNotChain"],
                    1,
                ),
                group("pudge_Select", &["Select", "Ispudge"], 5),
                group(
                    "pudge_CustomHookCatchNothing",
                    &["Custom", "Ispudge", "HookCatchNothing"],
                    6,
                ),
                group(
                    "pudge_CustomIsGameStart",
                    &["Custom", "Ispudge", "IsGameStart"],
                    2,
                ),
                group("pudge_KillIsMeepo", &["Kill", "Ispudge", "IsMeepo"], 7),
                group("pudge_Unknown", &["Move", "Nowhere"], 8),
            ],
            scripts: Vec::new(),
        }
    }

    #[test]
    fn pruning_counts_every_line_under_one_reason() {
        let pruned = prune(&files(), "npc_dota_hero_pudge");
        let kept: Vec<&str> = pruned.rules.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            kept,
            [
                "pudge_Move",
                "pudge_MoveIsNotChain",
                "pudge_CustomIsGameStart"
            ]
        );
        let counts: Vec<(String, u32)> = pruned
            .counts
            .iter()
            .map(|(why, n)| (why.name(), *n))
            .collect();
        assert_eq!(
            counts,
            [
                ("unknown criterion".to_owned(), 8),
                ("concept not raised: TLK_DOTA_SELECT".to_owned(), 5),
                ("fact not supplied: customresponse".to_owned(), 4),
                ("fact not supplied: hookedtarget".to_owned(), 6),
                ("value absent from the game: targetclassname".to_owned(), 7),
            ]
        );
        assert_eq!(pruned.total, 36);
    }
}
