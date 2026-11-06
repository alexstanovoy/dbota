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

/// The text of a string whose opening quote is before `start`, and where it ends.
fn quoted(chars: &[char], start: usize) -> (String, usize) {
    let mut text = String::new();
    let mut at = start;
    while at < chars.len() {
        match chars[at] {
            '"' => return (text, at + 1),
            '\\' if at + 1 < chars.len() => {
                text.push(chars[at + 1]);
                at += 2;
            }
            c => {
                text.push(c);
                at += 1;
            }
        }
    }
    (text, at)
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

/// The text with comment lines dropped and everything but letters, digits and underscores
/// removed, lower-cased: `"$CARGO_TARGET_DIR/release/dbota"` and `target/${profile}/dbota`
/// become the same kind of run, whatever the quoting, concatenation or variable around it.
pub fn compact(source: &str) -> String {
    code_lines(source)
        .flat_map(str::chars)
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The ways to start the client as it is built, which would give it the caller's HOME, found in
/// the text squeezed by [`compact`]: the binary Cargo names, `Command::new("dbota")`, a built
/// command's program, and a `dbota` that closes a path through a build profile's folder
/// (`target/release/dbota`, `$profile/dbota`, `"$CARGO_TARGET_DIR/release/dbota"`, a joined one),
/// whatever the quoting, variable or concatenation around it; and `cargo run`, found in the text,
/// since squeezed together it is also the sentence "cargo runs".
const LAUNCHES: &[&str] = &["cargo_bin_exe_dbota", "commandnewdbota", "get_program"];

/// The folders a built client lies in: a profile's name, or a variable that holds it.
const PROFILE_FOLDERS: &[&str] = &["release", "debug", "dist", "profile", "profiling"];

pub fn client_launches(source: &str) -> usize {
    let text = compact(source);
    let squeezed: usize = LAUNCHES
        .iter()
        .map(|launch| text.match_indices(launch).count())
        .sum();
    let in_profile_folder = text
        .match_indices("dbota")
        .filter(|(at, _)| {
            let before = &text[..*at];
            let before = before.strip_suffix("join").unwrap_or(before);
            PROFILE_FOLDERS
                .iter()
                .any(|folder| before.ends_with(folder))
        })
        .count();
    let cargo_run: usize = code_lines(source)
        .map(|line| {
            line.match_indices("cargo run")
                .filter(|(at, found)| {
                    !line[at + found.len()..]
                        .chars()
                        .next()
                        .is_some_and(char::is_alphanumeric)
                })
                .count()
        })
        .sum();
    squeezed + in_profile_folder + cargo_run
}

/// The lines of a Rust, shell or Python source that are not comments.
fn code_lines(source: &str) -> impl Iterator<Item = &str> {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//") && !line.trim_start().starts_with('#'))
}

/// Whether a text (comment lines aside) names the user's own managed asset cache, `~/.cache/dbota`
/// (not the gate's `~/.cache/dbota-gate`).
pub fn names_the_managed_cache(source: &str) -> bool {
    code_lines(source).any(|line| {
        line.match_indices(".cache/dbota").any(|(at, found)| {
            let after = line[at + found.len()..].chars().next();
            !after.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_')
        })
    })
}
