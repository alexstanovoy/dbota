//! The hero answering a query from its rules, as Source's response system and Dota's speech
//! do it:
//!
//! - a rule matches when every criterion it requires matches the query (Dota plays a response
//!   "only if all rules are fulfilled": the "Custom response rules" note of every hero's response
//!   page on Liquipedia); its score is the sum of their weights, and the best-scoring rule is
//!   taken, ties drawn at random (`CResponseSystem::FindBestMatchingRule`, Source SDK 2013);
//! - its line is drawn among those not said since the rule last went through them all (a
//!   group's default, "by default we visit all responses in group before repeating any"), and
//!   never one said once that is `speakonce` or within its own `respeakdelay` ("don't use this
//!   response again for at least this long"; both quoted from the header of Valve's
//!   `response_rules.txt`);
//! - a concept's own cooldown ([`Concept::cooldown_secs`]) holds after a line of it, and the
//!   hero says nothing new while a line is still being said or waits for its `predelay`.
//!
//! A line's `weapondelay` keeps a Source speaker from firing its weapon so that it is heard
//! (`CAI_PlayerAlly::OnSpokeConcept`): it changes nothing of what or when the hero says, and is
//! not kept.

use std::collections::BTreeMap;

use super::concept::{Concept, Query};
use super::rules::HeroRules;

/// A line the hero says.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    /// The rule's index in [`HeroRules::rules`].
    pub rule: usize,
    /// The line's index in the rule's lines.
    pub line: usize,
    /// The pack's sound it plays.
    pub sound: usize,
    /// The criteria of the rule, all of which matched, by index.
    pub matched: Vec<usize>,
    pub score: f32,
    /// Seconds before it starts.
    pub delay: f32,
}

/// Why the hero says nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Silence {
    /// It is still saying (or about to say) its last line.
    Speaking,
    /// The concept's own cooldown holds.
    Cooldown,
    /// No rule matches.
    NoRule,
    /// The best rule has no line it may say now.
    NoLine,
}

/// What the hero remembers of what it said, for one match.
#[derive(Clone, Debug)]
pub struct Speaker {
    /// Per rule, per line: whether said since the rule last went through them all.
    used: Vec<Vec<bool>>,
    /// Per rule, per line: when it may be said again (never for a `speakonce` one said).
    ready: Vec<Vec<f64>>,
    /// When each concept may be said again.
    concept_ready: BTreeMap<Concept, f64>,
    /// When the line being said ends.
    busy_until: f64,
    rng: u64,
}

impl Speaker {
    /// A hero that has said nothing yet, drawing its randomness from `seed`.
    pub fn new(rules: &HeroRules, seed: u64) -> Speaker {
        Speaker {
            used: rules
                .rules
                .iter()
                .map(|r| vec![false; r.lines.len()])
                .collect(),
            ready: rules
                .rules
                .iter()
                .map(|r| vec![f64::NEG_INFINITY; r.lines.len()])
                .collect(),
            concept_ready: BTreeMap::new(),
            busy_until: f64::NEG_INFINITY,
            // xorshift has no zero state.
            rng: seed | 1,
        }
    }

    /// The match clock (seconds) up to which the hero is saying, or waiting to say, its last
    /// line: it says nothing new before.
    pub fn speaking_until(&self) -> f64 {
        self.busy_until
    }

    /// The next random number of the speaker's stream, below `n` (which is at least 1).
    pub fn draw(&mut self, n: usize) -> usize {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng % n.max(1) as u64) as usize
    }

    /// Starts the stream of randomness afresh from `seed`.
    pub fn reseed(&mut self, seed: u64) {
        // xorshift has no zero state.
        self.rng = seed | 1;
    }

    /// What the hero would say to `query` at `now` (seconds of the match's clock); nothing is
    /// remembered of it until it is [accepted](Self::accept).
    pub fn choose(
        &mut self,
        rules: &HeroRules,
        query: &Query,
        now: f64,
    ) -> Result<Choice, Silence> {
        if now < self.busy_until {
            return Err(Silence::Speaking);
        }
        if self
            .concept_ready
            .get(&query.concept)
            .is_some_and(|&ready| now < ready)
        {
            return Err(Silence::Cooldown);
        }
        let (rule, score) = self.best_rule(rules, query).ok_or(Silence::NoRule)?;
        let line = self.pick_line(rule, now).ok_or(Silence::NoLine)?;
        let said = &rules.rules[rule].lines[line];
        let event = &rules.events[said.event];
        let sound = event.sounds[self.draw(event.sounds.len())];
        Ok(Choice {
            rule,
            line,
            sound,
            matched: rules.rules[rule].criteria.clone(),
            score,
            delay: said.predelay,
        })
    }

    /// Remembers that the hero says `choice` to a query of `concept` from `now` on (its sound
    /// started): the line is used, waits its respeak (forever when `speakonce`), the concept its
    /// cooldown, and the hero says nothing else until the line ends.
    pub fn accept(&mut self, rules: &HeroRules, concept: Concept, choice: &Choice, now: f64) {
        let said = &rules.rules[choice.rule].lines[choice.line];
        let fresh_left = (0..self.used[choice.rule].len()).any(|l| {
            l != choice.line && !self.used[choice.rule][l] && now >= self.ready[choice.rule][l]
        });
        if !fresh_left && self.used[choice.rule][choice.line] {
            // The rule had gone through every line it may say: it goes round again.
            self.used[choice.rule]
                .iter_mut()
                .for_each(|used| *used = false);
        }
        self.used[choice.rule][choice.line] = true;
        self.ready[choice.rule][choice.line] = if said.once {
            f64::INFINITY
        } else {
            now + f64::from(said.respeak)
        };
        let seconds = rules.events[said.event].seconds;
        self.busy_until = now + f64::from(said.predelay) + f64::from(seconds);
        let cooldown = concept.cooldown_secs();
        if cooldown > 0.0 {
            self.concept_ready
                .insert(concept, now + f64::from(cooldown));
        }
    }

    /// [`choose`](Self::choose), then [`accept`](Self::accept) what is chosen.
    pub fn respond(
        &mut self,
        rules: &HeroRules,
        query: &Query,
        now: f64,
    ) -> Result<Choice, Silence> {
        let choice = self.choose(rules, query, now)?;
        self.accept(rules, query.concept, &choice, now);
        Ok(choice)
    }

    /// The best-scoring rule every criterion of which matches, ties drawn; none scoring above
    /// zero is none (Source's best score starts above it).
    fn best_rule(&mut self, rules: &HeroRules, query: &Query) -> Option<(usize, f32)> {
        let mut best: Vec<usize> = Vec::new();
        let mut best_score = 0.0f32;
        for (at, rule) in rules.rules.iter().enumerate() {
            let mut score = 0.0f32;
            let all = rule.criteria.iter().all(|&c| {
                let criterion = &rules.criteria[c];
                score += criterion.weight;
                criterion.matcher.matches(query.value(&criterion.key))
            });
            if !all || score <= 0.0 || score < best_score {
                continue;
            }
            if score > best_score {
                best.clear();
                best_score = score;
            }
            best.push(at);
        }
        let pick = *best.get(self.draw(best.len()))?;
        Some((pick, best_score))
    }

    /// A line of `rule` that may be said at `now`: one not said since the rule went through them
    /// all, when there is one; the rule goes round again once every line it may say was said.
    fn pick_line(&mut self, rule: usize, now: f64) -> Option<usize> {
        let ready: Vec<usize> = (0..self.ready[rule].len())
            .filter(|&l| now >= self.ready[rule][l])
            .collect();
        if ready.is_empty() {
            return None;
        }
        let fresh: Vec<usize> = ready
            .iter()
            .copied()
            .filter(|&l| !self.used[rule][l])
            .collect();
        let pool = if fresh.is_empty() { ready } else { fresh };
        Some(pool[self.draw(pool.len())])
    }
}

/// Most steps (queries and lines accepted) a [`History`] keeps to rewind by: a match of two
/// hours asking a query a second, and accepting each.
pub const MAX_STEPS: usize = 1 << 14;

/// One step of a [`History`].
#[derive(Clone, Debug)]
enum Step {
    /// A query asked at a time.
    Ask(f64, Query),
    /// The answer to query `n`, of a concept, accepted at a time.
    Accept(usize, Concept, f64),
}

/// What a hero was asked in one match and what it said: the answers are a function of the
/// match's seed and of the steps before, every query drawing its randomness afresh from the seed
/// and its own number, so a replay sought back to a time ([`History::rewind`]) answers again as
/// it did. A match of more steps than [`MAX_STEPS`] keeps answering, but a rewind past then
/// starts afresh from the seed.
#[derive(Clone, Debug)]
pub struct History {
    seed: u64,
    speaker: Speaker,
    steps: Vec<Step>,
    /// Queries asked so far: the next one's number.
    asked: usize,
    /// The line the last query chose, by its number, until it is accepted or another query is
    /// asked: a line is accepted before the hero is asked anything else.
    pending: Option<(usize, Choice)>,
    whole: bool,
}

/// The seed of the `n`th query of a history of `seed` (SplitMix64's finalizer).
fn query_seed(seed: u64, n: usize) -> u64 {
    let mut z = seed ^ (n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl History {
    /// A history of nothing asked yet, of the match whose seed is `seed`.
    pub fn new(rules: &HeroRules, seed: u64) -> History {
        History {
            seed,
            speaker: Speaker::new(rules, seed),
            steps: Vec::new(),
            asked: 0,
            pending: None,
            whole: true,
        }
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// The match clock up to which the hero is saying its last line.
    pub fn speaking_until(&self) -> f64 {
        self.speaker.speaking_until()
    }

    fn record(&mut self, step: Step) {
        if self.steps.len() < MAX_STEPS {
            self.steps.push(step);
        } else {
            self.whole = false;
        }
    }

    /// Asks the query `query` makes of the random number (0 to 99) drawn for it, at `now`: the
    /// query, its number (for [`History::accept`]) and what the hero would say.
    pub fn ask(
        &mut self,
        rules: &HeroRules,
        now: f64,
        query: impl FnOnce(u32) -> Query,
    ) -> (usize, Query, Result<Choice, Silence>) {
        let n = self.asked;
        self.asked += 1;
        self.speaker.reseed(query_seed(self.seed, n));
        let query = query(self.speaker.draw(100) as u32);
        let choice = self.speaker.choose(rules, &query, now);
        self.record(Step::Ask(now, query.clone()));
        self.pending = choice.as_ref().ok().map(|choice| (n, choice.clone()));
        (n, query, choice)
    }

    /// Accepts at `now` the answer of query `n`, the last asked: its line sounds. The answer
    /// of a query asked before the last is not accepted.
    pub fn accept(&mut self, rules: &HeroRules, n: usize, concept: Concept, now: f64) {
        if let Some((_, choice)) = self.pending.take_if(|(asked, _)| *asked == n) {
            self.speaker.accept(rules, concept, &choice, now);
            self.record(Step::Accept(n, concept, now));
        }
    }

    /// The history as it stood at `now`: every step after it forgotten, the speaker as those
    /// before left it.
    pub fn rewind(&mut self, rules: &HeroRules, now: f64) {
        let steps = std::mem::take(&mut self.steps);
        let whole = self.whole;
        *self = History::new(rules, self.seed);
        if !whole {
            return;
        }
        for step in steps {
            match step {
                Step::Ask(at, query) if at <= now => {
                    let _ = self.ask(rules, at, |_| query);
                }
                Step::Accept(n, concept, at) if at <= now => self.accept(rules, n, concept, at),
                _ => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::concept::Fact;
    use crate::voice::expr::Matcher;
    use crate::voice::rules::{Criterion, Event, Line, Rule};

    fn criterion(name: &str, key: &str, expr: &str, weight: f32) -> Criterion {
        Criterion {
            name: name.into(),
            key: key.into(),
            expr: expr.into(),
            matcher: Matcher::parse(expr),
            weight,
        }
    }

    fn line(event: usize, once: bool, respeak: f32) -> Line {
        Line {
            event,
            once,
            respeak,
            predelay: 0.0,
        }
    }

    /// Pudge's kill rules, cut down: the plain kill, and first blood weighing far more.
    fn kills() -> HeroRules {
        HeroRules {
            hero: "pudge".into(),
            class: "npc_dota_hero_pudge".into(),
            criteria: vec![
                criterion("Kill", "Concept", "TLK_DOTA_KILL", 1.0),
                criterion("Ispudge", "classname", "npc_dota_hero_pudge", 5.0),
                criterion("IsFirstBlood", "firstblood", "1", 50.0),
                criterion("IsNotChain", "customresponse", "!=pudarc_chains", 5.0),
            ],
            rules: vec![
                Rule {
                    name: "pudge_Kill".into(),
                    criteria: vec![0, 1, 3],
                    lines: vec![line(0, false, 0.0), line(1, false, 0.0)],
                },
                Rule {
                    name: "pudge_KillIsFirstBlood".into(),
                    criteria: vec![0, 1, 2],
                    lines: vec![line(2, true, 0.0)],
                },
            ],
            events: (0..3)
                .map(|i| Event {
                    name: format!("kill_{i}"),
                    sounds: vec![i],
                    seconds: 1.0,
                })
                .collect(),
            pruned: BTreeMap::new(),
        }
    }

    fn kill(first_blood: bool) -> Query {
        Query::new(Concept::Kill, "npc_dota_hero_pudge", 0, 100.0)
            .with(Fact::FirstBlood, if first_blood { "1" } else { "0" })
    }

    #[test]
    fn the_heaviest_matching_rule_answers() {
        let rules = kills();
        let mut speaker = Speaker::new(&rules, 7);
        let choice = speaker.respond(&rules, &kill(true), 0.0).unwrap();
        assert_eq!(rules.rules[choice.rule].name, "pudge_KillIsFirstBlood");
        assert_eq!(choice.score, 56.0);
        let choice = speaker.respond(&rules, &kill(false), 10.0).unwrap();
        assert_eq!(rules.rules[choice.rule].name, "pudge_Kill");
    }

    #[test]
    fn a_speakonce_line_is_said_once_and_its_rule_then_says_nothing() {
        let rules = kills();
        let mut speaker = Speaker::new(&rules, 7);
        assert!(speaker.respond(&rules, &kill(true), 0.0).is_ok());
        assert_eq!(
            speaker.respond(&rules, &kill(true), 100.0),
            Err(Silence::NoLine)
        );
    }

    #[test]
    fn a_rule_says_all_its_lines_before_repeating_one() {
        let rules = kills();
        let mut speaker = Speaker::new(&rules, 3);
        let mut said: Vec<usize> = (0..2)
            .map(|i| speaker.respond(&rules, &kill(false), 10.0 * f64::from(i)))
            .map(|c| c.unwrap().line)
            .collect();
        said.sort();
        assert_eq!(said, [0, 1]);
    }

    #[test]
    fn a_line_waits_its_respeak_and_the_hero_its_last_line() {
        let mut rules = kills();
        rules.rules[1].lines[0] = line(2, false, 300.0);
        let mut speaker = Speaker::new(&rules, 7);
        assert!(speaker.respond(&rules, &kill(true), 0.0).is_ok());
        assert_eq!(
            speaker.respond(&rules, &kill(true), 0.5),
            Err(Silence::Speaking)
        );
        assert_eq!(
            speaker.respond(&rules, &kill(true), 299.9),
            Err(Silence::NoLine)
        );
        assert!(speaker.respond(&rules, &kill(true), 300.0).is_ok());
    }

    #[test]
    fn a_move_keeps_its_concepts_cooldown() {
        let rules = HeroRules {
            criteria: vec![criterion("Move", "Concept", "TLK_DOTA_MOVE", 1.0)],
            rules: vec![Rule {
                name: "move".into(),
                criteria: vec![0],
                lines: vec![line(0, false, 0.0)],
            }],
            events: vec![Event {
                name: "move".into(),
                sounds: vec![0],
                seconds: 0.5,
            }],
            ..HeroRules::default()
        };
        let mut speaker = Speaker::new(&rules, 1);
        let query = Query::new(Concept::Move, "npc_dota_hero_pudge", 0, 0.0);
        assert!(speaker.respond(&rules, &query, 0.0).is_ok());
        assert_eq!(speaker.respond(&rules, &query, 5.9), Err(Silence::Cooldown));
        assert!(speaker.respond(&rules, &query, 6.0).is_ok());
    }

    /// A line whose sound the mixer refused was not said: the hero may choose it again.
    #[test]
    fn a_line_chosen_but_not_accepted_is_not_remembered() {
        let rules = kills();
        let mut speaker = Speaker::new(&rules, 7);
        let chosen = speaker.choose(&rules, &kill(true), 0.0).expect("a line");
        let again = speaker.choose(&rules, &kill(true), 0.1).expect("a line");
        assert_eq!((again.rule, again.line), (chosen.rule, chosen.line));
    }

    /// What a fresh history answers to kills at `times`, by line.
    fn kills_at(history: &mut History, rules: &HeroRules, times: &[f64]) -> Vec<Option<usize>> {
        times
            .iter()
            .map(|&now| {
                let (n, query, said) = history.ask(rules, now, |_| kill(false));
                if said.is_ok() {
                    history.accept(rules, n, query.concept, now);
                }
                said.ok().map(|c| c.line)
            })
            .collect()
    }

    /// A replay sought back answers again as it did: answers are the match's seed and what
    /// was asked before, and a history rewound to a time is the one it had then.
    #[test]
    fn a_history_rewound_answers_again_as_it_did() {
        let mut rules = kills();
        rules.rules[0].lines = (0..8).map(|i| line(i % 2, false, 0.0)).collect();
        let times = [0.0, 10.0, 20.0, 30.0, 40.0, 50.0];
        let mut first = History::new(&rules, 11);
        let once = kills_at(&mut first, &rules, &times);
        let mut sought = History::new(&rules, 11);
        kills_at(&mut sought, &rules, &times[..4]);
        sought.rewind(&rules, 15.0);
        let again = kills_at(&mut sought, &rules, &times[2..]);
        assert_eq!(again, once[2..]);
    }

    /// A match asked more than [`MAX_STEPS`] times keeps no more than that of what it asked.
    #[test]
    fn a_long_match_keeps_a_bounded_history() {
        let rules = kills();
        let mut history = History::new(&rules, 3);
        for n in 0..2 * MAX_STEPS {
            let _ = history.ask(&rules, n as f64, |_| kill(false));
        }
        assert!(
            history.steps.len() <= MAX_STEPS,
            "{} steps",
            history.steps.len()
        );
        // Of the choices, only the last one's waits to be accepted.
        let (n, query, said) = history.ask(&rules, 1e6, |_| kill(true));
        assert_eq!(history.pending.as_ref().map(|(asked, _)| *asked), Some(n));
        history.accept(&rules, n, query.concept, 1e6);
        assert!(said.is_ok());
        assert!(history.pending.is_none());
    }

    #[test]
    fn a_rule_without_criteria_never_answers() {
        let mut rules = kills();
        rules.rules[0].criteria.clear();
        rules.rules[1].criteria.clear();
        let mut speaker = Speaker::new(&rules, 7);
        assert_eq!(
            speaker.respond(&rules, &kill(false), 0.0),
            Err(Silence::NoRule)
        );
    }
}
