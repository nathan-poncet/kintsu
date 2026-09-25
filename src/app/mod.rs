//! The composition root: one function per subcommand that builds the
//! gateways, calls a use case and hands the result to a presenter. Nothing
//! here decides anything a test would want to check.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use crate::adapters::controllers::{Command, DaemonAction, ScopeFlag, parse_args};
use crate::adapters::gateways::{
    DEFAULT_CONFIG, DaemonClient, EnvSecrets, FsEnvironment, HookNotes, HttpModels, JsonState,
    ShellAgents, SystemClock, load_settings,
};
use crate::adapters::presenters::doctor::Places;
use crate::adapters::presenters::{
    Style, doctor_report, error_line, explanation, fix_report, hand_off_notice, ignored,
    pending_line, privacy_report, raw_fix, shell_hook,
};
use crate::daemon::{self, DaemonConfig};
use crate::entities::{SessionId, Settings, Shell, TerminalIdentity, UiMode};
use crate::use_cases::ports::SessionRegistry;
use crate::use_cases::{
    Diagnose, Explain, FixLast, HandOff, Ignore, IgnoreRequest, Privacy, ScopeChoice,
};

mod desktop;
mod hooks;
mod panel;
mod setup;
#[cfg(test)]
mod tests;

pub const USAGE: &str = "\
kintsu — when a command fails, fix it, understand it, or hand it to your agent

Usage:
  kintsu init <zsh|bash|fish>     the shell hook, to eval or source
  kintsu panel                    the last bubble, expanded under the prompt (what ^K runs)
  kintsu fix [--raw]              the corrected command for the last failure
  kintsu why                      what happened, from your model
  kintsu agent [--with <name>] [words…]
                                  hand the last failure to your CLI agent
  kintsu privacy                  what a model or an agent would receive
  kintsu ignore [--command|--dir|--session|--always] [program]
  kintsu mute [30m|1h|…]          nothing for a while (default 1h)
  kintsu setup [--yes]            three questions, then the configuration file
  kintsu doctor                   check the hook, the models, the keys
  kintsu default-config           the commented default configuration
  kintsu config path              where the files are
  kintsu daemon [run|stop|status] the resident process the hooks talk to
  kintsu service install|uninstall
                                  the daemon as a service; kintsu:// links handled by the desktop
  kintsu open <kintsu://…>        what the desktop runs when you click a word
  kintsu --version | --help

Environment: KINTSU_CONFIG, KINTSU_STATE_DIR, KINTSU_SOCKET, KINTSU_NO_DAEMON,
KINTSU_DISABLE=1, NO_COLOR.
";

/// The sync budget: how long a hook waits for the daemon's decision.
pub(super) const SYNC_BUDGET: Duration = Duration::from_millis(40);

/// What the process knows about its surroundings; read once in `main`.
pub struct Runtime {
    pub args: Vec<String>,
    pub session: Option<SessionId>,
    pub cwd: Option<String>,
    pub home: Option<String>,
    pub config_path: PathBuf,
    pub state_dir: PathBuf,
    pub socket_path: PathBuf,
    pub log_path: PathBuf,
    pub exe: PathBuf,
    pub path_var: String,
    pub color: bool,
    /// Colour for what is drawn on the tty itself, whatever stdout is.
    pub tty_color: bool,
    /// Colour for text a hook captures and prints on the terminal for us:
    /// both our streams are its pipes, so only the terminal's existence
    /// and `NO_COLOR` count.
    pub terminal_color: bool,
    pub debug: bool,
    /// Whether hooks may talk to, and start, the daemon.
    pub daemon: bool,
    /// The pane this process runs in, for the output capture.
    pub terminal: TerminalIdentity,
}

impl Runtime {
    fn client(&self) -> DaemonClient {
        DaemonClient::new(&self.socket_path, &self.exe, &self.log_path)
    }
}

pub fn run(rt: &Runtime, out: &mut dyn Write, err: &mut dyn Write) -> ExitCode {
    let command = match parse_args(rt.args.iter().map(String::as_str)) {
        Ok(command) => command,
        Err(e) => {
            let _ = writeln!(err, "kintsu: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let plain = Style {
        color: rt.color,
        ascii: false,
        mode: UiMode::Toast,
        links: false,
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
            let _ = write!(
                out,
                "{}",
                shell_hook(shell, &rt.state_dir.display().to_string())
            );
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
                "config  {}{exists}\nstate   {}\nsocket  {}",
                rt.config_path.display(),
                rt.state_dir.display(),
                rt.socket_path.display()
            );
            return ExitCode::SUCCESS;
        }
        Command::Setup { yes } => return setup::run(rt, yes, out, err),
        Command::Open { url } => return desktop::open(rt, &url, err, &plain),
        Command::Service(action) => return desktop::service(rt, action, out, err, &plain),
        Command::Daemon(DaemonAction::Run) => {
            return daemon::run(DaemonConfig {
                socket: rt.socket_path.clone(),
                state_dir: rt.state_dir.clone(),
                config_path: rt.config_path.clone(),
                home: rt.home.clone(),
                path_var: rt.path_var.clone(),
            });
        }
        Command::Daemon(DaemonAction::Stop) => {
            return match rt.client().shutdown() {
                Ok(true) => {
                    let _ = writeln!(out, "{}", plain.line("daemon stopped"));
                    ExitCode::SUCCESS
                }
                Ok(false) => {
                    let _ = writeln!(out, "{}", plain.line("no daemon was running"));
                    ExitCode::SUCCESS
                }
                Err(e) => failure(err, &e.to_string(), &plain, false),
            };
        }
        Command::Daemon(DaemonAction::Status) => {
            return match rt.client().hello() {
                Ok(version) => {
                    let _ = writeln!(
                        out,
                        "{}",
                        plain.line(&format!(
                            "daemon {version} running on {}",
                            rt.socket_path.display()
                        ))
                    );
                    ExitCode::SUCCESS
                }
                Err(_) => {
                    let _ = writeln!(
                        out,
                        "{}",
                        plain.line(
                            "no daemon running; the next failure in a hooked shell starts it"
                        )
                    );
                    ExitCode::from(1)
                }
            };
        }
        Command::Subscribe { session } => return hooks::subscribe(rt, session, out),
        Command::Pending { session } => {
            let Some(session) = session.or_else(|| rt.session.clone()) else {
                return ExitCode::SUCCESS;
            };
            let notes = HookNotes::new(&rt.state_dir);
            for text in rt
                .client()
                .pending(&session, rt.terminal_color)
                .unwrap_or_default()
            {
                let _ = writeln!(err, "{text}");
                let _ = notes.append_bubble(&session, &text);
            }
            return ExitCode::SUCCESS;
        }
        _ => {}
    }

    let settings = match load_settings(&rt.config_path, rt.home.as_deref()) {
        Ok(settings) => settings,
        Err(e) => {
            let _ = writeln!(err, "{}", error_line(&e.to_string(), &plain));
            if matches!(command, Command::Triage { .. }) {
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
        links: settings.ui.links,
    };
    let state = JsonState::new(&rt.state_dir);
    let environment = FsEnvironment::new(rt.path_var.clone());
    let session = rt.session.as_ref();

    match command {
        Command::Triage { input, signal_pid } => hooks::triage(
            rt,
            &Local {
                settings: &settings,
                state: &state,
                environment: &environment,
                style: &style,
            },
            *input,
            signal_pid,
            err,
        ),
        Command::Panel { above } => panel::expand(
            rt,
            &Local {
                settings: &settings,
                state: &state,
                environment: &environment,
                style: &style,
            },
            session,
            above,
            out,
            err,
        ),
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
                    let report = fix_report(&proposal, &style);
                    let _ = writeln!(out, "{report}");
                    if let Some(session) = session {
                        let _ = HookNotes::new(&rt.state_dir).set_bubble(session, &report);
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => failure(err, &e.to_string(), &style, raw),
            }
        }
        Command::Why => {
            // bash hears a message only at its next prompt: answer in place
            // instead of telling it to wait.
            let hears_messages = session
                .and_then(|s| state.load(s).ok().flatten())
                .and_then(|s| s.shell())
                .is_none_or(Shell::delivers_live);
            if rt.daemon
                && hears_messages
                && let Some(session) = session
            {
                match rt.client().explain(session) {
                    Some(Ok(model)) => {
                        let _ = writeln!(err, "{}", pending_line(&model, &style));
                        let _ = HookNotes::new(&rt.state_dir).set_asking(session);
                        return ExitCode::SUCCESS;
                    }
                    Some(Err(reason)) => return failure(err, &reason, &style, false),
                    None => {}
                }
            }
            let explain = Explain {
                settings: &settings,
                cases: &state,
                secrets: &EnvSecrets,
                models: &HttpModels,
            };
            match explain.run(session) {
                Ok(e) => {
                    let text = explanation(&e, &style);
                    let _ = writeln!(out, "{text}");
                    if let Some(session) = session {
                        let _ = HookNotes::new(&rt.state_dir).set_bubble(session, &text);
                    }
                    ExitCode::SUCCESS
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
                models: &HttpModels,
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
        | Command::ConfigPath
        | Command::Daemon(_)
        | Command::Setup { .. }
        | Command::Open { .. }
        | Command::Service(_)
        | Command::Subscribe { .. }
        | Command::Pending { .. } => ExitCode::SUCCESS,
    }
}

/// The gateways the local path needs, built once per run.
pub(super) struct Local<'a> {
    pub(super) settings: &'a Settings,
    pub(super) state: &'a JsonState,
    pub(super) environment: &'a FsEnvironment,
    pub(super) style: &'a Style,
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

pub(super) fn failure(err: &mut dyn Write, message: &str, style: &Style, quiet: bool) -> ExitCode {
    if !quiet {
        let _ = writeln!(err, "{}", error_line(message, style));
    }
    ExitCode::from(1)
}
