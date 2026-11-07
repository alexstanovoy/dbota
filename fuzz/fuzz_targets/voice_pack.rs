//! A hero's voice pack as the client reads it (`dbota_import::voice::open`: the records, the
//! rules and their bounds), and the rules it holds answering queries: every index the rules
//! hold stays inside its list, a history rewound answers again as it did, and no input panics.
//! The input's first byte picks the hero the pack must be of; the rest is the pack.
#![no_main]

use dbota_import::voice::concept::{Concept, Fact, Query};
use dbota_import::voice::expr::Matcher;
use dbota_import::voice::speaker::History;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&pick, pack)) = data.split_first() else {
        return;
    };
    let hero = if pick % 2 == 0 { "pudge" } else { "nevermore" };
    // The matcher over any text: a criterion's expression and a fact are both untrusted.
    if let Ok(text) = std::str::from_utf8(pack) {
        // Cut at a character's boundary: a text cut inside one is no text.
        let cut = (0..=text.len() / 2)
            .rev()
            .find(|&at| text.is_char_boundary(at))
            .unwrap_or(0);
        let (expr, value) = text.split_at(cut);
        let matcher = Matcher::parse(expr);
        let _ = matcher.matches(Some(value));
        let _ = matcher.matches(None);
    }
    let Ok(voice) = dbota_import::voice::open(hero, pack) else {
        return;
    };
    let rules = &voice.rules;
    // Asks every concept twice, accepting what is chosen; a history rewound to the middle answers
    // the second half again as it did.
    let ask_all = |history: &mut History, from: usize| -> Vec<Option<(usize, usize, usize)>> {
        let concepts = Concept::ALL.into_iter().chain(Concept::ALL);
        concepts
            .enumerate()
            .skip(from)
            .map(|(at, concept)| {
                let now = at as f64 * 2.0;
                let (n, query, said) = history.ask(rules, now, |random| {
                    Query::new(concept, &rules.class, random, at as f32)
                        .with(Fact::AbilityName, "pudge_rot")
                        .with(Fact::Level, "2")
                });
                let choice = said.ok()?;
                let line = &rules.rules[choice.rule].lines[choice.line];
                assert!(rules.events[line.event].sounds.contains(&choice.sound));
                assert!(choice.sound < voice.sounds.len());
                history.accept(rules, n, query.concept, now);
                Some((choice.rule, choice.line, choice.sound))
            })
            .collect()
    };
    let mut history = History::new(rules, u64::from(pick));
    let once = ask_all(&mut history, 0);
    let half = Concept::ALL.len();
    history.rewind(rules, (half - 1) as f64 * 2.0);
    assert_eq!(ask_all(&mut history, half), once[half..]);
});
