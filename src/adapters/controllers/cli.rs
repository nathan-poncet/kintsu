//! The `kintsu` command line, turned into something a use case understands.

use crate::entities::{CommandLine, CommandLineError, CommandOutcome, ExitStatus, Shell};
use thiserror::Error;

/// What the user or a hook asked the binary to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `kintsu init <shell>`: print the shell integration.
    Init(Shell),
    /// `kintsu triage --status <code> --command <text>`: a hook reports a
    /// finished command line.
    Triage(CommandOutcome),
    /// `kintsu --version`.
    Version,
    /// `kintsu --help`, or no arguments at all.
    Help,
}

/// Why the arguments did not make a command.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CliError {
    /// The first argument is not a subcommand.
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    /// `init` without a shell.
    #[error("init needs a shell: zsh, bash or fish")]
    MissingShell,
    /// `init` with a shell Kintsu has no hook for.
    #[error("unsupported shell `{shell}` (expected {expected})", shell = .0, expected = supported_shells())]
    UnsupportedShell(String),
    /// `triage` without both `--status` and `--command`.
    #[error("triage needs --status <code> --command <text>")]
    IncompleteTriage,
    /// `--status` with something that is not an integer.
    #[error("`--status` needs an integer, got `{0}`")]
    InvalidStatus(String),
    /// `--command` with blank text.
    #[error(transparent)]
    BlankCommand(#[from] CommandLineError),
    /// A flag `triage` does not know.
    #[error("unknown flag `{0}`")]
    UnknownFlag(String),
}

fn supported_shells() -> String {
    let names: Vec<&str> = Shell::ALL.iter().map(|shell| shell.name()).collect();
    names.join(", ")
}

/// Parses the arguments that follow the program name.
pub fn parse_args<'a>(args: impl IntoIterator<Item = &'a str>) -> Result<Command, CliError> {
    let args: Vec<&str> = args.into_iter().collect();
    match args.as_slice() {
        [] | ["--help" | "-h"] => Ok(Command::Help),
        ["--version" | "-V"] => Ok(Command::Version),
        ["init"] => Err(CliError::MissingShell),
        ["init", shell] => Shell::from_name(shell)
            .map(Command::Init)
            .ok_or_else(|| CliError::UnsupportedShell((*shell).to_string())),
        ["triage", flags @ ..] => parse_triage(flags),
        [other, ..] => Err(CliError::UnknownCommand((*other).to_string())),
    }
}

fn parse_triage(flags: &[&str]) -> Result<Command, CliError> {
    let mut status = None;
    let mut command = None;
    let mut flags = flags.iter();
    while let Some(flag) = flags.next() {
        let value = *flags.next().ok_or(CliError::IncompleteTriage)?;
        match *flag {
            "--status" => {
                let code = value
                    .parse::<i32>()
                    .map_err(|_| CliError::InvalidStatus(value.to_string()))?;
                status = Some(ExitStatus::new(code));
            }
            "--command" => command = Some(CommandLine::new(value)?),
            other => return Err(CliError::UnknownFlag(other.to_string())),
        }
    }
    match (status, command) {
        (Some(status), Some(command)) => Ok(Command::Triage(CommandOutcome::new(command, status))),
        _ => Err(CliError::IncompleteTriage),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triage(command: &str, code: i32) -> Command {
        Command::Triage(CommandOutcome::new(
            CommandLine::new(command).unwrap(),
            ExitStatus::new(code),
        ))
    }

    #[test]
    fn no_arguments_or_a_help_flag_ask_for_help() {
        for args in [vec![], vec!["--help"], vec!["-h"]] {
            assert_eq!(parse_args(args), Ok(Command::Help));
        }
    }

    #[test]
    fn a_version_flag_asks_for_the_version() {
        assert_eq!(parse_args(["--version"]), Ok(Command::Version));
        assert_eq!(parse_args(["-V"]), Ok(Command::Version));
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
    fn triage_flags_are_accepted_in_any_order() {
        let expected = triage("make", 2);
        assert_eq!(
            parse_args(["triage", "--status", "2", "--command", "make"]),
            Ok(expected.clone())
        );
        assert_eq!(
            parse_args(["triage", "--command", "make", "--status", "2"]),
            Ok(expected)
        );
    }

    #[test]
    fn triage_refuses_incomplete_arguments() {
        assert_eq!(
            parse_args(["triage", "--status", "2"]),
            Err(CliError::IncompleteTriage)
        );
        assert_eq!(
            parse_args(["triage", "--status"]),
            Err(CliError::IncompleteTriage)
        );
    }

    #[test]
    fn triage_refuses_a_status_that_is_not_a_number() {
        assert_eq!(
            parse_args(["triage", "--status", "two", "--command", "make"]),
            Err(CliError::InvalidStatus("two".into()))
        );
    }

    #[test]
    fn triage_refuses_a_blank_command() {
        assert_eq!(
            parse_args(["triage", "--status", "1", "--command", "  "]),
            Err(CliError::BlankCommand(CommandLineError::Blank))
        );
    }

    #[test]
    fn triage_refuses_a_flag_it_does_not_know() {
        assert_eq!(
            parse_args(["triage", "--bogus", "1"]),
            Err(CliError::UnknownFlag("--bogus".into()))
        );
    }

    #[test]
    fn anything_else_is_an_unknown_command() {
        assert_eq!(
            parse_args(["dance"]),
            Err(CliError::UnknownCommand("dance".into()))
        );
    }
}
