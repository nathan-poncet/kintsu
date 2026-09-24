//! The composition root: one function per subcommand that builds the
//! gateways, calls a use case and hands the result to a presenter. Nothing
//! here decides anything a test would want to check.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use crate::adapters::controllers::{Command, ScopeFlag, parse_args};
use crate::adapters::gateways::{
    DEFAULT_CONFIG, EnvSecrets, FsEnvironment, HttpModels, JsonState, RandomIds, ShellAgents,
    SystemClock, load_settings,
};
use crate::adapters::presenters::doctor::Places;
use crate::adapters::presenters::{
    Style, doctor_report, error_line, explanation, fix_report, hand_off_notice, ignored,
    privacy_report, raw_fix, shell_hook, toast,
};
use crate::entities::{SessionId, Settings, UiMode};
use crate::use_cases::{
    Diagnose, Explain, ExplainError, FixLast, HandOff, Ignore, IgnoreRequest, Privacy, ScopeChoice,
    Triage,
};

pub const USAGE: &str = "\
kintsu — when a command fails, fix it, understand it, or hand it to your agent

Usage:
  kintsu init <zsh|bash|fish>     the shell hook, to eval or source
  kintsu fix [--raw]              the corrected command for the last failure
  kintsu why                      what happened, from your model
  kintsu agent [--with <name>] [words…]
                                  hand the last failure to your CLI agent
  kintsu privacy                  what a model or an agent would receive
  kintsu ignore [--command|--dir|--session|--always] [program]
  kintsu mute [30m|1h|…]          nothing for a while (default 1h)
  kintsu doctor                   check the hook, the models, the keys
  kintsu default-config           the commented default configuration
  kintsu config path              where the files are
  kintsu --version | --help

Environment: KINTSU_CONFIG, KINTSU_STATE_DIR, KINTSU_DISABLE=1, NO_COLOR.
";

/// What the process knows about its surroundings; read once in `main`.
pub struct Runtime {
    pub args: Vec<String>,
    pub session: Option<SessionId>,
    pub cwd: Option<String>,
    pub home: Option<String>,
    pub config_path: PathBuf,
    pub state_dir: PathBuf,
    pub path_var: String,
    pub color: bool,
    pub debug: bool,
}

pub fn run(rt: &Runtime, out: &mut dyn Write, err: &mut dyn Write) -> ExitCode {
    let command = match parse_args(rt.args.iter().map(String::as_str)) {
        Ok(command) => command,
        Err(e) => {
            let _ = writeln!(err, "kintsu: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match command {
        Command::Help => {
            let _ = write!(out, "{USAGE}");
            return ExitCode::SUCCESS;
        }
        Command::Version => {
            let _ = writeln!(out, "kintsu {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Command::Init(shell) => {
            let _ = write!(out, "{}", shell_hook(shell));
            return ExitCode::SUCCESS;
        }
        Command::DefaultConfig => {
            let _ = write!(out, "{DEFAULT_CONFIG}");
            return ExitCode::SUCCESS;
        }
        Command::ConfigPath => {
            let exists = if rt.config_path.is_file() {
                ""
            } else {
                "  (missing: kintsu default-config > that path)"
            };
            let _ = writeln!(
                out,
                "config  {}{exists}\nstate   {}",
                rt.config_path.display(),
                rt.state_dir.display()
            );
            return ExitCode::SUCCESS;
        }
        _ => {}
    }

    let plain = Style {
        color: rt.color,
        ascii: false,
        mode: UiMode::Toast,
    };
    let settings = match load_settings(&rt.config_path, rt.home.as_deref()) {
        Ok(settings) => settings,
        Err(e) => {
            let _ = writeln!(err, "{}", error_line(&e.to_string(), &plain));
            if matches!(command, Command::Triage(_)) {
                Settings::default()
            } else {
                return ExitCode::from(2);
            }
        }
    };
    let style = Style {
        color: rt.color,
        ascii: settings.ui.ascii,
        mode: settings.ui.mode,
    };
    let state = JsonState::new(&rt.state_dir);
    let environment = FsEnvironment::new(rt.path_var.clone());
    let session = rt.session.as_ref();

    match command {
        Command::Triage(input) => {
            let triage = Triage {
                settings: &settings,
                clock: &SystemClock,
                ids: &RandomIds,
                sessions: &state,
                cases: &state,
                ignores: &state,
                environment: &environment,
            };
            match triage.run(input) {
                Ok(decision) => {
                    if let Some(text) = toast(&decision, &style) {
                        let _ = writeln!(err, "{text}");
                    }
                }
                Err(e) if rt.debug => {
                    let _ = writeln!(err, "{}", error_line(&e.to_string(), &style));
                }
                Err(_) => {}
            }
            ExitCode::SUCCESS
        }
        Command::Fix { raw } => {
            let fix_last = FixLast {
                settings: &settings,
                cases: &state,
                environment: &environment,
                secrets: &EnvSecrets,
                models: &HttpModels,
            };
            match fix_last.run(session) {
                Ok(proposal) if raw => match raw_fix(&proposal) {
                    Some(command) => {
                        let _ = writeln!(out, "{command}");
                        ExitCode::SUCCESS
                    }
                    None => ExitCode::from(1),
                },
                Ok(proposal) => {
                    let _ = writeln!(out, "{}", fix_report(&proposal, &style));
                    ExitCode::SUCCESS
                }
                Err(e) => failure(err, &e.to_string(), &style, raw),
            }
        }
        Command::Why => {
            let explain = Explain {
                settings: &settings,
                cases: &state,
                secrets: &EnvSecrets,
                models: &HttpModels,
            };
            match explain.run(session) {
                Ok(e) => {
                    let _ = writeln!(out, "{}", explanation(&e, &style));
                    ExitCode::SUCCESS
                }
                Err(ExplainError::AllFailed(failures)) => {
                    let detail: Vec<String> =
                        failures.iter().map(|(n, e)| format!("{n}: {e}")).collect();
                    failure(
                        err,
                        &format!("no model answered ({})", detail.join("; ")),
                        &style,
                        false,
                    )
                }
                Err(e) => failure(err, &e.to_string(), &style, false),
            }
        }
        Command::Agent { with, words } => {
            let launcher = ShellAgents::new(rt.state_dir.join("briefs"));
            let hand_off = HandOff {
                settings: &settings,
                cases: &state,
                environment: &environment,
                launcher: &launcher,
            };
            let plan = match hand_off.prepare(session, with.as_deref(), words.as_deref()) {
                Ok(plan) => plan,
                Err(e) => return failure(err, &e.to_string(), &style, false),
            };
            let _ = writeln!(err, "{}", hand_off_notice(&plan, &style));
            match hand_off.launch(&plan) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => failure(err, &e.to_string(), &style, false),
            }
        }
        Command::Privacy => match (Privacy { cases: &state }).run(session) {
            Ok(doc) => {
                let _ = writeln!(out, "{}", privacy_report(&doc, &style));
                ExitCode::SUCCESS
            }
            Err(e) => failure(err, &e.to_string(), &style, false),
        },
        Command::Ignore { program, scope } => {
            let scope = match scope {
                ScopeFlag::Command => ScopeChoice::Command,
                ScopeFlag::Always => ScopeChoice::Always,
                ScopeFlag::Session => ScopeChoice::Session,
                ScopeFlag::Dir => match &rt.cwd {
                    Some(cwd) => ScopeChoice::Directory(cwd.clone()),
                    None => {
                        return failure(err, "cannot tell the current directory", &style, false);
                    }
                },
            };
            silence(
                &state,
                session,
                IgnoreRequest::Last { program, scope },
                &style,
                out,
                err,
            )
        }
        Command::Mute(duration) => silence(
            &state,
            session,
            IgnoreRequest::Mute(duration),
            &style,
            out,
            err,
        ),
        Command::Doctor => {
            let checks = Diagnose {
                settings: &settings,
                secrets: &EnvSecrets,
                environment: &environment,
            }
            .run(session);
            let places = Places {
                config: &rt.config_path.display().to_string(),
                config_exists: rt.config_path.is_file(),
                state: &rt.state_dir.display().to_string(),
            };
            let _ = writeln!(out, "{}", doctor_report(&checks, &places, &style));
            ExitCode::SUCCESS
        }
        Command::Help
        | Command::Version
        | Command::Init(_)
        | Command::DefaultConfig
        | Command::ConfigPath => ExitCode::SUCCESS,
    }
}

fn silence(
    state: &JsonState,
    session: Option<&SessionId>,
    request: IgnoreRequest,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    let ignore = Ignore {
        clock: &SystemClock,
        cases: state,
        ignores: state,
    };
    match ignore.run(session, request) {
        Ok(entry) => {
            let _ = writeln!(out, "{}", ignored(&entry, style));
            ExitCode::SUCCESS
        }
        Err(e) => failure(err, &e.to_string(), style, false),
    }
}

fn failure(err: &mut dyn Write, message: &str, style: &Style, quiet: bool) -> ExitCode {
    if !quiet {
        let _ = writeln!(err, "{}", error_line(message, style));
    }
    ExitCode::from(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Bench {
        dir: PathBuf,
    }

    impl Bench {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("kintsu-app-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("bin")).unwrap();
            std::fs::write(dir.join("bin").join("git"), "").unwrap();
            Self { dir }
        }

        fn run(&self, args: &[&str], session: Option<&str>) -> (ExitCode, String, String) {
            let rt = Runtime {
                args: args.iter().map(|s| s.to_string()).collect(),
                session: session.map(SessionId::new),
                cwd: Some(self.dir.display().to_string()),
                home: None,
                config_path: self.dir.join("config.toml"),
                state_dir: self.dir.join("state"),
                path_var: self.dir.join("bin").display().to_string(),
                color: false,
                debug: true,
            };
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = run(&rt, &mut out, &mut err);
            (
                code,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            )
        }
    }

    impl Drop for Bench {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn a_typo_is_toasted_then_fixed_raw_for_the_shell_binding() {
        let b = Bench::new("typo");
        let (_, out, err) = b.run(
            &[
                "triage",
                "--status",
                "127",
                "--command",
                "gti status",
                "--session",
                "7",
            ],
            None,
        );
        assert!(out.is_empty());
        assert_eq!(
            err,
            "▎ Did you mean git status?\n▎ ^K to insert · kintsu why · kintsu agent · kintsu ignore\n"
        );
        let (code, out, _) = b.run(&["fix", "--raw"], Some("7"));
        assert_eq!((code, out.as_str()), (ExitCode::SUCCESS, "git status\n"));
        let (code, out, err) = b.run(&["fix"], Some("7"));
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.starts_with("▎ git status\n"), "{out}{err}");
    }

    #[test]
    fn success_is_silent_and_a_failure_without_fix_offers_the_commands() {
        let b = Bench::new("plain");
        let (code, out, err) = b.run(
            &[
                "triage",
                "--status",
                "0",
                "--command",
                "ls",
                "--session",
                "7",
            ],
            None,
        );
        assert_eq!(
            (code, out.as_str(), err.as_str()),
            (ExitCode::SUCCESS, "", "")
        );
        let (_, _, err) = b.run(
            &[
                "triage",
                "--status",
                "2",
                "--command",
                "make test",
                "--session",
                "7",
                "--duration-ms",
                "12000",
            ],
            None,
        );
        assert_eq!(
            err,
            "▎ make test exited 2 after 12 s.\n▎ kintsu fix · kintsu why · kintsu agent · kintsu ignore\n"
        );
        let (code, _, err) = b.run(&["fix", "--raw"], Some("7"));
        assert_eq!((code, err.as_str()), (ExitCode::from(1), ""));
        let (code, _, err) = b.run(&["why"], Some("7"));
        assert_eq!(code, ExitCode::from(1));
        assert_eq!(err, "▎ kintsu: no model is configured for explanations\n");
    }

    #[test]
    fn ignoring_the_command_keeps_the_next_identical_failure_quiet() {
        let b = Bench::new("ignore");
        b.run(
            &[
                "triage",
                "--status",
                "2",
                "--command",
                "make test",
                "--session",
                "7",
            ],
            None,
        );
        let (code, out, _) = b.run(&["ignore"], Some("7"));
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(
            out.starts_with("▎ make test stays quiet everywhere."),
            "{out}"
        );
        b.run(
            &[
                "triage",
                "--status",
                "0",
                "--command",
                "ls",
                "--session",
                "7",
            ],
            None,
        );
        let (_, _, err) = b.run(
            &[
                "triage",
                "--status",
                "2",
                "--command",
                "make  test",
                "--session",
                "7",
            ],
            None,
        );
        assert_eq!(err, "");
        let (_, _, err) = b.run(
            &[
                "triage",
                "--status",
                "2",
                "--command",
                "make",
                "--session",
                "7",
            ],
            None,
        );
        assert!(err.starts_with("▎ make exited 2."));
        let (code, out, _) = b.run(&["mute", "1h"], Some("7"));
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.starts_with("▎ Everything stays quiet until the mute ends."));
        let (_, _, err) = b.run(
            &[
                "triage",
                "--status",
                "1",
                "--command",
                "cargo test",
                "--session",
                "7",
            ],
            None,
        );
        assert_eq!(err, "");
    }

    #[test]
    fn privacy_doctor_and_config_commands_answer() {
        let b = Bench::new("misc");
        b.run(
            &[
                "triage",
                "--status",
                "22",
                "--command",
                "curl -H 'Authorization: Bearer sk-live-abcdefghijklmnop' https://x",
                "--session",
                "7",
            ],
            None,
        );
        let (code, out, _) = b.run(&["privacy"], Some("7"));
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(
            out.contains("(1 secret redacted)")
                && out.contains("Bearer ••••••••")
                && !out.contains("sk-live"),
            "{out}"
        );
        let (code, out, _) = b.run(&["doctor"], None);
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(
            out.contains("✗ shell hook") && out.contains("! models"),
            "{out}"
        );
        let (_, out, _) = b.run(&["config", "path"], None);
        assert!(out.contains("config.toml  (missing"));
        let (_, out, _) = b.run(&["default-config"], None);
        assert_eq!(out, DEFAULT_CONFIG);
        let (code, _, err) = b.run(&["agent"], Some("7"));
        assert_eq!(code, ExitCode::from(1));
        assert!(err.contains("no agent is configured"));
    }

    #[test]
    fn a_broken_config_is_reported_and_triage_still_works() {
        let b = Bench::new("broken");
        std::fs::write(
            b.dir.join("config.toml"),
            "[models.x]\nprovider = \"nope\"\nmodel = \"m\"",
        )
        .unwrap();
        let (code, _, err) = b.run(
            &[
                "triage",
                "--status",
                "2",
                "--command",
                "make",
                "--session",
                "7",
            ],
            None,
        );
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(
            err.starts_with("▎ kintsu: config: models.x: unknown provider `nope`"),
            "{err}"
        );
        assert!(err.contains("▎ make exited 2."));
        let (code, _, _) = b.run(&["why"], Some("7"));
        assert_eq!(code, ExitCode::from(2));
        let (code, _, err) = b.run(&["frobnicate"], None);
        assert_eq!(code, ExitCode::from(2));
        assert!(err.starts_with("kintsu: unknown command `frobnicate`"));
    }
}
