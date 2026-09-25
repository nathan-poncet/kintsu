//! The composition root: one function per subcommand that builds the
//! gateways, calls a use case and hands the result to a presenter. Nothing
//! here decides anything a test would want to check.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use crate::adapters::controllers::{
    Command, DaemonAction, Prompter, ScopeFlag, ServiceAction, parse_act_url, parse_args,
};
use crate::adapters::gateways::service;
use crate::adapters::gateways::{
    DEFAULT_CONFIG, DaemonClient, EnvSecrets, FsEnvironment, HookNotes, HttpModels, JsonState,
    RandomIds, ShellAgents, SystemClock, TerminalOutput, load_settings, render_settings,
};
use crate::adapters::presenters::doctor::Places;
use crate::adapters::presenters::{
    Style, doctor_report, error_line, explanation, fix_report, hand_off_notice, ignored,
    pending_line, privacy_report, raw_fix, shell_hook, toast,
};
use crate::daemon::{self, DaemonConfig};
use crate::entities::{SessionId, Settings, Shell, TerminalIdentity, TriageDecision, UiMode};
use crate::use_cases::{
    CaptureOutput, Detect, Diagnose, Explain, FixLast, HandOff, Ignore, IgnoreRequest, Privacy,
    ScopeChoice, Triage, TriageInput, compose,
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
const SYNC_BUDGET: Duration = Duration::from_millis(40);

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
        Command::Setup { yes } => return setup(rt, yes, out, err),
        Command::Open { url } => return open(rt, &url, err, &plain),
        Command::Service(action) => return service_command(rt, action, out, err, &plain),
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
        Command::Subscribe { session } => return subscribe(rt, session, out),
        Command::Pending { session } => {
            let Some(session) = session.or_else(|| rt.session.clone()) else {
                return ExitCode::SUCCESS;
            };
            for text in rt.client().pending(&session, rt.color).unwrap_or_default() {
                let _ = writeln!(err, "{text}");
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
        Command::Triage { input, signal_pid } => triage(
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
            if rt.daemon {
                if let Some(session) = session {
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
            }
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
struct Local<'a> {
    settings: &'a Settings,
    state: &'a JsonState,
    environment: &'a FsEnvironment,
    style: &'a Style,
}

/// The hook's call: the daemon within the budget when it may, the local
/// path otherwise. The prompt never waits longer than the budget.
fn triage(
    rt: &Runtime,
    local: &Local<'_>,
    mut input: TriageInput,
    signal_pid: Option<u32>,
    err: &mut dyn Write,
) -> ExitCode {
    let Local {
        settings,
        state,
        environment,
        style,
    } = *local;
    input.terminal = rt.terminal.clone();
    if rt.daemon {
        if let Some(view) = rt
            .client()
            .command_finished(&input, rt.color, signal_pid, SYNC_BUDGET)
        {
            for text in view.toast.iter().chain(view.bubbles.iter()) {
                let _ = writeln!(err, "{text}");
            }
            if let Some(session) = &input.session {
                let notes = HookNotes::new(&rt.state_dir);
                if view.pending.is_some() {
                    let _ = notes.set_asking(session);
                }
                if let Some(ghost) = &view.ghost {
                    let _ = notes.set_ghost(session, ghost);
                }
            }
            return ExitCode::SUCCESS;
        }
    }
    let triage = Triage {
        settings,
        clock: &SystemClock,
        ids: &RandomIds,
        sessions: state,
        cases: state,
        ignores: state,
        environment,
    };
    let ghost_shell = input.shell.is_some_and(Shell::supports_ghost_text);
    let session = input.session.clone();
    match triage.run(input) {
        Ok(decision) => {
            if let Some(text) = toast(&decision, style, ghost_shell) {
                let _ = writeln!(err, "{text}");
            }
            if let (TriageDecision::Offer { fix: Some(fix), .. }, Some(session), true) =
                (&decision, &session, ghost_shell)
            {
                if fix.is_ghostable() {
                    let _ =
                        HookNotes::new(&rt.state_dir).set_ghost(session, fix.command().as_str());
                }
            }
            // The output is read after the bubble: it costs a program run,
            // and only an offered case is worth it.
            if let TriageDecision::Offer { case, .. } = decision {
                let capture = CaptureOutput {
                    settings,
                    output: &TerminalOutput::new(settings.capture.sources.clone()),
                    cases: state,
                };
                let _ = capture.run(*case, &rt.terminal);
            }
        }
        Err(e) if rt.debug => {
            let _ = writeln!(err, "{}", error_line(&e.to_string(), style));
        }
        Err(_) => {}
    }
    ExitCode::SUCCESS
}

/// `kintsu subscribe`: prints every bubble the daemon sends for the session
/// until the daemon or the parent shell goes away.
fn subscribe(rt: &Runtime, session: Option<SessionId>, out: &mut dyn Write) -> ExitCode {
    let Some(session) = session.or_else(|| rt.session.clone()) else {
        return ExitCode::from(2);
    };
    if !rt.daemon {
        return ExitCode::from(1);
    }
    let Ok(stream) = rt.client().subscribe(&session, rt.color) else {
        return ExitCode::from(1);
    };
    let _ = DaemonClient::follow(stream, |text| {
        let _ = writeln!(out, "{text}");
        let _ = out.flush();
    });
    ExitCode::SUCCESS
}

/// `kintsu setup`: what the machine has, three questions, one file, and
/// `kintsu open kintsu://act?case=…&do=…`: a click, handed to the daemon,
/// which answers in the shell the case came from.
fn open(rt: &Runtime, url: &str, err: &mut dyn Write, plain: &Style) -> ExitCode {
    let (case, action) = match parse_act_url(url) {
        Ok(parsed) => parsed,
        Err(e) => return failure(err, &e.to_string(), plain, false),
    };
    match rt.client().act(&case, action) {
        Some(Ok(())) => ExitCode::SUCCESS,
        Some(Err(reason)) => failure(err, &reason, plain, false),
        None => failure(
            err,
            "no daemon is running; open a hooked shell first",
            plain,
            false,
        ),
    }
}

/// `kintsu service install|uninstall`: launchd or systemd keeps the daemon
/// up, and the desktop hands `kintsu://` links to `kintsu open`.
fn service_command(
    rt: &Runtime,
    action: ServiceAction,
    out: &mut dyn Write,
    err: &mut dyn Write,
    plain: &Style,
) -> ExitCode {
    let Some(home) = rt.home.clone() else {
        return failure(err, "HOME is not set", plain, false);
    };
    let paths = service::ServicePaths {
        home: PathBuf::from(home),
        state_dir: rt.state_dir.clone(),
        exe: rt.exe.clone(),
    };
    let macos = cfg!(target_os = "macos");
    let mut run = |program: &str, args: &[String]| service::run_command(program, args);
    let result = match action {
        ServiceAction::Install => service::install(&paths, macos, &mut run),
        ServiceAction::Uninstall => service::uninstall(&paths, macos, &mut run),
    };
    match result {
        Ok(lines) => {
            for line in lines {
                let _ = writeln!(out, "{}", plain.line(&line));
            }
            ExitCode::SUCCESS
        }
        Err(e) => failure(err, &e.to_string(), plain, false),
    }
}

/// doctor's verdict on it.
fn setup(rt: &Runtime, yes: bool, out: &mut dyn Write, err: &mut dyn Write) -> ExitCode {
    let plain = Style {
        color: rt.color,
        ascii: false,
        mode: UiMode::Toast,
        links: false,
    };
    let environment = FsEnvironment::new(rt.path_var.clone());
    let detected = Detect {
        environment: &environment,
        models: &HttpModels,
    }
    .run();
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut prompter = Prompter {
        input: &mut input,
        output: out,
        assume_yes: yes,
    };
    if rt.config_path.is_file() && !yes {
        let _ = writeln!(
            prompter.output,
            "{}",
            plain.line(&format!("{} exists.", rt.config_path.display()))
        );
        let mut line = String::new();
        let _ = write!(prompter.output, "▎ Replace it? [y/N] ");
        let _ = prompter.output.flush();
        let _ = prompter.input.read_line(&mut line);
        if !matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            let _ = writeln!(prompter.output, "{}", plain.line("kept as is."));
            return ExitCode::SUCCESS;
        }
    }
    let answers = prompter.run(&detected);
    let settings = compose(&answers);
    if let Some(parent) = rt.config_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&rt.config_path, render_settings(&settings)) {
        return failure(
            err,
            &format!("cannot write {}: {e}", rt.config_path.display()),
            &plain,
            false,
        );
    }
    let _ = writeln!(
        out,
        "{}",
        plain.line(&format!("written to {}", rt.config_path.display()))
    );
    let checks = Diagnose {
        settings: &settings,
        secrets: &EnvSecrets,
        environment: &environment,
        models: &HttpModels,
    }
    .run(rt.session.as_ref());
    let places = Places {
        config: &rt.config_path.display().to_string(),
        config_exists: true,
        state: &rt.state_dir.display().to_string(),
    };
    let _ = writeln!(
        out,
        "
{}",
        doctor_report(&checks, &places, &plain)
    );
    ExitCode::SUCCESS
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
                socket_path: self.dir.join("d.sock"),
                log_path: self.dir.join("d.log"),
                exe: PathBuf::from("/definitely/not/kintsu"),
                path_var: self.dir.join("bin").display().to_string(),
                color: false,
                debug: true,
                daemon: false,
                terminal: TerminalIdentity::default(),
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
                "--shell",
                "zsh",
            ],
            None,
        );
        assert!(out.is_empty());
        assert_eq!(
            err,
            "▎ Did you mean git status?\n▎ Tab to fix · kintsu why · kintsu agent · kintsu ignore\n"
        );
        assert_eq!(
            std::fs::read_to_string(b.dir.join("state").join("sessions").join("7.ghost")).unwrap(),
            "git status"
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
        assert!(out.contains("config.toml  (missing") && out.contains("socket  "));
        let (_, out, _) = b.run(&["default-config"], None);
        assert_eq!(out, DEFAULT_CONFIG);
        let (code, _, err) = b.run(&["agent"], Some("7"));
        assert_eq!(code, ExitCode::from(1));
        assert!(err.contains("no agent is configured"));
    }

    #[test]
    fn setup_with_yes_writes_a_file_from_what_the_path_offers() {
        let b = Bench::new("setup");
        for tool in ["ollama", "codex"] {
            std::fs::write(b.dir.join("bin").join(tool), "").unwrap();
        }
        let (code, out, err) = b.run(&["setup", "--yes"], Some("7"));
        assert_eq!(code, ExitCode::SUCCESS, "{err}");
        let written = std::fs::read_to_string(b.dir.join("config.toml")).unwrap();
        assert!(
            written.contains("[models.local]") && written.contains("[models.codex-cli]"),
            "{written}"
        );
        assert!(
            written.contains("investigate = [\"codex-cli\"]"),
            "{written}"
        );
        assert!(
            out.contains("written to") && out.contains("model codex-cli"),
            "{out}"
        );
        let (code, out, _) = b.run(&["doctor"], Some("7"));
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(out.contains("model local"), "{out}");
    }

    #[test]
    fn without_a_daemon_status_stop_and_pending_say_so_and_subscribe_leaves() {
        let b = Bench::new("nodaemon");
        let (code, out, _) = b.run(&["daemon", "status"], None);
        assert_eq!(code, ExitCode::from(1));
        assert!(out.contains("no daemon running"));
        let (code, out, _) = b.run(&["daemon", "stop"], None);
        assert_eq!(
            (code, out.as_str()),
            (ExitCode::SUCCESS, "▎ no daemon was running\n")
        );
        let (code, out, err) = b.run(&["pending"], Some("7"));
        assert_eq!(
            (code, out.as_str(), err.as_str()),
            (ExitCode::SUCCESS, "", "")
        );
        let (code, _, _) = b.run(&["subscribe", "--session", "7"], None);
        assert_eq!(code, ExitCode::from(1));
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
