//! Reading the repository's own Rust, shell and Python sources for the ways a test or tool could
//! reach the user's data: starting the client as it is built, or naming an environment variable.

/// A token of Rust source that matters to the audit: words, punctuation and string contents.
/// Comments, character literals and lifetimes are dropped.
#[derive(Debug, PartialEq)]
pub enum Token {
    Word(String),
    Punct(char),
    Text(String),
}

pub fn tokens(source: &str) -> Vec<Token> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let next = chars.get(at + 1).copied();
        match chars[at] {
            '/' if next == Some('/') => {
                while at < chars.len() && chars[at] != '\n' {
                    at += 1;
                }
            }
            '/' if next == Some('*') => at = after_block_comment(&chars, at),
            '"' => {
                let (text, end) = quoted(&chars, at + 1);
                tokens.push(Token::Text(text));
                at = end;
            }
            '\'' => at = after_character_or_lifetime(&chars, at),
            c if c.is_alphabetic() || c == '_' => {
                let start = at;
                while at < chars.len() && (chars[at].is_alphanumeric() || chars[at] == '_') {
                    at += 1;
                }
                let word: String = chars[start..at].iter().collect();
                if matches!(word.as_str(), "r" | "br") && starts_raw_string(&chars, at) {
                    let (text, end) = raw(&chars, at);
                    tokens.push(Token::Text(text));
                    at = end;
                } else {
                    tokens.push(Token::Word(word));
                }
            }
            c if c.is_whitespace() => at += 1,
            c => {
                tokens.push(Token::Punct(c));
                at += 1;
            }
        }
    }
    tokens
}

fn after_block_comment(chars: &[char], start: usize) -> usize {
    let (mut depth, mut at) = (0, start);
    while at < chars.len() {
        match (chars[at], chars.get(at + 1)) {
            ('/', Some('*')) => {
                depth += 1;
                at += 2;
            }
            ('*', Some('/')) => {
                depth -= 1;
                at += 2;
                if depth == 0 {
                    break;
                }
            }
            _ => at += 1,
        }
    }
    at
}

/// The text of a string whose opening quote is before `start`, and where it ends. The escapes
/// that can spell a name (`\x42`, `\u{42}`) are decoded, so a variable cannot hide in them.
fn quoted(chars: &[char], start: usize) -> (String, usize) {
    let mut text = String::new();
    let mut at = start;
    while at < chars.len() {
        match chars[at] {
            '"' => return (text, at + 1),
            '\\' if at + 1 < chars.len() => {
                let (decoded, used) = escape(&chars[at + 1..]);
                text.push(decoded);
                at += 1 + used;
            }
            c => {
                text.push(c);
                at += 1;
            }
        }
    }
    (text, at)
}

/// The character an escape (after its backslash) stands for, and how many characters it takes.
fn escape(chars: &[char]) -> (char, usize) {
    let hex = |digits: &[char]| {
        let text: String = digits.iter().collect();
        u32::from_str_radix(&text, 16).ok().and_then(char::from_u32)
    };
    match chars {
        ['x', a, b, ..] => hex(&[*a, *b]).map_or(('x', 1), |c| (c, 3)),
        ['u', '{', rest @ ..] => match rest.iter().position(|c| *c == '}') {
            Some(end) if end <= 6 => hex(&rest[..end]).map_or(('u', 1), |c| (c, end + 3)),
            _ => ('u', 1),
        },
        ['n', ..] => ('\n', 1),
        ['t', ..] => ('\t', 1),
        ['r', ..] => ('\r', 1),
        ['0', ..] => ('\0', 1),
        [c, ..] => (*c, 1),
        [] => ('\\', 0),
    }
}

/// Whether the hashes (if any) at `at` open a raw string, not a raw identifier such as `r#type`.
fn starts_raw_string(chars: &[char], at: usize) -> bool {
    let hashes = chars[at..].iter().take_while(|c| **c == '#').count();
    chars.get(at + hashes) == Some(&'"')
}

/// The text of a raw string whose hashes (if any) start at `start`, and where it ends.
fn raw(chars: &[char], start: usize) -> (String, usize) {
    let hashes = chars[start..].iter().take_while(|c| **c == '#').count();
    let body = start + hashes + 1;
    let mut at = body;
    while at < chars.len() {
        let closes = chars[at] == '"'
            && chars[at + 1..]
                .iter()
                .take(hashes)
                .filter(|c| **c == '#')
                .count()
                == hashes;
        if closes {
            return (chars[body..at].iter().collect(), at + 1 + hashes);
        }
        at += 1;
    }
    (chars[body.min(chars.len())..].iter().collect(), chars.len())
}

/// Past a character literal such as `'x'` or `'\n'`; past the tick alone of a lifetime.
fn after_character_or_lifetime(chars: &[char], start: usize) -> usize {
    match (chars.get(start + 1), chars.get(start + 2)) {
        (Some('\\'), _) => {
            let mut at = start + 2;
            while at < chars.len() && chars[at] != '\'' {
                at += 1;
            }
            at + 1
        }
        (Some(_), Some('\'')) => start + 3,
        _ => start + 1,
    }
}

/// A string literal of Rust source and whether it only sets or removes a variable of a child
/// process (`.env("NAME", ..)`, `.env_remove("NAME")`), which cannot read anything.
#[derive(Debug, PartialEq)]
pub struct Literal {
    pub text: String,
    pub for_child: bool,
}

pub fn literals(source: &str) -> Vec<Literal> {
    let tokens = tokens(source);
    let mut found = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        let Token::Text(text) = token else {
            continue;
        };
        let for_child = at >= 3
            && tokens[at - 1] == Token::Punct('(')
            && tokens[at - 3] == Token::Punct('.')
            && matches!(&tokens[at - 2], Token::Word(w) if w == "env" || w == "env_remove");
        found.push(Literal {
            text: text.clone(),
            for_child,
        });
    }
    found
}

/// A variable of the project: the client's own knobs and the server's.
pub fn is_project_variable(name: &str) -> bool {
    (name.starts_with("BOTA_") || name.starts_with("DBOTA_"))
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// A variable that says where a user's files are.
pub fn is_location_variable(name: &str) -> bool {
    matches!(name, "HOME" | "APPDATA" | "LOCALAPPDATA") || name.starts_with("XDG_")
}

/// Why `source` may not be what it is: it names a variable of the project that is neither
/// scrubbed from clients (`handled`) nor an explicit input (`known`), or a location variable
/// without being one of the files that resolve default locations (`resolver`).
pub fn environment_violations(
    source: &str,
    resolver: bool,
    handled: impl Fn(&str) -> bool,
) -> Vec<String> {
    let mut found = Vec::new();
    let tokens = tokens(source);
    for (at, token) in tokens.iter().enumerate() {
        let enumerates = matches!(token, Token::Word(w) if w == "vars" || w == "vars_os")
            && at >= 3
            && tokens[at - 1] == Token::Punct(':')
            && tokens[at - 2] == Token::Punct(':')
            && matches!(&tokens[at - 3], Token::Word(w) if w == "env");
        if enumerates {
            found.push("enumerates the whole environment".to_owned());
        }
    }
    for literal in literals(source) {
        let name = literal.text.as_str();
        if is_project_variable(name) && !handled(name) && !name.ends_with("_CHILD") {
            found.push(format!("unhandled variable {name}"));
        }
        if is_location_variable(name) && !resolver && !literal.for_child {
            found.push(format!("reads the location variable {name}"));
        }
    }
    found
}

/// The lines of a Rust, shell or Python source that are not comments, with their numbers.
fn code_lines(source: &str) -> impl Iterator<Item = (usize, &str)> {
    source.lines().enumerate().filter(|(_, line)| {
        !line.trim_start().starts_with("//") && !line.trim_start().starts_with('#')
    })
}

/// The code squeezed to letters, digits and underscores, lower-cased, and the line each
/// character came from: `"$CARGO_TARGET_DIR/release/dbota"` and `target/${profile}/dbota` become
/// the same kind of run, whatever the quoting, concatenation or variable around it.
fn squeezed(source: &str) -> (String, Vec<usize>) {
    let (mut text, mut lines) = (String::new(), Vec::new());
    for (number, line) in code_lines(source) {
        for c in line
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        {
            text.push(c.to_ascii_lowercase());
            lines.push(number);
        }
    }
    (text, lines)
}

/// The ways to start the client as it is built, which would give it the caller's HOME, found in
/// the squeezed code: the binary Cargo names, `Command::new("dbota")`, a built command's
/// program, cargo's `run` spelled as arguments, and a `dbota` that closes a path through a build
/// profile's folder (`target/release/dbota`, `$profile/dbota`, `"$CARGO_TARGET_DIR/release/dbota"`,
/// a joined one).
const LAUNCHES: &[&str] = &[
    "cargo_bin_exe_dbota",
    "commandnewdbota",
    "get_program",
    "cargoargrun",
    "cargoargsrun",
];

/// The folders a built client lies in: a profile's name, or a variable that holds it.
const PROFILE_FOLDERS: &[&str] = &["release", "debug", "dist", "profile", "profiling"];

/// Whether a line runs `cargo ... run`, however cargo is named (`cargo`, `"$CARGO"`) and whatever
/// options come between.
fn runs_cargo(line: &str) -> bool {
    let words: Vec<String> = line
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|c| matches!(c, '"' | '\'' | '(' | ')' | ';' | '`'))
                .to_ascii_lowercase()
        })
        .collect();
    words.iter().enumerate().any(|(at, word)| {
        matches!(word.as_str(), "cargo" | "$cargo" | "${cargo}")
            && words[at + 1..]
                .iter()
                .take_while(|word| *word != "--")
                .any(|word| word == "run")
    })
}

/// Whether a line starts a game server by its own `Command`, which has to go through the helper
/// that removes the variables that steer a server.
fn starts_server_by_hand(line: &str) -> bool {
    line.match_indices("Command::new(").any(|(at, found)| {
        line[at + found.len()..]
            .to_ascii_lowercase()
            .contains("server")
    })
}

/// The lines (trimmed) of `source` that start the client or a server in a way that gives it the
/// caller's environment.
pub fn launch_lines(source: &str) -> Vec<String> {
    let (text, lines) = squeezed(source);
    let mut numbers = std::collections::BTreeSet::new();
    for launch in LAUNCHES {
        numbers.extend(text.match_indices(launch).map(|(at, _)| lines[at]));
    }
    numbers.extend(text.match_indices("dbota").filter_map(|(at, _)| {
        let before = &text[..at];
        let before = before.strip_suffix("join").unwrap_or(before);
        PROFILE_FOLDERS
            .iter()
            .any(|folder| before.ends_with(folder))
            .then(|| lines[at])
    }));
    let all: Vec<&str> = source.lines().collect();
    for (number, line) in code_lines(source) {
        if runs_cargo(line) || starts_server_by_hand(line) {
            numbers.insert(number);
        }
    }
    numbers
        .into_iter()
        .map(|number| all[number].trim().to_owned())
        .collect()
}

/// Whether a text (comment lines aside) names the user's own managed asset cache, `~/.cache/dbota`
/// (not the gate's `~/.cache/dbota-gate`).
pub fn names_the_managed_cache(source: &str) -> bool {
    code_lines(source).any(|(_, line)| {
        line.match_indices(".cache/dbota").any(|(at, found)| {
            let after = line[at + found.len()..].chars().next();
            !after.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_')
        })
    })
}
