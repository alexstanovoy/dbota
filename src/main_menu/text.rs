//! Text that came from the matchmaking service or a game server, made safe to
//! draw: no control characters, no direction overrides that would make a
//! name read as something else, nothing invisible, and bounded in length.

/// Whether `c` changes how the text around it reads without being seen:
/// direction marks, embeddings, overrides and isolates, joiners and other
/// zero-width characters, the byte order mark.
fn hidden(c: char) -> bool {
    matches!(c,
        '\u{00AD}'
        | '\u{034F}'
        | '\u{061C}'
        | '\u{115F}'..='\u{1160}'
        | '\u{17B4}'..='\u{17B5}'
        | '\u{180B}'..='\u{180F}'
        | '\u{200B}'..='\u{200F}'
        | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{206F}'
        | '\u{3164}'
        | '\u{FE00}'..='\u{FE0F}'
        | '\u{FEFF}'
        | '\u{FFA0}'
        | '\u{FFF0}'..='\u{FFFB}'
        | '\u{E0000}'..='\u{E0FFF}')
}

/// `text` as one line of at most `max` characters: control and hidden
/// characters dropped, runs of white space made one space, an ellipsis where
/// it was cut.
pub fn line(text: &str, max: usize) -> String {
    let mut out = String::with_capacity(text.len().min(max * 4));
    let mut count = 0;
    let mut space = false;
    for c in text.chars() {
        if c.is_control() || hidden(c) {
            if c.is_whitespace() {
                space = !out.is_empty();
            }
            continue;
        }
        if c.is_whitespace() {
            space = !out.is_empty();
            continue;
        }
        let extra = usize::from(space);
        if count + extra + 1 > max {
            out.push('…');
            return out;
        }
        if space {
            out.push(' ');
            space = false;
        }
        out.push(c);
        count += extra + 1;
    }
    out
}

/// Longest nickname, lobby name or ruleset name drawn.
pub const NAME: usize = 32;
/// Longest message drawn (a ban reason, a server's name).
pub const MESSAGE: usize = 200;

/// Someone's name as the service shows other accounts: `nickname#id`.
pub fn who(nickname: &str, id: u64) -> String {
    format!("{}#{id}", line(nickname, NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_cannot_hide_or_reverse_text_or_grow_unbounded() {
        // A right-to-left override would draw "evil\u{202E}gnp.exe" as "evilexe.png".
        assert_eq!(line("evil\u{202E}gnp.exe", NAME), "evilgnp.exe");
        assert_eq!(line("a\u{200B}d\u{2066}m\u{FEFF}in", NAME), "admin");
        assert_eq!(
            line("two\nlines\tand\r\n  gaps ", NAME),
            "two lines and gaps"
        );
        assert_eq!(line(&"x".repeat(10_000), 5), "xxxxx…");
        assert_eq!(line("\u{0007}\u{001B}[31m", NAME), "[31m");
    }
}
