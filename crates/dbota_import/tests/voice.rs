//! The heroes' voice packs out of the install: every response line of a hero's rules (its own
//! file and the shared one it includes) is kept or pruned under one reason, with the counts of
//! Dota 7.41f, and what is kept is what a query of the client can match. Needs the install, so
//! it runs only when asked (the gate's install-tests step):
//!
//! ```sh
//! cargo test --release -p dbota_import --test voice -- --ignored
//! ```

use std::collections::BTreeMap;

use dbota_import::import::{Flow, Item, Plan, Sink, run};
use dbota_import::source::InstallSource;
use dbota_import::voice::expr::Matcher;
use dbota_import::voice::{self, pack_path};
use heatel::find_game_dir;

#[derive(Default)]
struct Files(BTreeMap<String, Vec<u8>>);

impl Sink for Files {
    fn write(&mut self, path: &str, data: &[u8]) -> heatel::Result<()> {
        self.0.insert(path.to_owned(), data.to_vec());
        Ok(())
    }
}

/// The packs of `heroes`, made from the install, and the import's notes.
fn packs(heroes: &[&str]) -> (Files, Vec<String>) {
    let dota = std::env::var_os("DBOTA_DOTA_PATH").map(std::path::PathBuf::from);
    let game = find_game_dir(dota.as_deref()).expect("Dota 2 not found; set DBOTA_DOTA_PATH");
    let plan = Plan {
        items: heroes
            .iter()
            .map(|hero| Item::Voice {
                logical: format!("voice.{hero}"),
                hero: (*hero).to_owned(),
                out: pack_path(hero),
            })
            .collect(),
    };
    let mut files = Files::default();
    let report = run(InstallSource::new(&game), &plan, &mut files, &mut |_| {
        Flow::Continue
    })
    .expect("an import");
    assert!(report.failed.is_empty(), "{:#?}", report.failed);
    (files, report.notes)
}

/// The lines pruned by why, and the lines kept, of a hero's pack.
fn counts(files: &Files, hero: &str) -> (BTreeMap<String, u32>, usize) {
    let bytes = &files.0[&pack_path(hero)];
    let voice = voice::open(hero, bytes).expect("the pack opens");
    (voice.rules.pruned.clone(), voice.rules.lines())
}

/// The values a fact takes in the simulator's catalogue, when it is one it lists (an item's or a
/// rival's), as the importer asks the same catalogue; nothing for any other fact.
fn catalogue(key: &str) -> Vec<&'static str> {
    match key {
        "itemname" => bota_core::ITEMS.iter().map(|item| item.key).collect(),
        "targetclassname" => bota_core::HEROES.iter().map(|hero| hero.key).collect(),
        _ => Vec::new(),
    }
}

/// The prune counts of Pudge and Shadow Fiend. The concepts not raised are counted by the
/// install's own rule files alone (every line of a rule whose concept is one of these), so they
/// also check the importer's reading: Pudge's 2,215 lines and Shadow Fiend's 545 (each with the
/// shared file's two).
#[test]
#[ignore = "needs a local Dota 2 install"]
fn every_line_of_a_heros_rules_is_kept_or_pruned_under_one_reason() {
    let (files, notes) = packs(&["pudge", "nevermore"]);
    println!("{notes:#?}");
    for (hero, total, not_raised) in [
        (
            "pudge",
            2215,
            &[
                ("TLK_DOTA_ALLY_NEARBY", 196),
                ("TLK_DOTA_CAST_ORDER", 17),
                ("TLK_DOTA_COOLDOWN", 24),
                ("TLK_DOTA_EMOTE", 33),
                ("TLK_DOTA_ENEMY_NEARBY", 5),
                ("TLK_DOTA_IMMORTALITY", 10),
                ("TLK_DOTA_INTHEBAG", 12),
                ("TLK_DOTA_ITEMDROP", 10),
                ("TLK_DOTA_LANE_MISSING", 9),
                ("TLK_DOTA_LEARN", 17),
                ("TLK_DOTA_NO_MANA", 24),
                ("TLK_DOTA_PAIN", 30),
                ("TLK_DOTA_SELECT", 83),
                ("TLK_DOTA_TAUNT", 15),
                ("TLK_DOTA_THANKS", 10),
            ][..],
        ),
        (
            "nevermore",
            545,
            &[
                ("TLK_DOTA_CAST_ORDER", 6),
                ("TLK_DOTA_COOLDOWN", 12),
                ("TLK_DOTA_EMOTE", 16),
                ("TLK_DOTA_IMMORTALITY", 4),
                ("TLK_DOTA_INTHEBAG", 2),
                ("TLK_DOTA_ITEMDROP", 6),
                ("TLK_DOTA_LANE_MISSING", 6),
                ("TLK_DOTA_LEARN", 8),
                ("TLK_DOTA_NO_MANA", 12),
                ("TLK_DOTA_PAIN", 20),
                ("TLK_DOTA_SELECT", 10),
                ("TLK_DOTA_TAUNT", 7),
                ("TLK_DOTA_THANKS", 4),
            ][..],
        ),
    ] {
        let (pruned, kept) = counts(&files, hero);
        let dropped: u32 = pruned.values().sum();
        assert_eq!(kept as u32 + dropped, total, "{hero}: {pruned:#?}");
        let raised: Vec<(&str, u32)> = pruned
            .iter()
            .filter_map(|(why, n)| Some((why.strip_prefix("concept not raised: ")?, *n)))
            .collect();
        assert_eq!(raised, not_raised, "{hero}");
    }
}

/// The rest of the prune counts and what is kept, as Dota 7.41f's rules give them: the
/// cosmetics' lines (`customresponse`), the events the client has no fact of, and the rivals and
/// items the game lacks.
///
/// The first kinds follow from the install's rule files alone and are pinned exactly, with the
/// number of lines that survive them (kept plus absent). Which of those survivors are absent
/// is the simulator's catalogue's to say (`bota_core::ITEMS`, `HEROES`: a line moves to kept the
/// day its item exists), so it is asked of the catalogue here instead of pinned: every absent
/// reason names the value the rule asks for and the catalogue must hold nothing that matches it,
/// and every kept rule's item or rival the catalogue must hold. A line kept or dropped wrongly
/// fails whatever the catalogue's size.
#[test]
#[ignore = "needs a local Dota 2 install"]
fn the_heros_lines_kept_and_pruned_are_the_installs() {
    let (files, _) = packs(&["pudge", "nevermore"]);
    for (hero, facts, surviving) in [
        (
            "pudge",
            &[
                ("fact not supplied: abilityname", 26),
                ("fact not supplied: customresponse", 1443),
                ("fact not supplied: expensive_item", 5),
                ("fact not supplied: hookedtarget", 27),
                ("fact not supplied: reincarnating", 2),
                ("fact not supplied: special_spawn", 27),
                ("fact not supplied: tease", 2),
            ][..],
            159 + 10 + 19,
        ),
        (
            "nevermore",
            &[
                ("fact not supplied: customresponse", 205),
                ("fact not supplied: expensive_item", 4),
                ("fact not supplied: reincarnating", 2),
                ("fact not supplied: shadowraze", 9),
                ("fact not supplied: special_spawn", 24),
                ("fact not supplied: tease", 2),
            ][..],
            166 + 9 + 11,
        ),
    ] {
        let (pruned, kept) = counts(&files, hero);
        const ABSENT: &str = "value absent from the game: ";
        let installs: Vec<(&str, u32)> = pruned
            .iter()
            .filter(|(why, _)| !why.starts_with("concept not raised: ") && !why.starts_with(ABSENT))
            .map(|(why, n)| (why.as_str(), *n))
            .collect();
        assert_eq!(installs, facts, "{hero}");
        let mut absent = 0;
        for (why, lines) in pruned
            .iter()
            .filter_map(|(why, n)| Some((why.strip_prefix(ABSENT)?, n)))
        {
            let (key, expr) = why
                .split_once(" = ")
                .expect("an absent value names what was asked");
            let matcher = Matcher::parse(expr);
            let present: Vec<&str> = catalogue(key)
                .into_iter()
                .filter(|value| matcher.matches(Some(value)))
                .collect();
            assert!(
                present.is_empty(),
                "{hero}: {lines} lines are dropped for a {key} ({expr}) the game lacks, but it has {present:?}"
            );
            absent += lines;
        }
        let voice = voice::open(hero, &files.0[&pack_path(hero)]).expect("the pack opens");
        for rule in &voice.rules.rules {
            for &index in &rule.criteria {
                let criterion = &voice.rules.criteria[index];
                let values = catalogue(&criterion.key.to_ascii_lowercase());
                if !values.is_empty() {
                    assert!(
                        values
                            .iter()
                            .any(|value| criterion.matcher.matches(Some(value))),
                        "{hero}: {} is kept, but the game has no {} for {:?}",
                        rule.name,
                        criterion.key,
                        criterion.expr
                    );
                }
            }
        }
        assert_eq!(kept as u32 + absent, surviving, "{hero}: {pruned:#?}");
    }
    let voice = voice::open("pudge", &files.0[&pack_path("pudge")]).expect("Pudge's pack");
    let moves = voice
        .rules
        .rules
        .iter()
        .find(|r| r.name == "pudge_Move")
        .expect("Pudge's move rule kept");
    assert_eq!(moves.lines.len(), 23);
    let names: Vec<&str> = moves
        .criteria
        .iter()
        .map(|&c| voice.rules.criteria[c].name.as_str())
        .collect();
    assert_eq!(names, ["Move", "Ispudge"]);
    assert!(
        !voice
            .rules
            .rules
            .iter()
            .any(|r| r.name == "pudge_MoveIsPudgeChain"),
        "the arcana's move lines are pruned"
    );
}
