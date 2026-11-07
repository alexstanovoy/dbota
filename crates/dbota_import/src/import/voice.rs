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

use heatel::sound::SoundEvent;
use heatel::talker::{Criterion, ResponseGroup, ResponseKind};
use heatel::{Result, Source};

use super::{Out, malformed, resolve_event, safe_path};
use crate::text::{lipsync_txt, sound_events_txt};
use crate::voice::concept::{Concept, Fact, GAME_START, powerup_type};
use crate::voice::expr::Matcher;
use crate::voice::rules::{self, Event, HeroRules, Line, Rule};
use crate::voice::{EVENTS, RULES, lipsync_record, pack, sound_record};

mod files;
pub(super) use files::HeroFiles;
use files::MAX_SCRIPT_BYTES;

/// Most rule files one hero's rules read, its own and those included: the install's largest
/// include list (an announcer's aggregator) names 134.
const MAX_RULE_FILES: usize = 256;
/// Most sound event scripts read for one hero: its voice file and those its rules name.
const MAX_EVENT_SCRIPTS: usize = 16;
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
        Fact::RandomNum => (1..=100).map(|n: u32| n.to_string()).collect(),
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
    scripts: BTreeSet<String>,
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

fn read_rule_files(files: &mut HeroFiles<'_, impl Source>, hero: &str) -> Result<RuleFiles> {
    let mut queue = VecDeque::from([rule_path(&format!(
        "scripts/talker/response_rules_{hero}.vrr"
    ))?]);
    let mut seen = BTreeSet::new();
    let mut rules = RuleFiles {
        criteria: Vec::new(),
        groups: Vec::new(),
        scripts: BTreeSet::new(),
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
        let read = files.rules(&path)?;
        for include in &read.includes {
            queue.push_back(rule_path(include)?);
        }
        if queue.len() > MAX_RULE_FILES {
            return Err(malformed(
                "voice",
                "more included rule files than any hero has",
            ));
        }
        for script in &read.sound_event_scripts {
            rules.scripts.insert(script_path(script)?);
        }
        if rules.scripts.len() > MAX_EVENT_SCRIPTS {
            return Err(malformed(
                "voice",
                "more sound event scripts than any hero has",
            ));
        }
        rules.criteria.extend(read.criteria);
        rules.groups.extend(read.groups);
    }
    Ok(rules)
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
fn hero_script(files: &mut HeroFiles<'_, impl Source>, hero: &str) -> Result<(String, String)> {
    let class = format!("npc_dota_hero_{hero}");
    let bytes = files.bytes(&format!("scripts/npc/heroes/{class}.txt"), MAX_SCRIPT_BYTES)?;
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
    files: &mut HeroFiles<'_, impl Source>,
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
        for event in files.events(script)? {
            if wanted.contains(event.name.as_str()) && !found.contains_key(&event.name) {
                found.insert(event.name.clone(), event);
            }
        }
    }
    Ok(found)
}

/// One sound of a pack: its audio, its kind, its lip sync, its seconds.
type SoundFile = (Vec<u8>, &'static str, Option<String>, f32);

/// The sounds of a pack as it is being made: each sound file once, by its path, the bytes of
/// all of them within the pack's bound.
struct Sounds {
    index: HashMap<String, usize>,
    files: Vec<SoundFile>,
    /// Bytes the pack may still take: what [`Sounds::keep`] keeps is charged before it is kept.
    left: usize,
    /// Whether a sound was refused for want of room: the pack would not hold them all.
    full: bool,
}

impl Default for Sounds {
    fn default() -> Self {
        Self {
            index: HashMap::new(),
            files: Vec::new(),
            left: pack::MAX_PACK_BYTES,
            full: false,
        }
    }
}

impl Sounds {
    fn no_room(&mut self, path: &str) -> heatel::Error {
        self.full = true;
        malformed(
            "voice",
            format!(
                "{path}: more sound than a pack holds ({} of {} bytes left)",
                self.left,
                pack::MAX_PACK_BYTES
            ),
        )
    }

    /// Keeps `file` as the sound at `path`, charged with every byte it keeps.
    fn keep(&mut self, path: &str, file: SoundFile) -> Result<usize> {
        let size = file.0.len() + file.2.as_ref().map_or(0, String::len);
        self.left = match self.left.checked_sub(size) {
            Some(left) => left,
            None => return Err(self.no_room(path)),
        };
        self.files.push(file);
        self.index.insert(path.to_owned(), self.files.len() - 1);
        Ok(self.files.len() - 1)
    }

    /// The pack's index of the sound at `path` (a `.vsnd`), read the first time it is asked.
    fn add(
        &mut self,
        files: &mut HeroFiles<'_, impl Source>,
        path: &str,
        out: &mut Out<'_>,
    ) -> Result<usize> {
        if let Some(&at) = self.index.get(path) {
            return Ok(at);
        }
        if !path.starts_with("sounds/") || !path.ends_with(".vsnd") || !safe_path(path) {
            return Err(malformed("voice", format!("sound path {path:?}")));
        }
        let stored = format!("{path}_c");
        // The audio kept is a part of the file: a file larger than the room left is not read.
        if files
            .size(&stored)
            .is_some_and(|size| size > self.left as u64)
        {
            return Err(self.no_room(path));
        }
        let sound = files.sound(&stored)?;
        let lipsync = match sound.lipsync {
            Ok(lipsync) if !lipsync.is_empty() => {
                // Its text is of the phonemes and keys charged with the sound.
                let text = lipsync_txt(&lipsync);
                files.budget().charge(text.len())?;
                Some(text)
            }
            Ok(_) => None,
            // Heard with a still face; told as a failure of its own.
            Err(err) => {
                out.report
                    .failed
                    .push((format!("{path} lip sync"), err.to_string()));
                None
            }
        };
        self.keep(path, (sound.audio, sound.kind, lipsync, sound.seconds))
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
    files: &mut HeroFiles<'_, impl Source>,
    found: BTreeMap<String, SoundEvent>,
    sounds: &mut Sounds,
    out: &mut Out<'_>,
) -> Result<PackEvents> {
    let mixer = files.mixer()?;
    let stacks = files.stacks()?;
    let mut made = PackEvents {
        events: Vec::new(),
        index: HashMap::new(),
        resolved: Vec::new(),
    };
    for (name, event) in found {
        // An event of more sounds than any is refused before any of them is read.
        if event.files.is_empty() || event.files.len() > rules::MAX_EVENT_SOUNDS {
            out.report.failed.push((
                name,
                "a voice event of no sound, or of more sounds than any".to_owned(),
            ));
            continue;
        }
        let indices: Result<Vec<usize>> = event
            .files
            .iter()
            .map(|file| sounds.add(files, file, out))
            .collect();
        match indices {
            Ok(indices) => {
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
                let resolved = resolve_event(event, false, &stacks, &mixer, files.budget())?;
                made.resolved.push(resolved);
            }
            Err(err) => out.report.failed.push((name, err.to_string())),
        }
    }
    if sounds.full {
        return Err(malformed(
            "voice",
            "more sound than a pack holds: its events are not all kept",
        ));
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
    files: &mut HeroFiles<'_, impl Source>,
    hero: &str,
    dest: &str,
    out: &mut Out<'_>,
) -> Result<()> {
    let (class, voice_file) = hero_script(files, hero)?;
    let rules_read = read_rule_files(files, hero)?;
    let mut pruned = prune(&rules_read, &class);
    let wanted: BTreeSet<&str> = pruned
        .rules
        .iter()
        .flat_map(|r| r.lines.iter().map(|(event, _)| event.as_str()))
        .collect();
    let mut scripts = vec![voice_file];
    scripts.extend(rules_read.scripts.iter().cloned());
    let found = voice_events(files, &scripts, &wanted)?;
    let mut sounds = Sounds::default();
    let events = pack_events(files, found, &mut sounds, out)?;
    let rules = hero_rules(
        hero,
        &class,
        &rules_read,
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
/// it states none (Source's default). Its `m_bRequired` is not kept: the client requires every
/// criterion of a rule, as Dota plays a response only when all its rule's criteria are met
/// ([`crate::voice::speaker`]); of the install's 3,080 criteria only the 36 of
/// `response_rules_survivors` (an event mode's) state it, all as true.
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
            scripts: BTreeSet::new(),
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

    /// A `vsnd_c` of `audio` as Dota stores a voice line: the older binary `DATA` block, an
    /// MP3 of one channel at 22,050 Hz.
    fn vsnd(audio: &[u8]) -> Vec<u8> {
        let packed = 1 | (8 << 2) | (1 << 7) | (22_050 << 14);
        let mut data = Vec::new();
        for word in [packed, u32::MAX, 22_050, 1.0f32.to_bits(), 0, 0, 0] {
            data.extend_from_slice(&u32::to_le_bytes(word));
        }
        data.extend_from_slice(&u32::to_le_bytes(audio.len() as u32));
        data.extend_from_slice(&[0; 8]);
        let mut out = u32::to_le_bytes(28 + data.len() as u32).to_vec();
        out.extend_from_slice(&12u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        for word in [8, 1] {
            out.extend_from_slice(&u32::to_le_bytes(word));
        }
        out.extend_from_slice(b"DATA");
        out.extend_from_slice(&8u32.to_le_bytes());
        out.extend_from_slice(&u32::to_le_bytes(data.len() as u32));
        out.extend_from_slice(&data);
        out.extend_from_slice(audio);
        out
    }

    /// A package directory of `count` sounds `sounds/many/<n>.vsnd_c`, all the one stored
    /// `file` in the directory's own data section.
    fn same_sounds(count: usize, file: &[u8]) -> heatel::SliceSource<Vec<u8>> {
        let mut tree = b"vsnd_c\0sounds/many\0".to_vec();
        for n in 0..count {
            tree.extend_from_slice(format!("{n}\0").as_bytes());
            tree.extend_from_slice(&crc32fast::hash(file).to_le_bytes());
            tree.extend_from_slice(&0u16.to_le_bytes());
            tree.extend_from_slice(&0x7FFFu16.to_le_bytes());
            tree.extend_from_slice(&0u32.to_le_bytes());
            tree.extend_from_slice(&u32::to_le_bytes(file.len() as u32));
            tree.extend_from_slice(&0xFFFFu16.to_le_bytes());
        }
        tree.extend_from_slice(&[0, 0, 0]);
        let mut bytes = Vec::new();
        for word in [
            0x55AA_1234,
            2,
            tree.len() as u32,
            file.len() as u32,
            0,
            48,
            0,
        ] {
            bytes.extend_from_slice(&u32::to_le_bytes(word));
        }
        bytes.extend(tree);
        bytes.extend_from_slice(file);
        let mut src = heatel::SliceSource::default();
        src.insert("pak01_dir.vpk", bytes);
        src
    }

    struct Discard;

    impl crate::import::Sink for Discard {
        fn write(&mut self, _: &str, _: &[u8]) -> Result<()> {
            Ok(())
        }
    }

    /// The sounds kept for a pack stop at the pack's bound: each is charged before it is
    /// kept, and the one that would pass the bound is refused unread.
    #[test]
    fn sounds_past_the_packs_bound_are_refused_before_they_are_kept() {
        let src = same_sounds(12, &vsnd(&vec![7; 3 << 20]));
        let pak = heatel::Vpk::open(&src, "pak01_dir.vpk").expect("a package");
        let mut files = HeroFiles::new(&src, &pak);
        let mut report = crate::import::Report::default();
        let (mut written, mut pending) = (0, VecDeque::new());
        let mut out = Out {
            sink: &mut Discard,
            report: &mut report,
            written: &mut written,
            pending: &mut pending,
        };
        let mut sounds = Sounds::default();
        let added: Vec<Result<usize>> = (0..12)
            .map(|n| sounds.add(&mut files, &format!("sounds/many/{n}.vsnd"), &mut out))
            .collect();
        let kept: usize = sounds
            .files
            .iter()
            .map(|(audio, _, lipsync, _)| audio.len() + lipsync.as_ref().map_or(0, String::len))
            .sum();
        assert!(kept <= pack::MAX_PACK_BYTES, "{kept} bytes kept");
        let refused = added
            .iter()
            .find_map(|added| added.as_ref().err())
            .expect("a sound refused");
        assert!(
            refused.to_string().contains("more sound than a pack holds"),
            "{refused}"
        );
    }

    /// A KV3 value for the files these tests make.
    enum Kv {
        Text(String),
        List(Vec<Kv>),
        Object(Vec<(String, Kv)>),
    }

    fn text(text: &str) -> Kv {
        Kv::Text(text.to_owned())
    }

    fn object(members: Vec<(&str, Kv)>) -> Kv {
        Kv::Object(
            members
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect(),
        )
    }

    /// A compiled resource whose `DATA` block is `root` as a legacy KV3 document, stored, each
    /// text once in its table.
    fn kv3_resource(root: &Kv) -> Vec<u8> {
        fn id(table: &mut Vec<String>, text: &str) -> i32 {
            let at = table.iter().position(|t| t == text).unwrap_or_else(|| {
                table.push(text.to_owned());
                table.len() - 1
            });
            at as i32
        }
        fn write(v: &Kv, table: &mut Vec<String>, out: &mut Vec<u8>) {
            match v {
                Kv::Text(text) => {
                    out.push(6);
                    out.extend(id(table, text).to_le_bytes());
                }
                Kv::List(items) => {
                    out.push(8);
                    out.extend((items.len() as u32).to_le_bytes());
                    items.iter().for_each(|item| write(item, table, out));
                }
                Kv::Object(members) => {
                    out.push(9);
                    out.extend((members.len() as u32).to_le_bytes());
                    for (key, value) in members {
                        out.extend(id(table, key).to_le_bytes());
                        write(value, table, out);
                    }
                }
            }
        }
        let (mut table, mut body) = (Vec::new(), Vec::new());
        write(root, &mut table, &mut body);
        let mut data = 0x0356_4B56u32.to_le_bytes().to_vec();
        data.extend([
            0x00, 0x05, 0x86, 0x1B, 0xD8, 0xF7, 0xC1, 0x40, 0xAD, 0x82, 0x75, 0xA4, 0x82, 0x67,
            0xE7, 0x14,
        ]);
        data.extend([0; 16]);
        data.extend((table.len() as u32).to_le_bytes());
        for text in &table {
            data.extend(text.as_bytes());
            data.push(0);
        }
        data.extend(body);
        data.extend(u32::MAX.to_le_bytes());
        resource_of(b"DATA", &data)
    }

    /// A compiled resource of one block `kind` holding `data`.
    fn resource_of(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = u32::to_le_bytes(28 + data.len() as u32).to_vec();
        out.extend(12u16.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(8u32.to_le_bytes());
        out.extend(1u32.to_le_bytes());
        out.extend(kind);
        out.extend(8u32.to_le_bytes());
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        out
    }

    /// A package directory of `files` (`path` without its extension, extension, bytes), each in
    /// the directory's data section.
    fn package(files: &[(&str, &str, Vec<u8>)]) -> heatel::SliceSource<Vec<u8>> {
        let mut sorted: Vec<_> = files.iter().collect();
        sorted.sort_by_key(|(path, ext, _)| (*ext, path.rsplit_once('/').map(|p| p.0)));
        let (mut tree, mut section) = (Vec::<u8>::new(), Vec::<u8>::new());
        let mut last: Option<(&str, &str)> = None;
        for (path, ext, data) in sorted {
            let (dir, name) = path.rsplit_once('/').expect("a folder");
            if last.is_some_and(|(e, d)| (e, d) != (*ext, dir)) {
                tree.push(0);
                if last.is_some_and(|(e, _)| e != *ext) {
                    tree.push(0);
                }
            }
            if last.is_none_or(|(e, _)| e != *ext) {
                tree.extend(ext.as_bytes());
                tree.push(0);
            }
            if last.is_none_or(|(e, d)| (e, d) != (*ext, dir)) {
                tree.extend(dir.as_bytes());
                tree.push(0);
            }
            last = Some((ext, dir));
            tree.extend(name.as_bytes());
            tree.push(0);
            tree.extend(crc32fast::hash(data).to_le_bytes());
            tree.extend(0u16.to_le_bytes());
            tree.extend(0x7FFFu16.to_le_bytes());
            tree.extend((section.len() as u32).to_le_bytes());
            tree.extend((data.len() as u32).to_le_bytes());
            tree.extend(0xFFFFu16.to_le_bytes());
            section.extend(data);
        }
        tree.extend([0, 0, 0]);
        let mut bytes = Vec::new();
        for word in [
            0x55AA_1234,
            2,
            tree.len() as u32,
            section.len() as u32,
            0,
            48,
            0,
        ] {
            bytes.extend(u32::to_le_bytes(word));
        }
        bytes.extend(tree);
        bytes.extend(section);
        let mut src = heatel::SliceSource::default();
        src.insert("pak01_dir.vpk", bytes);
        src
    }

    /// The install's shared sound files: a mixer of one group, and stacks of one type whose
    /// limiter names `limiter` by default.
    fn shared_files(limiter: &str) -> Vec<(&'static str, &'static str, Vec<u8>)> {
        let stacks = kv3_resource(&object(vec![(
            "dota_src1_3d",
            object(vec![(
                "operators",
                Kv::List(vec![object(vec![
                    ("name", text("public")),
                    (
                        "operator_variables",
                        object(vec![(
                            "limiter_event_name",
                            object(vec![("value", text(limiter))]),
                        )]),
                    ),
                ])]),
            )]),
        )]));
        let mixer = b"<!-- kv3 -->\n{\n  MixGroups = [ { name = \"all\" path_substring = \"\" sndlvl_min = 1 priority = 30 } ]\n  SoundMixers = { Default_Mix = [ { mixgroup = \"all\" vol = 1 lvl = 1 mute = 0 } ] }\n  MixLayers = { }\n}".to_vec();
        vec![
            (
                "soundstacks/soundstacks_dota_core",
                "vsndstck_c",
                stacks.clone(),
            ),
            ("soundstacks/soundstacks_dota_music", "vsndstck_c", stacks),
            ("scripts/soundmixers", "txt", mixer),
        ]
    }

    /// Events of a stack whose limiter name is long resolve to that one name, not to a copy of
    /// it each: 64 events of a 1 MiB default keep it once.
    #[test]
    fn stack_defaults_are_shared_by_the_events_not_copied() {
        let limiter = "l".repeat(1 << 20);
        let names: Vec<String> = (0..64).map(|n| format!("e{n}")).collect();
        let script = kv3_resource(&Kv::Object(
            names
                .iter()
                .map(|name| {
                    (
                        name.clone(),
                        object(vec![
                            ("type", text("dota_src1_3d")),
                            ("vsnd_files", text("sounds/many/0.vsnd")),
                        ]),
                    )
                })
                .collect(),
        ));
        let mut files = shared_files(&limiter);
        files.push(("soundevents/voscripts/lines", "vsndevts_c", script));
        files.push(("sounds/many/0", "vsnd_c", vsnd(&[7; 64])));
        let src = package(&files);
        let pak = heatel::Vpk::open(&src, "pak01_dir.vpk").expect("a package");
        let mut hero = HeroFiles::new(&src, &pak);
        let wanted: BTreeSet<&str> = names.iter().map(String::as_str).collect();
        let scripts = ["soundevents/voscripts/lines.vsndevts_c".to_owned()];
        let found = voice_events(&mut hero, &scripts, &wanted).expect("the events");
        let mut report = crate::import::Report::default();
        let (mut written, mut pending) = (0, VecDeque::new());
        let mut out = Out {
            sink: &mut Discard,
            report: &mut report,
            written: &mut written,
            pending: &mut pending,
        };
        let made = pack_events(&mut hero, found, &mut Sounds::default(), &mut out).expect("made");
        assert_eq!(made.resolved.len(), 64);
        let kept: BTreeSet<(usize, usize)> = made
            .resolved
            .iter()
            .filter_map(|r| r.limits.limiter_event_name.as_deref())
            .map(|name| (name.as_ptr() as usize, name.len()))
            .collect();
        let bytes: usize = kept.iter().map(|(_, len)| len).sum();
        assert!(bytes <= 2 << 20, "{bytes} bytes of limiter names kept");
    }

    /// A sound whose `CTRL` block would take more than the hero has left (a table of millions
    /// of strings) is refused by the hero's budget, not decoded within a budget of its own.
    #[test]
    fn a_sounds_ctrl_block_past_the_heros_budget_is_refused() {
        // A legacy KV3 compressed with LZ4: the table's count, its 8M empty strings (one match
        // of the zero before them), an empty object.
        let count: u32 = 8 << 20;
        let tail = [9, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF];
        let mut block = vec![(5 << 4) | 15];
        block.extend(count.to_le_bytes());
        block.push(0);
        block.extend(1u16.to_le_bytes());
        let mut more = count as usize - 1 - 4 - 15;
        while more >= 255 {
            block.push(255);
            more -= 255;
        }
        block.push(more as u8);
        block.push((tail.len() as u8) << 4);
        block.extend(tail);
        let mut ctrl = 0x0356_4B56u32.to_le_bytes().to_vec();
        ctrl.extend([
            0x8A, 0x34, 0x47, 0x68, 0xA1, 0x63, 0x5C, 0x4F, 0xA1, 0x97, 0x53, 0x80, 0x6F, 0xD9,
            0xB1, 0x19,
        ]);
        ctrl.extend([0; 16]);
        ctrl.extend((4 + count + tail.len() as u32).to_le_bytes());
        ctrl.extend(block);
        let src = package(&[("sounds/many/0", "vsnd_c", resource_of(b"CTRL", &ctrl))]);
        let pak = heatel::Vpk::open(&src, "pak01_dir.vpk").expect("a package");
        let mut hero = HeroFiles::new(&src, &pak);
        let mut report = crate::import::Report::default();
        let (mut written, mut pending) = (0, VecDeque::new());
        let mut out = Out {
            sink: &mut Discard,
            report: &mut report,
            written: &mut written,
            pending: &mut pending,
        };
        let error = Sounds::default()
            .add(&mut hero, "sounds/many/0.vsnd", &mut out)
            .unwrap_err();
        assert!(error.to_string().contains("more than it may"), "{error}");
    }

    /// The voice importer reads through [`HeroFiles`] alone, and it holds one budget, the
    /// hero's: outside its tests the importer names no package, no reader of heatel's and no
    /// budget, and `HeroFiles` makes one.
    #[test]
    fn the_voice_importer_reads_within_the_heros_budget_alone() {
        let code = |text: &'static str| text.split("#[cfg(test)]").next().unwrap_or(text);
        let importer = code(include_str!("voice.rs"));
        let files = code(include_str!("voice/files.rs"));
        let budget = ["Budget", "::"].concat();
        let readers = [
            budget.as_str(),
            "Vpk",
            "read_from",
            "Resource::",
            "Sound::",
            "Lipsync::",
            "read_sound_",
            "read_response_rules",
            "kv3::",
        ];
        for name in readers {
            assert!(!importer.contains(name), "the importer names {name}");
        }
        assert_eq!(files.matches(budget.as_str()).count(), 1, "one budget");
    }
}
