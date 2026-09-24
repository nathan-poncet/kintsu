//! Where a shell draws, and how to cut a command's output out of what the
//! terminal remembers.

/// What identifies the pane a shell runs in, as the terminal or the
/// multiplexer names it; each field is set from that program's variable.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TerminalIdentity {
    /// `TERM_PROGRAM`.
    pub program: Option<String>,
    /// `TMUX_PANE`, with `TMUX` (socket path first) to reach the server.
    pub tmux_pane: Option<String>,
    pub tmux_socket: Option<String>,
    /// `HERDR_PANE_ID`, with `HERDR_SOCKET_PATH` and `HERDR_BIN_PATH`.
    pub herdr_pane: Option<String>,
    pub herdr_socket: Option<String>,
    pub herdr_bin: Option<String>,
    /// `WEZTERM_PANE`.
    pub wezterm_pane: Option<String>,
    /// `KITTY_WINDOW_ID`, with `KITTY_LISTEN_ON`.
    pub kitty_window: Option<String>,
    pub kitty_listen_on: Option<String>,
    /// `ITERM_SESSION_ID`.
    pub iterm_session: Option<String>,
}

impl TerminalIdentity {
    /// Whether any source could be asked at all.
    pub fn is_known(&self) -> bool {
        self.tmux_pane.is_some()
            || self.herdr_pane.is_some()
            || self.wezterm_pane.is_some()
            || self.kitty_window.is_some()
            || self.iterm_session.is_some()
    }
}

/// The output of `command` from a screen dump: what follows the last line
/// that echoes the command (the prompt line), trailing blank lines dropped,
/// at most `max_lines` kept from the end. When the command is not found on
/// screen, the last `max_lines` are returned as they are.
pub fn output_after(screen: &str, command: &str, max_lines: usize) -> Option<String> {
    let lines: Vec<&str> = screen.lines().map(str::trim_end).collect();
    let command = command.trim();
    let start = lines
        .iter()
        .rposition(|l| !command.is_empty() && l.contains(command))
        .map_or(0, |i| i + 1);
    let mut kept: Vec<&str> = lines[start..].to_vec();
    while kept.last().is_some_and(|l| l.is_empty()) {
        kept.pop();
    }
    while kept.first().is_some_and(|l| l.is_empty()) {
        kept.remove(0);
    }
    if kept.is_empty() {
        return None;
    }
    if kept.len() > max_lines {
        kept = kept[kept.len() - max_lines..].to_vec();
    }
    Some(kept.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: &str = "\
$ ls
a  b
$ npm run build   
> build
error TS2307: Cannot find module 'left-pad'
    at Object.<anonymous>

";

    #[test]
    fn the_output_is_what_follows_the_last_echo_of_the_command() {
        let out = output_after(SCREEN, "npm run build", 400).unwrap();
        assert_eq!(
            out,
            "> build\nerror TS2307: Cannot find module 'left-pad'\n    at Object.<anonymous>"
        );
    }

    #[test]
    fn only_the_last_lines_are_kept_and_an_unknown_command_gives_the_tail() {
        assert_eq!(
            output_after(SCREEN, "npm run build", 1).unwrap(),
            "    at Object.<anonymous>"
        );
        assert_eq!(
            output_after(SCREEN, "cargo test", 2).unwrap(),
            "error TS2307: Cannot find module 'left-pad'\n    at Object.<anonymous>"
        );
        assert_eq!(
            output_after("$ true\n\n", "true", 10),
            None,
            "nothing after the echo"
        );
        assert_eq!(output_after("", "x", 10), None);
    }

    #[test]
    fn a_known_pane_is_one_with_any_identifier() {
        assert!(!TerminalIdentity::default().is_known());
        assert!(
            TerminalIdentity {
                herdr_pane: Some("wS:p1".into()),
                ..Default::default()
            }
            .is_known()
        );
        assert!(
            !TerminalIdentity {
                program: Some("ghostty".into()),
                ..Default::default()
            }
            .is_known()
        );
    }
}
