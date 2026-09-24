//! The pane's recent text, from whichever program draws it: Herdr, tmux,
//! WezTerm, Kitty or iTerm2, each through its own command-line interface.
//! Tried in the configured order; the first that answers wins.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::entities::TerminalIdentity;
use crate::use_cases::ports::OutputSource;

/// How long a terminal may take to answer before it is given up on.
const TIMEOUT: Duration = Duration::from_secs(2);

pub struct TerminalOutput {
    sources: Vec<String>,
    bin_dir: Option<PathBuf>,
}

/// One program to run, with its arguments and environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl TerminalOutput {
    /// `sources` are tried in order: `herdr`, `tmux`, `wezterm`, `kitty`, `iterm2`.
    pub fn new(sources: Vec<String>) -> Self {
        Self {
            sources,
            bin_dir: None,
        }
    }

    /// Programs are looked up in this directory first: for tests with fakes.
    #[cfg(test)]
    fn with_bin_dir(mut self, dir: PathBuf) -> Self {
        self.bin_dir = Some(dir);
        self
    }
}

/// The command that reads the last `lines` lines of the pane, for a source
/// that applies to this terminal; nothing when the identity lacks what
/// that source needs.
pub fn command_for(source: &str, t: &TerminalIdentity, lines: usize) -> Option<Invocation> {
    let n = lines.to_string();
    let invocation = |program: &str, args: Vec<String>, env: Vec<(String, String)>| Invocation {
        program: program.to_string(),
        args,
        env,
    };
    match source {
        "herdr" => {
            let pane = t.herdr_pane.as_deref()?;
            let program = t.herdr_bin.as_deref().unwrap_or("herdr");
            let args = [
                "pane",
                "read",
                pane,
                "--source",
                "recent-unwrapped",
                "--lines",
                &n,
                "--format",
                "text",
            ];
            let env = t
                .herdr_socket
                .iter()
                .map(|s| ("HERDR_SOCKET_PATH".to_string(), s.clone()))
                .collect();
            Some(invocation(
                program,
                args.iter().map(|a| a.to_string()).collect(),
                env,
            ))
        }
        "tmux" => {
            let pane = t.tmux_pane.as_deref()?;
            let mut args: Vec<String> = Vec::new();
            if let Some(socket) = &t.tmux_socket {
                args.extend(["-S".to_string(), socket.clone()]);
            }
            args.extend(
                [
                    "capture-pane",
                    "-p",
                    "-J",
                    "-t",
                    pane,
                    "-S",
                    &format!("-{lines}"),
                ]
                .iter()
                .map(|a| a.to_string()),
            );
            Some(invocation("tmux", args, Vec::new()))
        }
        "wezterm" => {
            let pane = t.wezterm_pane.as_deref()?;
            let args = [
                "cli",
                "get-text",
                "--pane-id",
                pane,
                "--start-line",
                &format!("-{lines}"),
            ];
            Some(invocation(
                "wezterm",
                args.iter().map(|a| a.to_string()).collect(),
                Vec::new(),
            ))
        }
        "kitty" => {
            let window = t.kitty_window.as_deref()?;
            let mut args: Vec<String> = vec!["@".into()];
            if let Some(to) = &t.kitty_listen_on {
                args.extend(["--to".to_string(), to.clone()]);
            }
            args.extend(
                [
                    "get-text",
                    "--match",
                    &format!("id:{window}"),
                    "--extent",
                    "screen",
                ]
                .iter()
                .map(|a| a.to_string()),
            );
            Some(invocation("kitten", args, Vec::new()))
        }
        "iterm2" => {
            let session = t.iterm_session.as_deref()?;
            let uuid = session.rsplit(':').next().unwrap_or(session);
            let script = format!(
                "tell application \"iTerm2\"\n\
                 repeat with w in windows\n\
                 repeat with t in tabs of w\n\
                 repeat with s in sessions of t\n\
                 if id of s is \"{uuid}\" then return contents of s\n\
                 end repeat\nend repeat\nend repeat\nend tell"
            );
            Some(invocation(
                "osascript",
                vec!["-e".into(), script],
                Vec::new(),
            ))
        }
        _ => None,
    }
}

impl TerminalOutput {
    fn run(&self, invocation: &Invocation) -> Option<String> {
        let program = match &self.bin_dir {
            Some(dir) => dir.join(&invocation.program).display().to_string(),
            None => invocation.program.clone(),
        };
        let mut child = Command::new(program)
            .args(&invocation.args)
            .envs(invocation.env.iter().cloned())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut stdout = child.stdout.take()?;
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stdout.read_to_string(&mut text);
            text
        });
        let deadline = Instant::now() + TIMEOUT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
            }
        };
        let text = reader.join().ok()?;
        (status.success() && !text.trim().is_empty()).then_some(text)
    }
}

impl OutputSource for TerminalOutput {
    fn recent(&self, terminal: &TerminalIdentity, lines: usize) -> Option<String> {
        self.sources
            .iter()
            .filter_map(|source| command_for(source, terminal, lines))
            .find_map(|invocation| self.run(&invocation))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn everywhere() -> TerminalIdentity {
        TerminalIdentity {
            program: Some("ghostty".into()),
            tmux_pane: Some("%3".into()),
            tmux_socket: Some("/tmp/tmux-501/default".into()),
            herdr_pane: Some("wS:p1".into()),
            herdr_socket: Some("/home/me/.config/herdr/herdr.sock".into()),
            herdr_bin: Some("/home/me/.local/bin/herdr".into()),
            wezterm_pane: Some("7".into()),
            kitty_window: Some("12".into()),
            kitty_listen_on: Some("unix:/tmp/kitty".into()),
            iterm_session: Some("w0t0p0:ABCD-1234".into()),
        }
    }

    #[test]
    fn each_source_is_asked_the_way_its_program_documents() {
        let t = everywhere();
        let herdr = command_for("herdr", &t, 450).unwrap();
        assert_eq!(herdr.program, "/home/me/.local/bin/herdr");
        assert_eq!(
            herdr.args,
            [
                "pane",
                "read",
                "wS:p1",
                "--source",
                "recent-unwrapped",
                "--lines",
                "450",
                "--format",
                "text"
            ]
        );
        assert_eq!(
            herdr.env,
            vec![(
                "HERDR_SOCKET_PATH".to_string(),
                "/home/me/.config/herdr/herdr.sock".to_string()
            )]
        );
        let tmux = command_for("tmux", &t, 450).unwrap();
        assert_eq!(
            tmux.args,
            [
                "-S",
                "/tmp/tmux-501/default",
                "capture-pane",
                "-p",
                "-J",
                "-t",
                "%3",
                "-S",
                "-450"
            ]
        );
        let wezterm = command_for("wezterm", &t, 450).unwrap();
        assert_eq!(
            wezterm.args,
            ["cli", "get-text", "--pane-id", "7", "--start-line", "-450"]
        );
        let kitty = command_for("kitty", &t, 450).unwrap();
        assert_eq!(
            (kitty.program.as_str(), kitty.args.as_slice()),
            (
                "kitten",
                [
                    "@",
                    "--to",
                    "unix:/tmp/kitty",
                    "get-text",
                    "--match",
                    "id:12",
                    "--extent",
                    "screen"
                ]
                .map(String::from)
                .as_slice()
            )
        );
        let iterm = command_for("iterm2", &t, 450).unwrap();
        assert_eq!(iterm.program, "osascript");
        assert!(iterm.args[1].contains("if id of s is \"ABCD-1234\""));
        assert_eq!(command_for("herdr", &TerminalIdentity::default(), 10), None);
        assert_eq!(command_for("screen", &t, 10), None, "unknown source");
        let bare_tmux = command_for(
            "tmux",
            &TerminalIdentity {
                tmux_pane: Some("%1".into()),
                ..Default::default()
            },
            10,
        )
        .unwrap();
        assert_eq!(bare_tmux.args[0], "capture-pane", "no -S without a socket");
    }

    #[test]
    fn the_first_source_that_answers_wins_and_failures_are_skipped() {
        let dir = std::env::temp_dir().join(format!("kintsu-terminals-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = |name: &str, body: &str| {
            let path = dir.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        script("herdr", "exit 1");
        script("tmux", "printf '$ make\\nboom   \\n'");
        let output =
            TerminalOutput::new(vec!["herdr".into(), "tmux".into()]).with_bin_dir(dir.clone());
        let t = TerminalIdentity {
            herdr_pane: Some("p".into()),
            tmux_pane: Some("%1".into()),
            ..Default::default()
        };
        assert_eq!(output.recent(&t, 10).as_deref(), Some("$ make\nboom   \n"));
        let only_herdr = TerminalOutput::new(vec!["herdr".into()]).with_bin_dir(dir.clone());
        assert_eq!(
            only_herdr.recent(&t, 10),
            None,
            "a failing program is nothing"
        );
        assert_eq!(
            output.recent(&TerminalIdentity::default(), 10),
            None,
            "no pane, nothing asked"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
