//! kintsu — when a command fails, offer to hand it to an AI agent.
//!
//! Day-zero skeleton: the shell hooks are real, `triage` only prints a hint.
//! Where this is going: docs/VISION.md.

use std::env;
use std::process::ExitCode;

const ZSH_HOOK: &str = include_str!("../shell/kintsu.zsh");
const BASH_HOOK: &str = include_str!("../shell/kintsu.bash");
const FISH_HOOK: &str = include_str!("../shell/kintsu.fish");

const USAGE: &str = "\
kintsu — when a command fails, hand it to your agent

Usage:
  kintsu init <zsh|bash|fish>       print the shell hook to eval or source
  kintsu triage --status <code> --command <text>
                                    called by the hook after each command
  kintsu --version | --help
";

/// Exit statuses that mean the user stopped the command, not that it broke:
/// SIGINT (Ctrl-C), SIGPIPE (`| head`), SIGTSTP (Ctrl-Z).
const INTENTIONAL_STATUSES: &[i32] = &[130, 141, 148];

/// Longest command echoed back in the hint before it is shortened.
const HINT_COMMAND_WIDTH: usize = 60;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["init", shell] => init(shell),
        ["init"] => usage_error("init needs a shell: zsh, bash or fish"),
        ["triage", rest @ ..] => triage(rest),
        ["--version" | "-V"] => {
            println!("kintsu {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        [] | ["--help" | "-h"] => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        [other, ..] => usage_error(&format!("unknown command `{other}`")),
    }
}

fn init(shell: &str) -> ExitCode {
    let hook = match shell {
        "zsh" => ZSH_HOOK,
        "bash" => BASH_HOOK,
        "fish" => FISH_HOOK,
        other => {
            return usage_error(&format!("unsupported shell `{other}` (zsh, bash or fish)"));
        }
    };
    print!("{hook}");
    ExitCode::SUCCESS
}

fn triage(args: &[&str]) -> ExitCode {
    let Some(report) = FailureReport::parse(args) else {
        return usage_error("triage needs --status <code> --command <text>");
    };
    if let Some(hint) = hint_for(&report) {
        eprintln!("{hint}");
    }
    ExitCode::SUCCESS
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("kintsu: {message}\n\n{USAGE}");
    ExitCode::from(2)
}

/// What the shell hook knows about the command that just finished.
#[derive(Debug, PartialEq)]
struct FailureReport {
    status: i32,
    command: String,
}

impl FailureReport {
    fn parse(args: &[&str]) -> Option<Self> {
        let mut status = None;
        let mut command = None;
        let mut it = args.iter();
        while let Some(flag) = it.next() {
            match *flag {
                "--status" => status = it.next()?.parse().ok(),
                "--command" => command = Some((*it.next()?).to_string()),
                _ => return None,
            }
        }
        Some(Self {
            status: status?,
            command: command?,
        })
    }
}

/// The line the hook prints after a command, or nothing when it should stay quiet.
fn hint_for(report: &FailureReport) -> Option<String> {
    if report.status == 0 || INTENTIONAL_STATUSES.contains(&report.status) {
        return None;
    }
    let command = shorten(&report.command, HINT_COMMAND_WIDTH);
    if command.is_empty() {
        return None;
    }
    Some(format!(
        "kintsu · `{command}` exited {} · agent hand-off not wired yet, see docs/VISION.md",
        report.status
    ))
}

fn shorten(text: &str, width: usize) -> String {
    let single_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.chars().count() <= width {
        return single_line;
    }
    let cut: String = single_line.chars().take(width - 1).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(status: i32, command: &str) -> FailureReport {
        FailureReport {
            status,
            command: command.to_string(),
        }
    }

    #[test]
    fn a_successful_command_stays_quiet() {
        assert_eq!(hint_for(&report(0, "ls")), None);
    }

    #[test]
    fn ctrl_c_broken_pipes_and_ctrl_z_are_not_failures() {
        for status in [130, 141, 148] {
            assert_eq!(
                hint_for(&report(status, "sleep 10")),
                None,
                "status {status}"
            );
        }
    }

    #[test]
    fn a_failure_names_the_command_and_its_status() {
        let hint = hint_for(&report(127, "gti status")).unwrap();
        assert!(hint.contains("`gti status`"), "{hint}");
        assert!(hint.contains("127"), "{hint}");
    }

    #[test]
    fn a_blank_command_line_stays_quiet_even_on_failure() {
        assert_eq!(hint_for(&report(1, "   \n ")), None);
    }

    #[test]
    fn a_long_or_multiline_command_is_squeezed_onto_one_short_line() {
        let command = format!("echo one \\\n  two {}", "x".repeat(100));
        let hint = hint_for(&report(1, &command)).unwrap();
        assert!(!hint.contains('\n'));
        assert!(hint.contains("echo one \\ two xxx"), "{hint}");
        assert!(hint.contains('…'), "{hint}");
    }

    #[test]
    fn triage_arguments_are_parsed_in_any_order() {
        let forward = FailureReport::parse(&["--status", "2", "--command", "make"]);
        let backward = FailureReport::parse(&["--command", "make", "--status", "2"]);
        assert_eq!(forward, Some(report(2, "make")));
        assert_eq!(forward, backward);
    }

    #[test]
    fn triage_refuses_incomplete_or_unknown_arguments() {
        assert_eq!(FailureReport::parse(&["--status", "2"]), None);
        assert_eq!(
            FailureReport::parse(&["--status", "two", "--command", "make"]),
            None
        );
        assert_eq!(FailureReport::parse(&["--bogus", "1"]), None);
    }

    #[test]
    fn a_hook_exists_for_each_supported_shell() {
        assert!(ZSH_HOOK.contains("add-zsh-hook"));
        assert!(BASH_HOOK.contains("PROMPT_COMMAND"));
        assert!(FISH_HOOK.contains("fish_postexec"));
        for hook in [ZSH_HOOK, BASH_HOOK, FISH_HOOK] {
            assert!(hook.contains("kintsu triage --status"));
        }
    }
}
