//! A response criterion's match expression, read as Source's response system reads it
//! (`CResponseSystem::ComputeMatcher` and `Compare` of the Source SDK 2013's
//! `AI_ResponseSystem.cpp`; the syntax is documented in the header of Valve's own
//! `scripts/talker/response_rules.txt`): `npc_dota_hero_pudge` matches that text in any case,
//! `13` the number 13, `!=arcana` anything but that text, `>=97,<=98` a number in that range.
//!
//! A fact the query does not carry matches only a negated expression: `!=pudarc_chains` matches a
//! Pudge without the arcana, `0` (Pudge's `HookCatchNothing`) never matches a query without
//! `hookedtarget`. Source 1 compared a missing fact as the empty text, zero as a number; Dota's
//! own rules are not consistent with that: at the battle's start Pudge's `IsGameStart` rule would
//! tie with his `HookCatchNothing` rule (both 11), and his selection lines, which require
//! `IsNotChain`, play without the arcana. The text comparison ignores ASCII case (`Q_stricmp`).

/// One bound of a numeric range: its value and whether the value itself is inside.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Bound {
    value: f64,
    inclusive: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Test {
    /// The fact's text, in any case.
    Text(String),
    /// The fact's number within the bounds.
    Range {
        min: Option<Bound>,
        max: Option<Bound>,
    },
}

/// A parsed match expression.
#[derive(Clone, Debug, PartialEq)]
pub struct Matcher {
    test: Test,
    /// `!`: what the test says, the other way round.
    negated: bool,
}

/// What Source takes for a number: an optional sign, digits and at most one point
/// (`AI_IsNumeric`).
fn is_numeric(token: &str) -> bool {
    let digits = token.strip_prefix(['-', '+']).unwrap_or(token);
    !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && digits.bytes().filter(|&b| b == b'.').count() <= 1
        && digits.bytes().any(|b| b.is_ascii_digit())
}

/// A fact's number: zero for anything that is not one, as C's `atof` reads the facts the client
/// makes (they are either plain numbers or names). Never infinite, never NaN.
pub fn number(value: &str) -> f64 {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .unwrap_or(0.0)
}

impl Matcher {
    /// The matcher of `expr`. Every text is an expression: one that reads as nothing sensible
    /// (`=` alone) matches the facts it literally says.
    pub fn parse(expr: &str) -> Matcher {
        let mut min = None;
        let mut max = None;
        let mut text = None;
        let mut negated = false;
        for part in expr.split(',') {
            let (mut gt, mut lt, mut eq) = (false, false, false);
            let mut token = String::new();
            for c in part.chars() {
                match c {
                    '>' => gt = true,
                    '<' => lt = true,
                    '=' => eq = true,
                    '!' => negated = true,
                    other => token.push(other),
                }
            }
            let bound = || Bound {
                value: number(&token),
                inclusive: eq,
            };
            if gt {
                min = Some(bound());
            } else if lt {
                max = Some(bound());
            } else if is_numeric(&token) {
                let exact = Bound {
                    value: number(&token),
                    inclusive: true,
                };
                (min, max) = (Some(exact), Some(exact));
            } else {
                text = Some(token);
            }
        }
        let test = match text {
            Some(text) if min.is_none() && max.is_none() => Test::Text(text),
            _ => Test::Range { min, max },
        };
        Matcher { test, negated }
    }

    /// Whether a fact of `value` matches; `None` when the query lacks the fact.
    pub fn matches(&self, value: Option<&str>) -> bool {
        let Some(value) = value else {
            return self.negated;
        };
        let hit = match &self.test {
            Test::Text(text) => value.eq_ignore_ascii_case(text),
            Test::Range { min, max } => {
                let n = number(value);
                let above = min.is_none_or(|b| n > b.value || (b.inclusive && n == b.value));
                let below = max.is_none_or(|b| n < b.value || (b.inclusive && n == b.value));
                above && below
            }
        };
        hit != self.negated
    }
}

#[cfg(test)]
mod tests {
    use super::Matcher;

    fn matches(expr: &str, value: &str) -> bool {
        Matcher::parse(expr).matches(Some(value))
    }

    fn matches_missing(expr: &str) -> bool {
        Matcher::parse(expr).matches(None)
    }

    #[test]
    fn a_name_matches_in_any_case_and_nothing_else() {
        assert!(matches(
            "npc_dota_hero_Nevermore",
            "npc_dota_hero_nevermore"
        ));
        assert!(!matches("npc_dota_hero_Nevermore", "npc_dota_hero_pudge"));
        assert!(!matches("npc_dota_hero_Nevermore", ""));
    }

    #[test]
    fn only_a_negated_expression_matches_a_missing_fact() {
        assert!(matches_missing("!=pudarc_chains"));
        assert!(!matches("!=pudarc_chains", "PUDARC_CHAINS"));
        assert!(!matches_missing("0"));
        assert!(!matches_missing("<=30"));
        assert!(!matches_missing("pudarc_chains"));
    }

    #[test]
    fn ranges_keep_their_open_and_closed_ends() {
        assert!(matches(">=97,<=98", "97"));
        assert!(matches(">=97,<=98", "98"));
        assert!(!matches(">=97,<=98", "99"));
        assert!(!matches(">60,<660", "60"));
        assert!(matches(">60,<660", "60.5"));
        assert!(matches("<0.2", "0.19"));
        assert!(!matches("<0.2", "0.2"));
    }

    #[test]
    fn a_text_that_is_no_number_is_the_number_zero() {
        assert!(matches("<=30", "item_blink"));
        assert!(matches("0", ""));
        assert!(!matches("13", ""));
        assert!(matches("-1", "-1"));
        assert!(!matches("!=0", "0"));
        assert!(matches("!=0", "2"));
    }

    #[test]
    fn a_number_that_is_not_finite_counts_as_zero() {
        assert!(matches("0", "NaN"));
        assert!(matches("0", "inf"));
    }
}
