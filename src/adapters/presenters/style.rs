//! The one decoration: a gold seam on the left of every line Kintsu writes.

use crate::entities::UiMode;

/// How lines are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    /// ANSI colours and weights; off under `NO_COLOR` or a pipe.
    pub color: bool,
    /// `| Enter ->` instead of `▎ ⏎ →`.
    pub ascii: bool,
    /// Toast, hint or silent.
    pub mode: UiMode,
    /// Words become OSC 8 hyperlinks; needs a terminal, so `color` too.
    pub links: bool,
}

const GOLD: &str = "\x1b[38;5;179m";
const RED: &str = "\x1b[31m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

impl Style {
    /// No colour, no glyphs: for text that is reused rather than printed.
    pub const BARE: Style = Style {
        color: false,
        ascii: true,
        mode: UiMode::Toast,
        links: false,
    };
    /// The same, named for the tests.
    #[cfg(test)]
    pub const PLAIN: Style = Style::BARE;

    pub fn seam(&self) -> String {
        let bar = if self.ascii { "|" } else { "▎" };
        if self.color {
            format!("{GOLD}{bar}{RESET} ")
        } else {
            format!("{bar} ")
        }
    }

    /// One line of the bubble.
    pub fn line(&self, text: &str) -> String {
        format!("{}{text}", self.seam())
    }

    /// Several lines, each with its seam.
    pub fn lines(&self, text: &str) -> String {
        text.lines()
            .map(|l| self.line(l))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn bold(&self, text: &str) -> String {
        if self.color {
            format!("{BOLD}{text}{RESET}")
        } else {
            text.to_string()
        }
    }

    pub fn dim(&self, text: &str) -> String {
        if self.color {
            format!("{DIM}{text}{RESET}")
        } else {
            text.to_string()
        }
    }

    /// Red is for one thing only: a destructive suggestion.
    pub fn warn(&self, text: &str) -> String {
        if self.color {
            format!("{RED}{text}{RESET}")
        } else {
            text.to_string()
        }
    }

    /// `text` as a clickable word pointing to `url`, when links are drawn.
    pub fn link(&self, url: &str, text: &str) -> String {
        if self.links && self.color {
            format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
        } else {
            text.to_string()
        }
    }

    /// The Enter key, as the panel names it.
    pub fn enter(&self) -> &'static str {
        if self.ascii { "Enter" } else { "⏎" }
    }

    pub fn ellipsis(&self) -> &'static str {
        if self.ascii { "..." } else { "…" }
    }

    pub fn dot(&self) -> &'static str {
        if self.ascii { " - " } else { " · " }
    }

    pub fn warning_sign(&self) -> &'static str {
        if self.ascii { "!" } else { "⚠" }
    }

    /// A command echo, cut at `max` characters.
    pub fn abbreviate(&self, text: &str, max: usize) -> String {
        let count = text.chars().count();
        if count <= max {
            return text.to_string();
        }
        let cut: String = text.chars().take(max.saturating_sub(1)).collect();
        format!("{}{}", cut.trim_end(), self.ellipsis())
    }
}

/// Rows `text` takes on a terminal `columns` wide: escape sequences take
/// no room, long lines wrap.
pub fn screen_rows(text: &str, columns: u16) -> u16 {
    let width = usize::from(columns.max(1));
    text.lines()
        .map(|line| visible_chars(line).max(1).div_ceil(width))
        .map(|rows| u16::try_from(rows).unwrap_or(u16::MAX))
        .fold(0u16, u16::saturating_add)
}

/// The characters a line shows once its CSI and OSC sequences are skipped.
fn visible_chars(line: &str) -> usize {
    let mut count = 0;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            count += 1;
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                let mut previous = '\0';
                for c in chars.by_ref() {
                    if c == '\x07' || (previous == '\x1b' && c == '\\') {
                        break;
                    }
                    previous = c;
                }
            }
            _ => {}
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_rows_skip_escapes_and_count_wrapped_lines() {
        assert_eq!(screen_rows("", 80), 0);
        assert_eq!(screen_rows("a\nb", 80), 2);
        assert_eq!(screen_rows(&"x".repeat(100), 80), 2);
        assert_eq!(screen_rows("\n\n", 80), 2, "blank lines are rows too");
        let linked =
            "\x1b[1mbold\x1b[0m \x1b]8;;kintsu://act?case=c&do=why\x1b\\word\x1b]8;;\x1b\\";
        assert_eq!(screen_rows(linked, 10), 1);
        let colour = Style {
            color: true,
            ascii: false,
            mode: UiMode::Toast,
            links: true,
        };
        assert_eq!(screen_rows(&colour.lines("one\ntwo"), 80), 2);
    }

    #[test]
    fn colour_adds_a_gold_seam_and_plain_does_not() {
        let colour = Style {
            color: true,
            ascii: false,
            mode: UiMode::Toast,
            links: false,
        };
        assert_eq!(colour.line("hi"), "\x1b[38;5;179m▎\x1b[0m hi");
        assert_eq!(Style::PLAIN.line("hi"), "| hi");
        assert_eq!(Style::PLAIN.lines("a\nb"), "| a\n| b");
        assert_eq!(Style::PLAIN.bold("x"), "x");
        assert_eq!(colour.warn("x"), "\x1b[31mx\x1b[0m");
    }

    #[test]
    fn a_link_is_an_osc_8_hyperlink_only_on_a_colour_terminal_that_wants_them() {
        let linked = Style {
            color: true,
            ascii: false,
            mode: UiMode::Toast,
            links: true,
        };
        assert_eq!(
            linked.link("kintsu://act?case=c&do=why", "kintsu why"),
            "\x1b]8;;kintsu://act?case=c&do=why\x1b\\kintsu why\x1b]8;;\x1b\\"
        );
        let no_links = Style {
            links: false,
            ..linked
        };
        assert_eq!(no_links.link("kintsu://x", "kintsu why"), "kintsu why");
        let piped = Style {
            color: false,
            ..linked
        };
        assert_eq!(
            piped.link("kintsu://x", "kintsu why"),
            "kintsu why",
            "no escapes into a pipe"
        );
    }

    #[test]
    fn long_commands_are_cut_with_an_ellipsis() {
        assert_eq!(Style::PLAIN.abbreviate("make test", 60), "make test");
        assert_eq!(Style::PLAIN.abbreviate("abcdefghij", 5), "abcd...");
        let s = Style {
            color: false,
            ascii: false,
            mode: UiMode::Toast,
            links: false,
        };
        assert_eq!(s.abbreviate("abcdefghij", 5), "abcd…");
    }
}
