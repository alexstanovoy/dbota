//! A hero's response rules as its pack keeps them (`rules.json`): the criteria its rules use,
//! the rules that can match a query of the client ([`super::concept`]), each with the lines it
//! picks among and their own `speakonce`, `respeakdelay` and `predelay`, the voice events those
//! lines play and how many lines were pruned at import, by why.
//!
//! What a pack says is untrusted: every list, name and number is bounded, every index points
//! inside its list.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::expr::Matcher;

/// Most criteria a hero's rules use: its own file and the shared one define 1,096 for Pudge
/// (83 + 1,013); no file of the install defines more than 1,013.
pub const MAX_CRITERIA: usize = 4096;
/// Most rules kept: no file of the install has more than 1,009 groups.
pub const MAX_RULES: usize = 4096;
/// Most criteria one rule requires: nine at most in the install.
pub const MAX_RULE_CRITERIA: usize = 32;
/// Most lines one rule picks among: 102 at most in the install.
pub const MAX_RULE_LINES: usize = 512;
/// Most voice events: Pudge's voice script, the largest, names 2,099.
pub const MAX_EVENTS: usize = 8192;
/// Most sound files one event picks among (one in every voice event of the install).
pub const MAX_EVENT_SOUNDS: usize = 16;
/// Longest name, key or expression: 180 bytes at most in the install.
pub const MAX_TEXT: usize = 256;
/// Most seconds of a line's delay, its respeak or its length: 600 at most in the install.
pub const MAX_SECONDS: f32 = 3600.0;
/// Most weight a criterion gives: 50 at most in the install.
pub const MAX_WEIGHT: f32 = 1.0e4;
/// Most prune reasons told.
pub const MAX_REASONS: usize = 1024;
/// Most bytes of `rules.json`.
pub const MAX_RULES_BYTES: usize = 8 << 20;

/// A criterion, as a rule requires it.
#[derive(Clone, Debug, PartialEq)]
pub struct Criterion {
    pub name: String,
    /// The fact it reads (`classname`).
    pub key: String,
    /// What the fact must match, as written.
    pub expr: String,
    pub matcher: Matcher,
    /// What it adds to its rule's score when it matches (1 when the file states none, as
    /// Source's criteria default to).
    pub weight: f32,
}

/// One line a rule may say.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// Its voice event, an index into [`HeroRules::events`].
    pub event: usize,
    /// Said at most once a match (`speakonce`).
    pub once: bool,
    /// Seconds before this line may be said again (`respeakdelay`).
    pub respeak: f32,
    /// Seconds after the query before it starts (`predelay`).
    pub predelay: f32,
}

/// A rule: the criteria a query must all match, and its lines.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub name: String,
    /// Indices into [`HeroRules::criteria`].
    pub criteria: Vec<usize>,
    pub lines: Vec<Line>,
}

/// A voice event of the lines.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub name: String,
    /// The pack's sounds it picks among, by index.
    pub sounds: Vec<usize>,
    /// Seconds it lasts (its longest sound).
    pub seconds: f32,
}

/// A hero's rules, as kept in its pack.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeroRules {
    /// The hero's Dota name (`pudge`).
    pub hero: String,
    /// Its class, the value of its queries' `classname` (`npc_dota_hero_pudge`).
    pub class: String,
    pub criteria: Vec<Criterion>,
    pub rules: Vec<Rule>,
    pub events: Vec<Event>,
    /// Lines pruned at import, by why.
    pub pruned: BTreeMap<String, u32>,
}

/// Why a pack's rules do not read.
fn broken(why: impl Into<String>) -> String {
    format!("rules.json: {}", why.into())
}

fn text(v: &Value, key: &str) -> Result<String, String> {
    let s = v
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| broken(format!("no {key}")))?;
    if s.len() > MAX_TEXT {
        return Err(broken(format!("{key} of {} bytes", s.len())));
    }
    Ok(s.to_owned())
}

fn seconds(v: &Value, key: &str) -> Result<f32, String> {
    let n = v.get(key).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    if !(0.0..=MAX_SECONDS).contains(&n) {
        return Err(broken(format!("{key} of {n} s")));
    }
    Ok(n)
}

fn list<'a>(v: &'a Value, key: &str, max: usize) -> Result<&'a [Value], String> {
    let items = v
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| broken(format!("no {key} list")))?;
    if items.len() > max {
        return Err(broken(format!("{} {key}, at most {max}", items.len())));
    }
    Ok(items)
}

fn index(v: &Value, len: usize, what: &str) -> Result<usize, String> {
    v.as_u64()
        .and_then(|i| usize::try_from(i).ok())
        .filter(|&i| i < len)
        .ok_or_else(|| broken(format!("a {what} index out of its list")))
}

impl HeroRules {
    /// The rules as `rules.json` holds them.
    pub fn to_json(&self) -> Vec<u8> {
        let criteria: Vec<Value> = self
            .criteria
            .iter()
            .map(|c| json!({"name": c.name, "key": c.key, "expr": c.expr, "weight": c.weight}))
            .collect();
        let rules: Vec<Value> = self
            .rules
            .iter()
            .map(|r| {
                let lines: Vec<Value> = r
                    .lines
                    .iter()
                    .map(|l| {
                        json!({"event": l.event, "once": l.once, "respeak": l.respeak,
                            "predelay": l.predelay})
                    })
                    .collect();
                json!({"name": r.name, "criteria": r.criteria, "lines": lines})
            })
            .collect();
        let events: Vec<Value> = self
            .events
            .iter()
            .map(|e| json!({"name": e.name, "sounds": e.sounds, "seconds": e.seconds}))
            .collect();
        let value = json!({"hero": self.hero, "class": self.class, "criteria": criteria,
            "rules": rules, "events": events, "pruned": self.pruned});
        serde_json::to_vec(&value).expect("a JSON value writes")
    }

    /// The rules of a pack's `rules.json`, checked against the pack's `sounds` count.
    pub fn parse(bytes: &[u8], sounds: usize) -> Result<HeroRules, String> {
        if bytes.len() > MAX_RULES_BYTES {
            return Err(broken(format!("{} bytes", bytes.len())));
        }
        let v: Value = serde_json::from_slice(bytes).map_err(|e| broken(e.to_string()))?;
        let events = list(&v, "events", MAX_EVENTS)?
            .iter()
            .map(|e| event(e, sounds))
            .collect::<Result<Vec<_>, _>>()?;
        let criteria = list(&v, "criteria", MAX_CRITERIA)?
            .iter()
            .map(criterion)
            .collect::<Result<Vec<_>, _>>()?;
        let rules = list(&v, "rules", MAX_RULES)?
            .iter()
            .map(|r| rule(r, criteria.len(), events.len()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(HeroRules {
            hero: text(&v, "hero")?,
            class: text(&v, "class")?,
            criteria,
            rules,
            events,
            pruned: pruned(&v)?,
        })
    }

    /// Lines the rules can say.
    pub fn lines(&self) -> usize {
        self.rules.iter().map(|r| r.lines.len()).sum()
    }
}

fn criterion(v: &Value) -> Result<Criterion, String> {
    let expr = text(v, "expr")?;
    let weight = v.get("weight").and_then(Value::as_f64).unwrap_or(1.0) as f32;
    if !(-MAX_WEIGHT..=MAX_WEIGHT).contains(&weight) {
        return Err(broken(format!("a weight of {weight}")));
    }
    Ok(Criterion {
        name: text(v, "name")?,
        key: text(v, "key")?,
        matcher: Matcher::parse(&expr),
        expr,
        weight,
    })
}

fn rule(v: &Value, criteria: usize, events: usize) -> Result<Rule, String> {
    let line = |l: &Value| {
        Ok::<_, String>(Line {
            event: index(l.get("event").unwrap_or(&Value::Null), events, "event")?,
            once: l.get("once").and_then(Value::as_bool).unwrap_or(false),
            respeak: seconds(l, "respeak")?,
            predelay: seconds(l, "predelay")?,
        })
    };
    let lines = list(v, "lines", MAX_RULE_LINES)?
        .iter()
        .map(line)
        .collect::<Result<Vec<_>, _>>()?;
    if lines.is_empty() {
        return Err(broken("a rule without lines"));
    }
    Ok(Rule {
        name: text(v, "name")?,
        criteria: list(v, "criteria", MAX_RULE_CRITERIA)?
            .iter()
            .map(|i| index(i, criteria, "criterion"))
            .collect::<Result<_, _>>()?,
        lines,
    })
}

fn event(v: &Value, sounds: usize) -> Result<Event, String> {
    let sounds: Vec<usize> = list(v, "sounds", MAX_EVENT_SOUNDS)?
        .iter()
        .map(|i| index(i, sounds, "sound"))
        .collect::<Result<_, _>>()?;
    if sounds.is_empty() {
        return Err(broken("an event without sounds"));
    }
    Ok(Event {
        name: text(v, "name")?,
        sounds,
        seconds: seconds(v, "seconds")?,
    })
}

fn pruned(v: &Value) -> Result<BTreeMap<String, u32>, String> {
    let map = v
        .get("pruned")
        .and_then(Value::as_object)
        .ok_or_else(|| broken("no pruned counts"))?;
    if map.len() > MAX_REASONS {
        return Err(broken(format!("{} prune reasons", map.len())));
    }
    map.iter()
        .map(|(why, n)| {
            let n = n.as_u64().and_then(|n| u32::try_from(n).ok());
            match n {
                Some(n) if why.len() <= MAX_TEXT => Ok((why.clone(), n)),
                _ => Err(broken("a prune count that does not read")),
            }
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sample() -> HeroRules {
        let criterion = |name: &str, key: &str, expr: &str, weight: f32| Criterion {
            name: name.into(),
            key: key.into(),
            expr: expr.into(),
            matcher: Matcher::parse(expr),
            weight,
        };
        HeroRules {
            hero: "pudge".into(),
            class: "npc_dota_hero_pudge".into(),
            criteria: vec![
                criterion("Move", "Concept", "TLK_DOTA_MOVE", 1.0),
                criterion("Ispudge", "classname", "npc_dota_hero_pudge", 5.0),
            ],
            rules: vec![Rule {
                name: "pudge_Move".into(),
                criteria: vec![0, 1],
                lines: vec![Line {
                    event: 0,
                    once: false,
                    respeak: 120.0,
                    predelay: 0.0,
                }],
            }],
            events: vec![Event {
                name: "pudge_pud_move_01".into(),
                sounds: vec![0],
                seconds: 0.5,
            }],
            pruned: BTreeMap::from([("cosmetic".to_owned(), 3)]),
        }
    }

    #[test]
    fn rules_read_back_as_written() {
        let rules = sample();
        assert_eq!(HeroRules::parse(&rules.to_json(), 1), Ok(rules));
    }

    #[test]
    fn a_sound_index_past_the_pack_is_refused() {
        let error = HeroRules::parse(&sample().to_json(), 0).unwrap_err();
        assert_eq!(error, "rules.json: a sound index out of its list");
    }

    #[test]
    fn a_delay_past_the_bound_is_refused() {
        let mut rules = sample();
        rules.rules[0].lines[0].respeak = 1.0e9;
        let error = HeroRules::parse(&rules.to_json(), 1).unwrap_err();
        assert_eq!(error, "rules.json: respeak of 1000000000 s");
    }
}
