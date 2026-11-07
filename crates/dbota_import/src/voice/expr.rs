//! A response criterion's match expression, read and compared exactly as Source's response
//! system does (`ComputeMatcher` and `CompareUsingMatcher` of the Source SDK 2013's
//! `AI_ResponseSystem.cpp`; the syntax is documented in the header of Valve's own
//! `scripts/talker/response_rules.txt`): `npc_dota_hero_pudge` matches that text in any case,
//! `13` the number 13, `!=arcana` anything but that text, `>=97,<=98` a number in that range.
//!
//! A fact the query lacks is the empty text, whose number is zero (C's `atof`), with the one
//! exception Source makes: a plain number (`0`, Pudge's `HookCatchNothing`) never matches a
//! missing fact ("If the setValue is "", the NPC doesn't have the key at all, in which case we
//! shouldn't match "0""). So a range (`<=30`) matches a missing fact when it holds zero, a
//! negated text (`!=pudarc_chains`) matches it, a negated number (`!=0`) does not.

/// One bound of a numeric range: its value and whether the value itself is inside.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Bound {
    value: f64,
    inclusive: bool,
}

/// A parsed match expression.
#[derive(Clone, Debug, PartialEq)]
pub struct Matcher {
    min: Option<Bound>,
    max: Option<Bound>,
    /// `!`: the token is what the fact must not be.
    not_equal: bool,
    /// Whether the token is a number (Source's `AppearsToBeANumber`), or a range was given.
    numeric: bool,
    /// The last token of the expression.
    token: String,
}

/// The number at the start of `text`, as C's `atof` reads it: zero when there is none. Never
/// infinite, never NaN.
pub fn number(text: &str) -> f64 {
    let text = text.trim_start();
    let bytes = text.as_bytes();
    let mut end = 0;
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        end = 1;
    }
    let whole = digits(end);
    end += whole;
    let mut fraction = 0;
    if bytes.get(end) == Some(&b'.') {
        fraction = digits(end + 1);
        end += 1 + fraction;
    }
    if whole + fraction == 0 {
        return 0.0;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        let exponent = digits(end + 1 + sign);
        if exponent > 0 {
            end += 1 + sign + exponent;
        }
    }
    text[..end]
        .trim_end_matches('.')
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .unwrap_or(0.0)
}

/// Source's `AppearsToBeANumber`: a token whose number is not zero, or one of zeros only (the
/// empty token included).
fn appears_numeric(token: &str) -> bool {
    number(token) != 0.0 || token.bytes().all(|b| b == b'0')
}

impl Matcher {
    /// The matcher of `expr`, token by comma-separated token as Source reads it.
    pub fn parse(expr: &str) -> Matcher {
        let mut matcher = Matcher {
            min: None,
            max: None,
            not_equal: false,
            numeric: false,
            token: String::new(),
        };
        for part in expr.split(',') {
            let (mut gt, mut lt, mut eq, mut nt) = (false, false, false, false);
            let mut token = String::new();
            for c in part.chars() {
                match c {
                    '>' => gt = true,
                    '<' => lt = true,
                    '=' => eq = true,
                    '!' => nt = true,
                    other => token.push(other),
                }
            }
            let bound = Bound {
                value: number(&token),
                inclusive: eq,
            };
            if gt {
                matcher.min = Some(bound);
                matcher.numeric = true;
            } else if lt {
                matcher.max = Some(bound);
                matcher.numeric = true;
            } else {
                matcher.not_equal = nt;
                matcher.numeric = appears_numeric(&token);
            }
            matcher.token = token;
        }
        matcher
    }

    /// Whether a fact of `value` matches; `None` when the query lacks the fact (Source's empty
    /// text).
    pub fn matches(&self, value: Option<&str>) -> bool {
        let set = value.unwrap_or_default();
        let v = number(set);
        if self.min.is_some() || self.max.is_some() {
            let above = self
                .min
                .is_none_or(|b| v > b.value || (b.inclusive && v == b.value));
            let below = self
                .max
                .is_none_or(|b| v < b.value || (b.inclusive && v == b.value));
            return above && below;
        }
        if self.not_equal {
            return if self.numeric {
                v != number(&self.token)
            } else {
                !set.eq_ignore_ascii_case(&self.token)
            };
        }
        if self.numeric {
            return !set.is_empty() && v == number(&self.token);
        }
        set.eq_ignore_ascii_case(&self.token)
    }
}

#[cfg(test)]
mod tests {
    use super::{Matcher, number};

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

    /// Source's `CompareUsingMatcher` on a fact the query lacks (the empty text).
    #[test]
    fn a_missing_fact_is_compared_as_source_compares_it() {
        assert!(matches_missing("!=pudarc_chains"));
        assert!(!matches("!=pudarc_chains", "PUDARC_CHAINS"));
        assert!(!matches_missing("pudarc_chains"));
        assert!(
            !matches_missing("0"),
            "a plain number never matches a missing fact"
        );
        assert!(
            matches_missing("<=30"),
            "a range reads a missing fact as zero"
        );
        assert!(!matches_missing(">60,<660"));
        assert!(!matches_missing("!=0"), "zero is zero");
        assert!(matches_missing("!=5"));
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
    fn numbers_compare_as_numbers() {
        assert!(matches("13", "13.0"));
        assert!(!matches("13", "12"));
        assert!(matches("-1", "-1"));
        assert!(!matches("!=0", "0"));
        assert!(matches("!=0", "2"));
        assert!(
            matches("<=30", "item_blink"),
            "a text that is no number is zero"
        );
    }

    #[test]
    fn a_number_is_read_as_atof_reads_it() {
        assert_eq!(number("12abc"), 12.0);
        assert_eq!(number("  -1.5e2x"), -150.0);
        assert_eq!(number("NaN"), 0.0);
        assert_eq!(number("inf"), 0.0);
        assert_eq!(number("1e999"), 0.0);
        assert_eq!(number("."), 0.0);
    }
}
