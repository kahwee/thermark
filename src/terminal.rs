//! Escaping for untrusted single-line terminal fields. Keep structured data raw.
use std::borrow::Cow;

/// Render control characters and directional formatting visibly rather than
/// allowing a peer, filename, or library caller to alter terminal presentation.
pub fn escape(value: &str) -> Cow<'_, str> {
    fn unsafe_char(ch: char) -> bool {
        ch.is_control()
            || matches!(ch, '\u{061c}' | '\u{200e}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
    }
    if !value.chars().any(unsafe_char) {
        return Cow::Borrowed(value);
    }
    let mut out = String::new();
    for ch in value.chars() {
        if unsafe_char(ch) {
            out.extend(ch.escape_default());
        } else {
            out.push(ch);
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escapes_terminal_commands_newlines_c1_and_bidi() {
        let raw = "B1\x1b]52;c;secret\x07\r\n\u{009b}2J\u{202e}evil\u{2066}";
        let rendered = escape(raw);
        assert!(!rendered.chars().any(char::is_control));
        assert!(!rendered.contains('\u{202e}'));
        assert!(rendered.contains("\\u{1b}]52"));
        assert_eq!(escape("B1-Café 中文"), "B1-Café 中文");
    }
}
