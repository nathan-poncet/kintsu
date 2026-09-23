//! kintsu: when a command fails, offer to hand it to an AI agent, in the
//! user's own terminal and shell. `main` is the composition root only: it
//! wires controllers, use cases and presenters; no rule lives here.

#![forbid(unsafe_code)]

mod adapters;
mod entities;
mod use_cases;

use std::process::ExitCode;

use adapters::controllers::{Command, parse_args};
use adapters::presenters::{hint, shell_hook};
use use_cases::triage_outcome;

const USAGE: &str = "\
kintsu — when a command fails, hand it to your agent

Usage:
  kintsu init <zsh|bash|fish>       print the shell hook to eval or source
  kintsu triage --status <code> --command <text>
                                    called by the hook after each command
  kintsu --version | --help
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(args.iter().map(String::as_str)) {
        Ok(Command::Init(shell)) => {
            print!("{}", shell_hook(shell));
            ExitCode::SUCCESS
        }
        Ok(Command::Triage(outcome)) => {
            if let Some(line) = hint(&triage_outcome(outcome)) {
                eprintln!("{line}");
            }
            ExitCode::SUCCESS
        }
        Ok(Command::Version) => {
            println!("kintsu {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Command::Help) => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("kintsu: {error}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
