//! The `kintsu` command line, turned into something a use case understands.
//! Only argv is read here; the environment is the composition root's job.

use thiserror::Error;

use crate::entities::TerminalIdentity;
use crate::entities::{
    CommandLine, CommandLineError, CommandOutcome, Duration, ExitStatus, SessionId, Shell,
};
use crate::use_cases::TriageInput;

/// Which silence `kintsu ignore` asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeFlag {
    /// `--command`, the default: this exact command line, everywhere.
    Command,
    /// `--dir`: this program, in the current directory.
    Dir,
    /// `--session`: this program, in this shell.
    Session,
    /// `--always`: this program, everywhere.
    Always,
}

/// What `kintsu daemon` should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonAction {
    Run,
    Stop,
    Status,
}

/// What the user or a hook asked the binary to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `kintsu init <shell>`: print the shell integration.
    Init(Shell),
    /// `kintsu triage --status <code> --command <text> …`: a hook reports.
    /// `--signal-pid` names a shell to poke with SIGUSR1 when a message arrives.
    Triage {
        input: Box<TriageInput>,
        signal_pid: Option<u32>,
    },
    /// `kintsu subscribe [--session <id>]`: print bubbles as they come.
    Subscribe { session: Option<SessionId> },
    /// `kintsu pending [--session <id>]`: print the bubbles not yet seen.
    Pending { session: Option<SessionId> },
    /// `kintsu daemon [run|stop|status]`.
    Daemon(DaemonAction),
    /// `kintsu fix [--raw]`: the corrected command for the last failure.
    Fix { raw: bool },
    /// `kintsu why`: an explanation.
    Why,
    /// `kintsu agent [--with <name>] [words…]`: hand the case over.
    Agent {
        with: Option<String>,
        words: Option<String>,
    },
    /// `kintsu privacy`: what would be sent.
    Privacy,
    /// `kintsu ignore [--command|--dir|--session|--always] [program]`.
    Ignore {
        program: Option<String>,
        scope: ScopeFlag,
    },
    /// `kintsu mute [duration]`.
    Mute(Duration),
    /// `kintsu doctor`.
    Doctor,
    /// `kintsu default-config`.
    DefaultConfig,
    /// `kintsu config path`.
    ConfigPath,
    /// `kintsu --version`.
    Version,
    /// `kintsu --help`, or no arguments at all.
    Help,
}

/// Why the arguments did not make a command.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CliError {
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    #[error("init needs a shell: zsh, bash or fish")]
    MissingShell,
    #[error("unsupported shell `{shell}` (expected {expected})", shell = .0, expected = supported_shells())]
    UnsupportedShell(String),
    #[error("triage needs --status <code> --command <text>")]
    IncompleteTriage,
    #[error("`{flag}` needs an integer, got `{value}`")]
    InvalidInteger { flag: String, value: String },
    #[error(transparent)]
    BlankCommand(#[from] CommandLineError),
    #[error("unknown flag `{0}`")]
    UnknownFlag(String),
    #[error("`--with` needs an agent name")]
    MissingAgentName,
    #[error("cannot read `{0}` as a duration (try 30m, 1h, 90s)")]
    InvalidDuration(String),
    #[error("config knows one subcommand: path")]
    UnknownConfigSubcommand,
    #[error("daemon knows run, stop and status")]
    UnknownDaemonAction,
    #[error("`{0}` needs a value")]
    MissingValue(String),
}

fn supported_shells() -> String {
    let names: Vec<&str> = Shell::ALL.iter().map(|shell| shell.name()).collect();
    names.join(", ")
}

/// Parses the arguments that follow the program name.
pub fn parse_args<'a>(args: impl IntoIterator<Item = &'a str>) -> Result<Command, CliError> {
    let args: Vec<&str> = args.into_iter().collect();
    match args.as_slice() {
        [] | ["--help" | "-h" | "help"] => Ok(Command::Help),
        ["--version" | "-V" | "version"] => Ok(Command::Version),
        ["init"] => Err(CliError::MissingShell),
        ["init", shell] => Shell::from_name(shell)
            .map(Command::Init)
            .ok_or_else(|| CliError::UnsupportedShell((*shell).to_string())),
        ["triage", flags @ ..] => parse_triage(flags),
        ["subscribe", flags @ ..] => {
            parse_session_flag(flags).map(|session| Command::Subscribe { session })
        }
        ["pending", flags @ ..] => {
            parse_session_flag(flags).map(|session| Command::Pending { session })
        }
        ["daemon"] | ["daemon", "run"] => Ok(Command::Daemon(DaemonAction::Run)),
        ["daemon", "stop"] => Ok(Command::Daemon(DaemonAction::Stop)),
        ["daemon", "status"] => Ok(Command::Daemon(DaemonAction::Status)),
        ["daemon", ..] => Err(CliError::UnknownDaemonAction),
        ["fix"] => Ok(Command::Fix { raw: false }),
        ["fix", "--raw"] => Ok(Command::Fix { raw: true }),
        ["fix", other, ..] => Err(CliError::UnknownFlag((*other).to_string())),
        ["why" | "explain"] => Ok(Command::Why),
        ["agent", rest @ ..] => parse_agent(rest),
        ["privacy"] => Ok(Command::Privacy),
        ["ignore", rest @ ..] => parse_ignore(rest),
        ["mute"] => Ok(Command::Mute(Duration::from_mins(60))),
        ["mute", text] => parse_duration(text)
            .map(Command::Mute)
            .ok_or_else(|| CliError::InvalidDuration((*text).to_string())),
        ["doctor"] => Ok(Command::Doctor),
        ["default-config"] => Ok(Command::DefaultConfig),
        ["config", "path"] => Ok(Command::ConfigPath),
        ["config", ..] => Err(CliError::UnknownConfigSubcommand),
        [other, ..] => Err(CliError::UnknownCommand((*other).to_string())),
    }
}

fn parse_triage(flags: &[&str]) -> Result<Command, CliError> {
    let mut status = None;
    let mut command = None;
    let mut cwd = None;
    let mut session = None;
    let mut shell = None;
    let mut duration = None;
    let mut signal_pid = None;
    let mut flags = flags.iter();
    while let Some(flag) = flags.next() {
        let value = *flags.next().ok_or(CliError::IncompleteTriage)?;
        match *flag {
            "--status" => status = Some(ExitStatus::new(integer(flag, value)?)),
            "--command" => command = Some(CommandLine::new(value)?),
            "--cwd" => cwd = Some(value.to_string()).filter(|c| !c.is_empty()),
            "--session" => session = Some(SessionId::new(value)).filter(|s| !s.as_str().is_empty()),
            "--shell" => shell = Shell::from_name(value),
            "--duration-ms" => duration = Some(Duration::from_millis(integer::<u64>(flag, value)?)),
            "--signal-pid" => signal_pid = Some(integer::<u32>(flag, value)?),
            other => return Err(CliError::UnknownFlag(other.to_string())),
        }
    }
    let (Some(status), Some(command)) = (status, command) else {
        return Err(CliError::IncompleteTriage);
    };
    let mut outcome = CommandOutcome::new(command, status);
    if let Some(d) = duration {
        outcome = outcome.lasting(d);
    }
    Ok(Command::Triage {
        input: Box::new(TriageInput {
            outcome,
            cwd,
            session,
            shell,
            terminal: TerminalIdentity::default(),
        }),
        signal_pid,
    })
}

fn parse_session_flag(flags: &[&str]) -> Result<Option<SessionId>, CliError> {
    match flags {
        [] => Ok(None),
        ["--session", id] => Ok(Some(SessionId::new(*id)).filter(|s| !s.as_str().is_empty())),
        ["--session"] => Err(CliError::MissingValue("--session".into())),
        [other, ..] => Err(CliError::UnknownFlag((*other).to_string())),
    }
}

fn integer<T: std::str::FromStr>(flag: &str, value: &str) -> Result<T, CliError> {
    value.parse::<T>().map_err(|_| CliError::InvalidInteger {
        flag: flag.to_string(),
        value: value.to_string(),
    })
}

fn parse_agent(rest: &[&str]) -> Result<Command, CliError> {
    let (with, words) = match rest {
        ["--with"] => return Err(CliError::MissingAgentName),
        ["--with", name, words @ ..] => (Some((*name).to_string()), words),
        words => (None, words),
    };
    if let Some(flag) = words.iter().find(|w| w.starts_with("--")) {
        return Err(CliError::UnknownFlag((*flag).to_string()));
    }
    let words = words.join(" ");
    Ok(Command::Agent {
        with,
        words: Some(words).filter(|w| !w.trim().is_empty()),
    })
}

fn parse_ignore(rest: &[&str]) -> Result<Command, CliError> {
    let mut scope = ScopeFlag::Command;
    let mut program = None;
    for arg in rest {
        match *arg {
            "--command" => scope = ScopeFlag::Command,
            "--dir" => scope = ScopeFlag::Dir,
            "--session" => scope = ScopeFlag::Session,
            "--always" => scope = ScopeFlag::Always,
            flag if flag.starts_with('-') => return Err(CliError::UnknownFlag(flag.to_string())),
            name => program = Some(name.to_string()),
        }
    }
    Ok(Command::Ignore { program, scope })
}

/// `90s`, `30m`, `1h`, `1h30m`, or bare minutes.
pub fn parse_duration(text: &str) -> Option<Duration> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(minutes) = text.parse::<u64>() {
        return Some(Duration::from_mins(minutes));
    }
    let mut total = 0u64;
    let mut number = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            number.push(c);
            continue;
        }
        let n: u64 = number.parse().ok()?;
        number.clear();
        total += match c {
            'h' => n * 3_600_000,
            'm' => n * 60_000,
            's' => n * 1_000,
            _ => return None,
        };
    }
    if !number.is_empty() {
        return None;
    }
    Some(Duration::from_millis(total))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triage(args: &[&str]) -> TriageInput {
        match parse_args(args.iter().copied()).unwrap() {
            Command::Triage { input, .. } => *input,
            other => panic!("expected triage, got {other:?}"),
        }
    }

    #[test]
    fn triage_can_name_a_shell_to_signal_and_the_daemon_commands_parse() {
        let Command::Triage { signal_pid, .. } = parse_args([
            "triage",
            "--status",
            "1",
            "--command",
            "x",
            "--signal-pid",
            "4242",
        ])
        .unwrap() else {
            panic!()
        };
        assert_eq!(signal_pid, Some(4242));
        assert_eq!(
            parse_args(["daemon"]),
            Ok(Command::Daemon(DaemonAction::Run))
        );
        assert_eq!(
            parse_args(["daemon", "stop"]),
            Ok(Command::Daemon(DaemonAction::Stop))
        );
        assert_eq!(
            parse_args(["daemon", "status"]),
            Ok(Command::Daemon(DaemonAction::Status))
        );
        assert_eq!(
            parse_args(["daemon", "dance"]),
            Err(CliError::UnknownDaemonAction)
        );
        assert_eq!(
            parse_args(["subscribe"]),
            Ok(Command::Subscribe { session: None })
        );
        assert_eq!(
            parse_args(["subscribe", "--session", "7"]),
            Ok(Command::Subscribe {
                session: Some(SessionId::new("7"))
            })
        );
        assert_eq!(
            parse_args(["pending", "--session", ""]),
            Ok(Command::Pending { session: None })
        );
        assert_eq!(
            parse_args(["pending", "--x"]),
            Err(CliError::UnknownFlag("--x".into()))
        );
        assert_eq!(
            parse_args(["subscribe", "--session"]),
            Err(CliError::MissingValue("--session".into()))
        );
    }

    #[test]
    fn no_arguments_or_a_help_flag_ask_for_help() {
        for args in [vec![], vec!["--help"], vec!["-h"], vec!["help"]] {
            assert_eq!(parse_args(args), Ok(Command::Help));
        }
        assert_eq!(parse_args(["--version"]), Ok(Command::Version));
    }

    #[test]
    fn init_names_a_supported_shell() {
        assert_eq!(parse_args(["init", "fish"]), Ok(Command::Init(Shell::Fish)));
        assert_eq!(parse_args(["init"]), Err(CliError::MissingShell));
        assert_eq!(
            parse_args(["init", "nu"]),
            Err(CliError::UnsupportedShell("nu".into()))
        );
    }

    #[test]
    fn triage_reads_every_flag_in_any_order() {
        let input = triage(&[
            "triage",
            "--cwd",
            "/w",
            "--command",
            "make",
            "--status",
            "2",
            "--session",
            "42",
            "--shell",
            "zsh",
            "--duration-ms",
            "12000",
        ]);
        assert_eq!(input.outcome.command().as_str(), "make");
        assert_eq!(input.outcome.status(), ExitStatus::new(2));
        assert_eq!(input.outcome.duration(), Some(Duration::from_secs(12)));
        assert_eq!(input.cwd.as_deref(), Some("/w"));
        assert_eq!(input.session, Some(SessionId::new("42")));
        assert_eq!(input.shell, Some(Shell::Zsh));
        let bare = triage(&[
            "triage",
            "--status",
            "0",
            "--command",
            "ls",
            "--cwd",
            "",
            "--session",
            "",
        ]);
        assert_eq!((bare.cwd, bare.session, bare.shell), (None, None, None));
    }

    #[test]
    fn triage_refuses_incomplete_or_wrong_arguments() {
        assert_eq!(
            parse_args(["triage", "--status", "2"]),
            Err(CliError::IncompleteTriage)
        );
        assert_eq!(
            parse_args(["triage", "--status"]),
            Err(CliError::IncompleteTriage)
        );
        assert_eq!(
            parse_args(["triage", "--status", "two", "--command", "make"]),
            Err(CliError::InvalidInteger {
                flag: "--status".into(),
                value: "two".into()
            })
        );
        assert_eq!(
            parse_args(["triage", "--status", "1", "--command", "  "]),
            Err(CliError::BlankCommand(CommandLineError::Blank))
        );
        assert_eq!(
            parse_args(["triage", "--bogus", "1"]),
            Err(CliError::UnknownFlag("--bogus".into()))
        );
    }

    #[test]
    fn the_actions_on_the_last_failure_parse() {
        assert_eq!(parse_args(["fix"]), Ok(Command::Fix { raw: false }));
        assert_eq!(parse_args(["fix", "--raw"]), Ok(Command::Fix { raw: true }));
        assert_eq!(parse_args(["why"]), Ok(Command::Why));
        assert_eq!(parse_args(["privacy"]), Ok(Command::Privacy));
        assert_eq!(
            parse_args(["agent"]),
            Ok(Command::Agent {
                with: None,
                words: None
            })
        );
        assert_eq!(
            parse_args(["agent", "--with", "codex", "only", "the", "tests"]),
            Ok(Command::Agent {
                with: Some("codex".into()),
                words: Some("only the tests".into())
            })
        );
        assert_eq!(
            parse_args(["agent", "--with"]),
            Err(CliError::MissingAgentName)
        );
        assert_eq!(
            parse_args(["agent", "--yolo"]),
            Err(CliError::UnknownFlag("--yolo".into()))
        );
    }

    #[test]
    fn ignore_defaults_to_the_command_and_takes_a_scope_and_a_program() {
        assert_eq!(
            parse_args(["ignore"]),
            Ok(Command::Ignore {
                program: None,
                scope: ScopeFlag::Command
            })
        );
        assert_eq!(
            parse_args(["ignore", "--dir"]),
            Ok(Command::Ignore {
                program: None,
                scope: ScopeFlag::Dir
            })
        );
        assert_eq!(
            parse_args(["ignore", "--always", "make"]),
            Ok(Command::Ignore {
                program: Some("make".into()),
                scope: ScopeFlag::Always
            })
        );
        assert_eq!(
            parse_args(["ignore", "--session"]),
            Ok(Command::Ignore {
                program: None,
                scope: ScopeFlag::Session
            })
        );
        assert_eq!(
            parse_args(["ignore", "--x"]),
            Err(CliError::UnknownFlag("--x".into()))
        );
    }

    #[test]
    fn mute_takes_a_duration_and_defaults_to_an_hour() {
        assert_eq!(
            parse_args(["mute"]),
            Ok(Command::Mute(Duration::from_mins(60)))
        );
        assert_eq!(
            parse_args(["mute", "30m"]),
            Ok(Command::Mute(Duration::from_mins(30)))
        );
        assert_eq!(
            parse_args(["mute", "1h30m"]),
            Ok(Command::Mute(Duration::from_mins(90)))
        );
        assert_eq!(
            parse_args(["mute", "90s"]),
            Ok(Command::Mute(Duration::from_secs(90)))
        );
        assert_eq!(
            parse_args(["mute", "15"]),
            Ok(Command::Mute(Duration::from_mins(15)))
        );
        assert_eq!(
            parse_args(["mute", "soon"]),
            Err(CliError::InvalidDuration("soon".into()))
        );
        assert_eq!(parse_duration("1h30"), None);
    }

    #[test]
    fn the_setup_commands_parse() {
        assert_eq!(parse_args(["doctor"]), Ok(Command::Doctor));
        assert_eq!(parse_args(["default-config"]), Ok(Command::DefaultConfig));
        assert_eq!(parse_args(["config", "path"]), Ok(Command::ConfigPath));
        assert_eq!(
            parse_args(["config"]),
            Err(CliError::UnknownConfigSubcommand)
        );
        assert_eq!(
            parse_args(["dance"]),
            Err(CliError::UnknownCommand("dance".into()))
        );
    }
}
