//! What a tooltip says, before it is drawn: Dota's text with the
//! simulator's numbers in it, or, without the cache, only the simulator's own
//! plain names and numbers.

use bevy::prelude::Color;
use bota_core::{AbilityDef, ItemDef, ability_def, item_def};
use bota_proto::{AbilityId, ItemId};

use crate::dota_text::{Entry, Texts, Token, tokens};
use crate::manifest::Manifest;
use crate::tip_numbers::{self, Levels, Subject};

/// How a piece of text is coloured; the drawing picks the colour from this
/// and from the kind of section it sits in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tone {
    /// The prose.
    Body,
    /// A number, or the level of one that counts now.
    Value,
    /// The other levels of a number.
    Dim,
    /// The name of a number: the stat it adds, the label of a line.
    Label,
    Strength,
    Agility,
    Intelligence,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub text: String,
    pub tone: Tone,
    pub bold: bool,
    pub color: Option<Color>,
}

impl Run {
    fn new(text: impl Into<String>, tone: Tone) -> Run {
        Run {
            text: text.into(),
            tone,
            bold: false,
            color: None,
        }
    }
}

/// The look of a section of an item: Dota gives each its own bar and tint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Passive,
    Active,
    /// A consumable's use.
    Use,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub kind: Kind,
    pub header: Option<String>,
    pub body: Vec<Run>,
}

/// Which of the small icons leads a figure of the cost row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CostIcon {
    Cooldown,
    Mana,
    Health,
}

impl CostIcon {
    fn logical(self) -> &'static str {
        match self {
            CostIcon::Cooldown => "tip.cooldown",
            CostIcon::Mana => "tip.mana",
            CostIcon::Health => "tip.health",
        }
    }

    fn word(self) -> &'static str {
        match self {
            CostIcon::Cooldown => "Cooldown",
            CostIcon::Mana => "Mana",
            CostIcon::Health => "Health",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cost {
    /// The icon's asset path, when the cache has it.
    pub icon: Option<String>,
    /// What stands for the icon without one.
    pub word: &'static str,
    pub figures: Vec<Run>,
}

/// A whole tooltip.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Content {
    pub title: String,
    /// The icon's asset path, when the cache has it.
    pub icon: Option<String>,
    pub is_item: bool,
    /// "Level 2" at the title's right.
    pub level: Option<String>,
    /// Gold, for an item.
    pub price: Option<String>,
    /// The stats an item adds, a line each.
    pub bonuses: Vec<Vec<Run>>,
    pub sections: Vec<Section>,
    /// An ability's labelled numbers, a line each.
    pub stats: Vec<Vec<Run>>,
    pub costs: Vec<Cost>,
    pub notes: Vec<Vec<Run>>,
    pub lore: String,
    /// What the client adds about what to do with it, and what it holds now.
    pub hints: Vec<String>,
    /// Whether the title shows as written (a buff's, as Dota's buff tooltip
    /// has it) rather than in capitals (an ability's or an item's).
    pub as_written: bool,
}

/// How well a search finds an item; the better first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    /// A name starts with the search.
    Starts,
    /// A name holds it further in.
    Contains,
}

/// The sources a tooltip is made from.
pub struct Lookup<'a> {
    pub texts: &'a Texts,
    pub manifest: &'a Manifest,
}

impl Lookup<'_> {
    /// The name of an item: Dota's, or the simulator's.
    pub fn item_name(&self, id: u16) -> Option<String> {
        let def = item_def(ItemId(id))?;
        Some(
            self.entry("item", id)
                .map_or_else(|| def.name.to_string(), |e| e.name.clone()),
        )
    }

    /// What an item is built from, in names: "Built from: A, 2x B".
    pub fn parts_line(&self, id: u16) -> Option<String> {
        let def = item_def(ItemId(id))?;
        if def.components.is_empty() {
            return None;
        }
        let mut seen: Vec<(u16, usize)> = Vec::new();
        for part in def.components {
            match seen.iter_mut().find(|(p, _)| *p == part.0) {
                Some((_, n)) => *n += 1,
                None => seen.push((part.0, 1)),
            }
        }
        let names: Vec<String> = seen
            .into_iter()
            .filter_map(|(part, n)| {
                let name = self.item_name(part)?;
                Some(if n > 1 { format!("{n}x {name}") } else { name })
            })
            .collect();
        Some(format!("Built from: {}", names.join(", ")))
    }

    /// How well a shop search, lower-cased and trimmed, finds an item, as
    /// Dota's does: by its name or one of its aliases. A name that starts
    /// with the search finds it best; nothing when none holds it.
    pub fn search_rank(&self, id: u16, query: &str) -> Option<Rank> {
        let def = item_def(ItemId(id))?;
        let entry = self.entry("item", id);
        let names = std::iter::once(def.name)
            .chain(entry.map(|e| e.name.as_str()))
            .chain(
                entry
                    .into_iter()
                    .flat_map(|e| e.aliases.iter().map(String::as_str)),
            );
        names
            .filter_map(|name| {
                let name = name.to_lowercase();
                if name.starts_with(query) {
                    Some(Rank::Starts)
                } else {
                    name.contains(query).then_some(Rank::Contains)
                }
            })
            .min()
    }

    fn entry(&self, kind: &str, id: u16) -> Option<&Entry> {
        self.texts.get(&self.manifest.text_key(kind, id)?)
    }

    fn icon(&self, logical: &str) -> Option<String> {
        self.manifest.file(logical)
    }

    /// The tooltip of an item. `price` is what it costs this buyer now, when
    /// the buyer is in a shop.
    pub fn item(&self, id: u16, price: Option<i32>) -> Option<Content> {
        let def = item_def(ItemId(id))?;
        let subject = Subject::Item(def);
        let entry = self.entry("item", id);
        let mut content = Content {
            title: entry.map_or_else(|| def.name.to_string(), |e| e.name.clone()),
            icon: self.manifest.item_icon(id),
            is_item: true,
            price: (def.cost > 0 || price.is_some()).then(|| price.unwrap_or(def.cost).to_string()),
            ..Content::default()
        };
        match entry {
            Some(entry) => self.dota_body(&mut content, entry, subject, None),
            None => plain_item(&mut content, def),
        }
        self.costs(&mut content, subject, def.health_cost);
        Some(content)
    }

    /// The tooltip of an ability at a level of its owner's learning.
    pub fn ability(&self, id: u16, level: u8, max_level: u8) -> Option<Content> {
        let def = ability_def(AbilityId(id))?;
        let subject = Subject::Ability(AbilityId(id), def);
        let entry = self.entry("ability", id);
        let mut content = Content {
            title: entry.map_or_else(|| def.name.to_string(), |e| e.name.clone()),
            icon: self.manifest.ability_icon(id),
            level: (max_level > 1).then(|| format!("Level {level}")),
            ..Content::default()
        };
        let current = usize::from(level).checked_sub(1);
        match entry {
            Some(entry) => self.dota_body(&mut content, entry, subject, current),
            None => plain_ability(&mut content, def),
        }
        self.costs_at(&mut content, subject, 0, current);
        Some(content)
    }

    fn costs(&self, content: &mut Content, subject: Subject, health: i32) {
        self.costs_at(content, subject, health, None);
    }

    fn costs_at(
        &self,
        content: &mut Content,
        subject: Subject,
        health: i32,
        current: Option<usize>,
    ) {
        let rows = [
            (CostIcon::Cooldown, tip_numbers::cooldown(subject)),
            (CostIcon::Mana, tip_numbers::mana(subject)),
            (
                CostIcon::Health,
                if health > 0 {
                    vec![health.to_string()]
                } else {
                    Vec::new()
                },
            ),
        ];
        for (icon, figures) in rows {
            if figures.iter().all(|n| n == "0") {
                continue;
            }
            let mut runs = Vec::new();
            value_runs(&figures, current, &mut runs);
            content.costs.push(Cost {
                icon: self.icon(icon.logical()),
                word: icon.word(),
                figures: runs,
            });
        }
    }

    fn dota_body(
        &self,
        content: &mut Content,
        entry: &Entry,
        subject: Subject,
        current: Option<usize>,
    ) {
        let resolve = |key: &str| number_of(entry, subject, key);
        let is_item = content.is_item;
        for (key, label) in &entry.labels {
            let Some(figures) = resolve(key) else {
                continue;
            };
            if figures.iter().all(|n| n == "0") {
                continue;
            }
            let line = if label.starts_with('+') || label.starts_with("%+") {
                bonus_line(label, &figures)
            } else if matches!(key.as_str(), "abilitycooldown" | "abilitycastrange") {
                continue;
            } else {
                stat_line(label, &figures, current)
            };
            if label.starts_with('+') || label.starts_with("%+") {
                content.bonuses.push(line);
            } else {
                content.stats.push(line);
            }
        }
        content.sections = sections(&entry.description, &resolve, current, is_item);
        content.notes = entry
            .notes
            .iter()
            .map(|note| inline(note, &resolve, current))
            .collect();
        content.lore = entry.lore.clone();
    }
}

/// Dota's number for a key, with the simulator's put over it. The flag tells
/// the number is Dota's own because the simulator has none under that key.
pub fn number_of(entry: &Entry, subject: Subject, key: &str) -> Option<Levels> {
    tip_numbers::value(subject, key).or_else(|| entry.values.get(key).cloned())
}

/// A number per level as runs: the current level bright, the others dim.
fn value_runs(figures: &[String], current: Option<usize>, out: &mut Vec<Run>) {
    for (at, figure) in figures.iter().enumerate() {
        if at > 0 {
            out.push(Run::new(" / ", Tone::Dim));
        }
        let lit = figures.len() == 1 || current.is_none_or(|c| c == at);
        out.push(Run::new(figure, if lit { Tone::Value } else { Tone::Dim }));
    }
}

/// "+10 Strength": a bonus label of Dota's with the number in front of it.
fn bonus_line(label: &str, figures: &[String]) -> Vec<Run> {
    let percent = label.starts_with('%');
    let rest = label.trim_start_matches('%').trim_start_matches('+');
    let mut line = vec![Run::new(format!("+{}", figures.join(" / ")), Tone::Value)];
    if percent {
        line.push(Run::new("%", Tone::Value));
    }
    let tone = match rest.split_whitespace().next() {
        Some("Strength") => Tone::Strength,
        Some("Agility") => Tone::Agility,
        Some("Intelligence") => Tone::Intelligence,
        _ => Tone::Label,
    };
    line.push(Run::new(" ", Tone::Label));
    for mut run in markup_runs(rest, &|_| None, None, tone) {
        run.bold = false;
        line.push(run);
    }
    line
}

/// "DAMAGE: 150 / 220 / 290 / 360".
fn stat_line(label: &str, figures: &[String], current: Option<usize>) -> Vec<Run> {
    let percent = label.starts_with('%');
    let mut line = vec![Run::new(
        format!("{} ", label.trim_start_matches('%')),
        Tone::Label,
    )];
    let shown: Vec<String> = figures
        .iter()
        .map(|n| n.trim_start_matches('-').to_string())
        .collect();
    value_runs(&shown, current, &mut line);
    if percent {
        line.push(Run::new("%", Tone::Value));
    }
    line
}

/// The description cut at its `<h1>` headings.
fn sections(
    description: &str,
    resolve: &dyn Fn(&str) -> Option<Levels>,
    current: Option<usize>,
    is_item: bool,
) -> Vec<Section> {
    let mut out = vec![Section {
        kind: Kind::Passive,
        header: None,
        body: Vec::new(),
    }];
    let mut header: Option<String> = None;
    let (mut bold, mut color) = (false, None);
    for token in tokens(description) {
        let section = out.last_mut().expect("a section stands");
        match token {
            Token::HeaderOpen => {
                out.push(Section {
                    kind: Kind::Passive,
                    header: None,
                    body: Vec::new(),
                });
                header = Some(String::new());
            }
            Token::HeaderClose => {
                if let Some(text) = header.take() {
                    section.kind = kind_of(&text);
                    section.header = Some(text.trim().to_string());
                }
            }
            _ if header.is_some() => {
                if let (Some(h), Token::Text(t)) = (header.as_mut(), &token) {
                    h.push_str(t);
                }
            }
            Token::Bold(on) => bold = on,
            Token::Font(c) => color = c,
            token => push_token(
                &mut section.body,
                token,
                resolve,
                current,
                bold,
                color,
                Tone::Body,
            ),
        }
    }
    for section in &mut out {
        trim(&mut section.body);
    }
    out.retain(|s| s.header.is_some() || !s.body.is_empty());
    if !is_item {
        for section in &mut out {
            section.kind = Kind::Passive;
        }
    }
    out
}

fn kind_of(header: &str) -> Kind {
    let header = header.trim();
    if header.starts_with("Use") {
        Kind::Use
    } else if header.starts_with("Active") {
        Kind::Active
    } else {
        Kind::Passive
    }
}

/// A string of markup as one block of runs.
fn inline(
    source: &str,
    resolve: &dyn Fn(&str) -> Option<Levels>,
    current: Option<usize>,
) -> Vec<Run> {
    let mut runs = markup_runs(source, resolve, current, Tone::Body);
    trim(&mut runs);
    runs
}

fn markup_runs(
    source: &str,
    resolve: &dyn Fn(&str) -> Option<Levels>,
    current: Option<usize>,
    tone: Tone,
) -> Vec<Run> {
    let mut out = Vec::new();
    let (mut bold, mut color) = (false, None);
    for token in tokens(source) {
        match token {
            Token::Bold(on) => bold = on,
            Token::Font(c) => color = c,
            token => push_token(&mut out, token, resolve, current, bold, color, tone),
        }
    }
    out
}

fn push_token(
    out: &mut Vec<Run>,
    token: Token<'_>,
    resolve: &dyn Fn(&str) -> Option<Levels>,
    current: Option<usize>,
    bold: bool,
    color: Option<Color>,
    tone: Tone,
) {
    match token {
        Token::Text(t) => out.push(Run {
            text: t.to_string(),
            tone,
            bold,
            color,
        }),
        Token::Break => out.push(Run::new("\n", tone)),
        Token::Value(key) => match resolve(&key) {
            Some(figures) => value_runs(&figures, current, out),
            None => out.push(Run::new("?", Tone::Value)),
        },
        _ => {}
    }
}

/// Drops the blank space at both ends of a block.
fn trim(runs: &mut Vec<Run>) {
    while runs.first().is_some_and(|r| r.text.trim().is_empty()) {
        runs.remove(0);
    }
    while runs.last().is_some_and(|r| r.text.trim().is_empty()) {
        runs.pop();
    }
    if let Some(first) = runs.first_mut() {
        first.text = first.text.trim_start().to_string();
    }
    if let Some(last) = runs.last_mut() {
        last.text = last.text.trim_end().to_string();
    }
}

/// An item without Dota's text: what it adds, in plain words.
fn plain_item(content: &mut Content, def: &'static ItemDef) {
    let subject = Subject::Item(def);
    let all = tip_numbers::value(subject, "bonus_all_stats").is_some();
    let stats: [(&str, &str); 14] = [
        ("bonus_all_stats", "All Attributes"),
        ("bonus_strength", "Strength"),
        ("bonus_agility", "Agility"),
        ("bonus_intellect", "Intelligence"),
        ("bonus_damage", "Damage"),
        ("bonus_attack_speed", "Attack Speed"),
        ("bonus_armor", "Armor"),
        ("bonus_health", "Health"),
        ("bonus_mana", "Mana"),
        ("bonus_health_regen", "Health Regeneration"),
        ("bonus_mana_regen", "Mana Regeneration"),
        ("bonus_movement_speed", "Movement Speed"),
        ("movement_speed", "Movement Speed"),
        ("bonus_evasion", "% Evasion"),
    ];
    for (key, label) in stats {
        if all && matches!(key, "bonus_strength" | "bonus_agility" | "bonus_intellect") {
            continue;
        }
        if let Some(figures) = tip_numbers::value(subject, key)
            && figures.iter().any(|n| n != "0")
        {
            content.bonuses.push(vec![Run::new(
                format!("+{} {label}", figures.join(" / ")),
                Tone::Value,
            )]);
        }
    }
    if def.cast_range > 0 {
        content.stats.push(vec![Run::new(
            format!("Range {}", def.cast_range),
            Tone::Value,
        )]);
    }
    if def.charges > 1 {
        content.stats.push(vec![Run::new(
            format!("{} charges", def.charges),
            Tone::Value,
        )]);
    }
}

/// An ability without Dota's text.
fn plain_ability(content: &mut Content, def: &'static AbilityDef) {
    if def.range > 0 {
        content
            .stats
            .push(vec![Run::new(format!("Range {}", def.range), Tone::Value)]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether two lists of numbers state the same quantities.
    fn same(a: &[String], b: &[String]) -> bool {
        let value = |s: &String| s.parse::<f64>().ok();
        a.len() == b.len()
            && a.iter().zip(b).all(|(x, y)| match (value(x), value(y)) {
                (Some(x), Some(y)) => (x - y).abs() < 0.006,
                _ => x == y,
            })
    }

    /// Dota's numbers against the simulator's, for every item and ability
    /// of the cache's text: prints where they differ and where the
    /// simulator has no number of its own (`cargo test -- --ignored
    /// --nocapture audit`). The text never lands in a repository, so this
    /// needs the cache `dbota import` builds.
    #[test]
    #[ignore = "needs the asset cache"]
    fn audit_dota_numbers_against_the_simulator() {
        let root = std::env::var_os("DBOTA_ASSETS").map_or_else(
            || {
                let home = std::env::var_os("HOME").expect("HOME");
                std::path::PathBuf::from(home)
                    .join(".cache/dbota")
                    .join(dbota_import::import::folder_name())
            },
            std::path::PathBuf::from,
        );
        let manifest = Manifest::load(&root);
        let texts = Texts::load(manifest.root.as_deref());
        assert!(!texts.is_empty(), "no text file in {}", root.display());
        let mut unanswered = 0;
        for (kind, id, _) in &manifest.ids {
            let subject = match kind.as_str() {
                "item" => item_def(ItemId(*id)).map(Subject::Item),
                "ability" => {
                    ability_def(AbilityId(*id)).map(|d| Subject::Ability(AbilityId(*id), d))
                }
                _ => None,
            };
            let (Some(subject), Some(key)) = (subject, manifest.text_key(kind, *id)) else {
                continue;
            };
            let Some(entry) = texts.get(&key) else {
                continue;
            };
            let mut keys: Vec<String> = entry.labels.iter().map(|(k, _)| k.clone()).collect();
            for text in std::iter::once(&entry.description).chain(&entry.notes) {
                keys.extend(tokens(text).into_iter().filter_map(|t| match t {
                    Token::Value(k) => Some(k),
                    _ => None,
                }));
            }
            keys.sort();
            keys.dedup();
            let own = [
                (
                    "cost",
                    entry.price.clone(),
                    match subject {
                        Subject::Item(def) => vec![def.cost.to_string()],
                        Subject::Ability(..) => Vec::new(),
                    },
                ),
                (
                    "cooldown",
                    entry.cooldown.clone(),
                    tip_numbers::cooldown(subject),
                ),
                ("mana", entry.mana.clone(), tip_numbers::mana(subject)),
            ];
            for (what, theirs, ours) in own {
                let theirs = tip_numbers::collapse(&theirs);
                let ours = if ours == ["0"] { Vec::new() } else { ours };
                let theirs = if theirs == ["0"] { Vec::new() } else { theirs };
                if !same(&ours, &theirs) {
                    println!("DIFFERS {key} {what}: bota {ours:?}, Dota {theirs:?}");
                }
            }
            for k in keys {
                let theirs = entry.values.get(&k);
                match (tip_numbers::value(subject, &k), theirs) {
                    (Some(ours), Some(theirs)) if !same(&ours, theirs) => {
                        println!("DIFFERS {key} {k}: bota {ours:?}, Dota {theirs:?}");
                    }
                    (None, Some(theirs)) => {
                        println!("DOTA ONLY {key} {k}: {theirs:?}");
                    }
                    (None, None) => {
                        unanswered += 1;
                        println!("UNANSWERED {key} {k}");
                    }
                    _ => {}
                }
            }
        }
        assert_eq!(unanswered, 0, "a placeholder has no number at all");
    }
}
