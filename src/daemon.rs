//! `kintsu daemon run`: the resident process. One per user, started by the
//! first client that finds nobody on the socket, alive across every shell.
//! It answers `command_finished` within the sync budget, keeps the warm
//! configuration, and sends what models say later to the shells that
//! subscribed. Composition only: it wires gateways to use cases and
//! presenters, one function per frame, and decides nothing itself.

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::adapters::controllers::{Request, parse_frame};
use crate::adapters::gateways::ndjson::{read_line, send_line};
use crate::adapters::gateways::{
    FsEnvironment, HttpModels, JsonState, RandomIds, SessionSecrets, Sessions, SystemClock,
    TerminalOutput, load_settings, unix,
};
use crate::adapters::presenters::ignored;
use crate::adapters::presenters::{Style, frames, message_toast, pending_line, toast};
use crate::entities::{Action, CaseId, Hotkey, SessionId, Settings, Shell, TriageDecision, UiMode};
use crate::use_cases::ports::Secrets;
use crate::use_cases::{
    CaptureOutput, Focus, Ignore, IgnoreRequest, Messages, ScopeChoice, Triage, TriageInput,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Subscribers are pinged this often so dead ones are noticed.
const SUBSCRIBER_PING: Duration = Duration::from_secs(20);

pub struct DaemonConfig {
    pub socket: PathBuf,
    pub state_dir: PathBuf,
    pub config_path: PathBuf,
    pub home: Option<String>,
    pub path_var: String,
}

/// Runs until `shutdown`, an outdated client, or a fatal error.
pub fn run(cfg: DaemonConfig) -> ExitCode {
    unix::detach();
    let listener = match bind(&cfg.socket) {
        Ok(Some(listener)) => listener,
        Ok(None) => {
            log("another daemon is listening; leaving");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            log(&format!("cannot listen on {}: {e}", cfg.socket.display()));
            return ExitCode::from(1);
        }
    };
    log(&format!(
        "kintsu daemon {VERSION} listening on {}",
        cfg.socket.display()
    ));
    let ascii = Arc::new(AtomicBool::new(false));
    let links = Arc::new(AtomicBool::new(true));
    let hotkey = Arc::new(AtomicU8::new(Hotkey::DEFAULT.letter() as u8));
    let render_ascii = Arc::clone(&ascii);
    let render_links = Arc::clone(&links);
    let render_hotkey = Arc::clone(&hotkey);
    let sessions = Sessions::new(
        Box::new(move |message, color| {
            let style = Style {
                color,
                ascii: render_ascii.load(Ordering::Relaxed),
                mode: UiMode::Toast,
                links: render_links.load(Ordering::Relaxed),
                hotkey: hotkey_from(render_hotkey.load(Ordering::Relaxed)),
            };
            frames::bubble(message.case(), &message_toast(message, &style))
        }),
        frames::ping(),
    );
    let daemon = Arc::new(Daemon {
        cfg,
        sessions,
        ascii,
        links,
        hotkey,
        settings: Mutex::new(None),
    });
    daemon.settings();
    let pinger = Arc::clone(&daemon);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(SUBSCRIBER_PING);
            pinger.sessions.ping();
            let gone = pinger.sessions.forget_gone(unix::process_alive);
            if gone > 0 {
                log(&format!(
                    "{gone} shell(s) gone, {} still known",
                    pinger.sessions.count()
                ));
            }
        }
    });
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let daemon = Arc::clone(&daemon);
                std::thread::spawn(move || handle(&daemon, stream));
            }
            Err(e) => log(&format!("accept failed: {e}")),
        }
    }
    ExitCode::SUCCESS
}

struct Daemon {
    cfg: DaemonConfig,
    sessions: Sessions,
    ascii: Arc<AtomicBool>,
    links: Arc<AtomicBool>,
    /// The hotkey's letter, for the subscriber renderer that outlives a
    /// settings reload.
    hotkey: Arc<AtomicU8>,
    settings: Mutex<Option<(Settings, Option<SystemTime>)>>,
}

/// The letter kept in the atomic, back to a key; it came from a parsed
/// hotkey, so anything else is the default.
fn hotkey_from(letter: u8) -> Hotkey {
    Hotkey::parse(&format!("^{}", letter as char)).unwrap_or_default()
}

impl Daemon {
    /// The configuration, reread when the file changed.
    fn settings(&self) -> Settings {
        let mtime = std::fs::metadata(&self.cfg.config_path)
            .and_then(|m| m.modified())
            .ok();
        let mut cached = self.settings.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((settings, seen)) = cached.as_ref()
            && *seen == mtime
        {
            return settings.clone();
        }
        let settings = match load_settings(&self.cfg.config_path, self.cfg.home.as_deref()) {
            Ok(settings) => settings,
            Err(e) => {
                log(&format!("{e}; using the defaults"));
                Settings::default()
            }
        };
        self.ascii.store(settings.ui.ascii, Ordering::Relaxed);
        self.links.store(settings.ui.links, Ordering::Relaxed);
        self.hotkey
            .store(settings.ui.hotkey.letter() as u8, Ordering::Relaxed);
        self.sessions.set_silent(settings.ui.mode == UiMode::Silent);
        *cached = Some((settings.clone(), mtime));
        settings
    }

    /// The keys the session's shell forwarded, over the daemon's own
    /// environment: under launchd or systemd the latter has none.
    fn secrets_for(&self, session: Option<&SessionId>) -> SessionSecrets {
        SessionSecrets::new(session.map(|s| self.sessions.env_of(s)).unwrap_or_default())
    }

    fn state(&self) -> JsonState {
        JsonState::new(&self.cfg.state_dir)
    }

    fn style(&self, settings: &Settings, color: bool) -> Style {
        Style {
            color,
            ascii: settings.ui.ascii,
            mode: settings.ui.mode,
            links: settings.ui.links,
            hotkey: settings.ui.hotkey,
        }
    }

    fn quit(&self) -> ! {
        let _ = std::fs::remove_file(&self.cfg.socket);
        log("bye");
        std::process::exit(0)
    }
}

/// One connection, one frame, one answer; subscriptions stay open.
fn handle(daemon: &Arc<Daemon>, mut stream: UnixStream) {
    if unix::peer_uid(&stream) != Some(unix::uid()) {
        log("refused a connection from another user");
        return;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let Ok(line) = read_line(&mut stream) else {
        return;
    };
    let frame = match parse_frame(&line) {
        Ok(frame) => frame,
        Err(e) => {
            let _ = send_line(&mut stream, &frames::error(&e.to_string()));
            return;
        }
    };
    if frame
        .client_version
        .as_deref()
        .is_some_and(|v| v != VERSION)
    {
        let _ = send_line(&mut stream, &frames::outdated(VERSION));
        log(&format!(
            "a client runs {}; this daemon is {VERSION}: leaving so it can restart me",
            frame.client_version.unwrap_or_default()
        ));
        daemon.quit();
    }
    match frame.request {
        Request::Hello { .. } => {
            let _ = send_line(&mut stream, &frames::welcome(VERSION));
        }
        Request::SessionNew { session, details } => {
            daemon.sessions.register(&session, *details);
            log(&daemon.sessions.describe(&session));
            let _ = send_line(&mut stream, &frames::session(&session));
        }
        Request::CommandFinished {
            input,
            color,
            signal_pid,
        } => {
            on_command_finished(daemon, &mut stream, *input, color, signal_pid);
        }
        Request::Subscribe { session, color } => {
            if send_line(&mut stream, &frames::ack()).is_ok() {
                let _ = stream.set_read_timeout(None);
                daemon.sessions.attach(&session, stream, color);
            }
        }
        Request::Pending { session, color } => {
            let style = daemon.style(&daemon.settings(), color);
            for message in daemon.sessions.drain(&session) {
                let _ = send_line(
                    &mut stream,
                    &frames::bubble(message.case(), &message_toast(&message, &style)),
                );
            }
            let _ = send_line(&mut stream, &frames::done());
        }
        Request::Explain { session } => on_explain(daemon, &mut stream, session),
        Request::Act { case, action } => on_act(daemon, &mut stream, case, action),
        Request::Shutdown => {
            let _ = send_line(&mut stream, &frames::bye());
            daemon.quit();
        }
    }
}

/// The hook's frame: triage now, answer within the budget, then ask the
/// quick-fix model in the background when the policy says so.
fn on_command_finished(
    daemon: &Arc<Daemon>,
    stream: &mut UnixStream,
    input: TriageInput,
    color: bool,
    signal_pid: Option<u32>,
) {
    let settings = daemon.settings();
    let style = daemon.style(&settings, color);
    let session = input.session.clone();
    let ghost_shell = input.shell.is_some_and(Shell::supports_ghost_text);
    if let (Some(session), Some(pid)) = (&session, signal_pid) {
        daemon.sessions.register_signal(session, pid);
    }
    if let (Some(session), Some(reported)) = (&session, &input.path) {
        daemon.sessions.remember_path(session, reported.clone());
    }
    if let Some(session) = &session
        && !input.env.is_empty()
    {
        daemon.sessions.remember_env(session, input.env.clone());
    }
    // What the frame does not say, the shell's registration may have said.
    // The daemon's own PATH, the bare system one under launchd or systemd,
    // is the last resort.
    let path = input
        .path
        .clone()
        .or_else(|| session.as_ref().and_then(|s| daemon.sessions.path_of(s)))
        .unwrap_or_else(|| daemon.cfg.path_var.clone());
    let terminal = if input.terminal.is_known() {
        input.terminal.clone()
    } else {
        session
            .as_ref()
            .and_then(|s| daemon.sessions.terminal_of(s))
            .unwrap_or_else(|| input.terminal.clone())
    };
    let secrets = daemon.secrets_for(session.as_ref());
    let state = daemon.state();
    let environment = FsEnvironment::new(path.clone());
    let triage = Triage {
        settings: &settings,
        clock: &SystemClock,
        ids: &RandomIds,
        sessions: &state,
        cases: &state,
        ignores: &state,
        environment: &environment,
    };
    let decision = match triage.run(input) {
        Ok(decision) => decision,
        Err(e) => {
            let _ = send_line(stream, &frames::error(&e.to_string()));
            return;
        }
    };
    if let (TriageDecision::Offer { case, .. }, Some(session)) = (&decision, &session) {
        daemon.sessions.remember_case(case.id(), session);
    }
    let pending = match &decision {
        TriageDecision::Offer { case, fix: None } => {
            messages(daemon, &settings, &state, &secrets).fix_candidate(case)
        }
        _ => None,
    };
    let mut text = toast(&decision, &style, ghost_shell);
    if let (Some(t), Some(model)) = (text.as_mut(), pending.as_deref()) {
        t.push('\n');
        t.push_str(&pending_line(model, &style));
    }
    // bash hears what waited only now: a message about a failure the shell
    // no longer looks at names its command.
    let watched = Focus {
        sessions: &state,
        cases: &state,
    };
    let bubbles: Vec<String> = session
        .as_ref()
        .map(|s| {
            let waiting = daemon.sessions.drain(s);
            watched.mark_late(s, waiting.clone()).unwrap_or(waiting)
        })
        .unwrap_or_default()
        .iter()
        .map(|m| message_toast(m, &style))
        .collect();
    let _ = send_line(
        stream,
        &frames::decision(&decision, text.as_deref(), &bubbles, pending.as_deref()),
    );
    if let TriageDecision::Offer { case, fix } = decision {
        // Off the sync path: read the output, keep it with the case, try the
        // rules that read it, then ask the model when the policy allows.
        let daemon = Arc::clone(daemon);
        std::thread::spawn(move || {
            let state = daemon.state();
            let capture = CaptureOutput {
                settings: &settings,
                output: &TerminalOutput::new(settings.capture.sources.clone()),
                cases: &state,
            };
            let case = match capture.run(*case, &terminal) {
                Ok(case) => case,
                Err(e) => {
                    log(&format!("capture: {e}"));
                    return;
                }
            };
            if fix.is_some() {
                return;
            }
            let messages = messages(&daemon, &settings, &state, &secrets);
            match messages.fix_from_output(&case, &FsEnvironment::new(path)) {
                Some(Ok(_)) => return,
                Some(Err(e)) => log(&format!("fix for {}: {e}", case.outcome().command())),
                None => {}
            }
            if pending.is_some()
                && let Err(e) = messages.fix(&case)
            {
                log(&format!("fix for {}: {e}", case.outcome().command()));
            }
        });
    }
}

/// `kintsu why` through the daemon: say which model is asked, answer later.
fn on_explain(daemon: &Arc<Daemon>, stream: &mut UnixStream, session: SessionId) {
    let settings = daemon.settings();
    let state = daemon.state();
    let secrets = daemon.secrets_for(Some(&session));
    let candidate = messages(daemon, &settings, &state, &secrets).explain_candidate(&session);
    match candidate {
        Err(e) => {
            let _ = send_line(stream, &frames::error(&e.to_string()));
        }
        Ok(model) => {
            let _ = send_line(stream, &frames::asked(&model));
            let daemon = Arc::clone(daemon);
            std::thread::spawn(move || {
                let state = daemon.state();
                if let Err(e) = messages(&daemon, &settings, &state, &secrets).explain(&session) {
                    log(&format!("explain for {}: {e}", session.as_str()));
                }
            });
        }
    }
}

/// A click on a word, or `kintsu open`: never runs a command, never starts
/// an agent; it answers in the shell the case came from.
fn on_act(daemon: &Arc<Daemon>, stream: &mut UnixStream, case: CaseId, action: Action) {
    let Some(session) = daemon.sessions.session_of(&case) else {
        let _ = send_line(
            stream,
            &frames::error(
                "this case is gone; act on the last failure with kintsu why, fix, agent or ignore",
            ),
        );
        return;
    };
    let _ = send_line(stream, &frames::ack());
    let daemon = Arc::clone(daemon);
    std::thread::spawn(move || {
        let settings = daemon.settings();
        let state = daemon.state();
        let secrets = daemon.secrets_for(Some(&session));
        let messages = messages(&daemon, &settings, &state, &secrets);
        let result = match action {
            Action::Why => messages.explain(&session).map(|_| ()),
            Action::Fix => {
                let path = daemon
                    .sessions
                    .path_of(&session)
                    .unwrap_or_else(|| daemon.cfg.path_var.clone());
                let environment = FsEnvironment::new(path);
                messages.fix_now(&session, &environment).map(|_| ())
            }
            Action::Ignore => {
                let ignore = Ignore {
                    clock: &SystemClock,
                    cases: &state,
                    ignores: &state,
                };
                let request = IgnoreRequest::Last {
                    program: None,
                    scope: ScopeChoice::Command,
                };
                let bare = Style {
                    color: false,
                    ascii: true,
                    mode: UiMode::Toast,
                    links: false,
                    hotkey: settings.ui.hotkey,
                };
                let note = match ignore.run(Some(&session), request) {
                    Ok(entry) => ignored(&entry, &bare)
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .trim_start_matches("| ")
                        .to_string(),
                    Err(e) => e.to_string(),
                };
                messages.note(&session, &case, note).map_err(Into::into)
            }
            Action::Agent => messages
                .note(
                    &session,
                    &case,
                    "a click cannot start an agent; run kintsu agent in this shell".into(),
                )
                .map_err(Into::into),
            Action::Privacy => messages
                .note(
                    &session,
                    &case,
                    "kintsu privacy shows exactly what a model or an agent would receive".into(),
                )
                .map_err(Into::into),
        };
        if let Err(e) = result {
            log(&format!("act {action} on {}: {e}", case.as_str()));
        }
    });
}

fn messages<'a>(
    daemon: &'a Daemon,
    settings: &'a Settings,
    state: &'a JsonState,
    secrets: &'a dyn Secrets,
) -> Messages<'a> {
    Messages {
        settings,
        clock: &SystemClock,
        secrets,
        models: &HttpModels,
        notifier: &daemon.sessions,
        cases: state,
        sessions: state,
    }
}

/// Listens on the socket, unless another daemon already does.
fn bind(socket: &Path) -> io::Result<Option<UnixListener>> {
    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent)?;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }
    if socket.exists() {
        if UnixStream::connect(socket).is_ok() {
            return Ok(None);
        }
        std::fs::remove_file(socket)?;
    }
    let listener = UnixListener::bind(socket)?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))?;
    Ok(Some(listener))
}

fn log(message: &str) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    eprintln!("{secs} {message}");
}
