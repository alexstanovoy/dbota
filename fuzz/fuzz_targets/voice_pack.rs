//! A hero's voice pack as the client reads it (`dbota_import::voice::open`: the records, the
//! rules and their bounds), and the rules it holds answering queries: every index the rules
//! hold stays inside its list, and no input panics. The input's first byte picks the hero the
//! pack must be of; the rest is the pack.
#![no_main]

use dbota_import::voice::concept::{Concept, Fact, Query};
use dbota_import::voice::expr::Matcher;
use dbota_import::voice::speaker::Speaker;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&pick, pack)) = data.split_first() else {
        return;
    };
    let hero = if pick % 2 == 0 { "pudge" } else { "nevermore" };
    // The matcher over any text: a criterion's expression and a fact are both untrusted.
    if let Ok(text) = std::str::from_utf8(pack) {
        let (expr, value) = text.split_at(text.len() / 2);
        let matcher = Matcher::parse(expr);
        let _ = matcher.matches(Some(value));
        let _ = matcher.matches(None);
    }
    let Ok(voice) = dbota_import::voice::open(hero, pack) else {
        return;
    };
    let rules = &voice.rules;
    let mut speaker = Speaker::new(rules, u64::from(pick));
    for (at, concept) in Concept::ALL.into_iter().enumerate() {
        let query = Query::new(concept, &rules.class, u32::from(pick), at as f32)
            .with(Fact::AbilityName, "pudge_rot")
            .with(Fact::Level, "2");
        if let Ok(choice) = speaker.respond(rules, &query, at as f64 * 10.0) {
            let line = &rules.rules[choice.rule].lines[choice.line];
            assert!(rules.events[line.event].sounds.contains(&choice.sound));
            assert!(choice.sound < voice.sounds.len());
        }
    }
});
