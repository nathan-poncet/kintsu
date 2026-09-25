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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::adapters::controllers::{Request, parse_frame};
use crate::adapters::gateways::ndjson::{read_line, send_line};
use crate::adapters::gateways::{
    EnvSecrets, FsEnvironment, HttpModels, JsonState, RandomIds, Sessions, SystemClock,
    TerminalOutput, load_settings, unix,
};
use crate::adapters::presenters::{Style, frames, message_toast, pending_line, toast};
use crate::entities::{SessionId, Settings, Shell, TriageDecision, UiMode};
use crate::use_cases::{CaptureOutput, Messages, Triage, TriageInput};

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
    let render_ascii = Arc::clone(&ascii);
    let sessions = Sessions::new(
        Box::new(move |message, color| {
            let style = Style {
                color,
                ascii: render_ascii.load(Ordering::Relaxed),
                mode: UiMode::Toast,
            };
            frames::bubble(message.case(), &message_toast(message, &style))
        }),
        frames::ping(),
    );
    let daemon = Arc::new(Daemon {
        cfg,
        sessions,
        ascii,
        settings: Mutex::new(None),
    });
    daemon.settings();
    let pinger = Arc::clone(&daemon);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(SUBSCRIBER_PING);
            pinger.sessions.ping();
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
    settings: Mutex<Option<(Settings, Option<SystemTime>)>>,
}

impl Daemon {
    /// The configuration, reread when the file changed.
    fn settings(&self) -> Settings {
        let mtime = std::fs::metadata(&self.cfg.config_path)
            .and_then(|m| m.modified())
            .ok();
        let mut cached = self.settings.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((settings, seen)) = cached.as_ref() {
            if *seen == mtime {
                return settings.clone();
            }
        }
        let settings = match load_settings(&self.cfg.config_path, self.cfg.home.as_deref()) {
            Ok(settings) => settings,
            Err(e) => {
                log(&format!("{e}; using the defaults"));
                Settings::default()
            }
        };
        self.ascii.store(settings.ui.ascii, Ordering::Relaxed);
        self.sessions.set_silent(settings.ui.mode == UiMode::Silent);
        *cached = Some((settings.clone(), mtime));
        settings
    }

    fn state(&self) -> JsonState {
        JsonState::new(&self.cfg.state_dir)
    }

    fn style(&self, settings: &Settings, color: bool) -> Style {
        Style {
            color,
            ascii: settings.ui.ascii,
            mode: settings.ui.mode,
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
    let terminal = input.terminal.clone();
    let ghost_shell = input.shell.is_some_and(Shell::supports_ghost_text);
    if let (Some(session), Some(pid)) = (&session, signal_pid) {
        daemon.sessions.register_signal(session, pid);
    }
    let state = daemon.state();
    let environment = FsEnvironment::new(daemon.cfg.path_var.clone());
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
    let pending = match &decision {
        TriageDecision::Offer { case, fix: None } => {
            messages(daemon, &settings, &state).fix_candidate(case)
        }
        _ => None,
    };
    let mut text = toast(&decision, &style, ghost_shell);
    if let (Some(t), Some(model)) = (text.as_mut(), pending.as_deref()) {
        t.push('\n');
        t.push_str(&pending_line(model, &style));
    }
    let bubbles: Vec<String> = session
        .as_ref()
        .map(|s| daemon.sessions.drain(s))
        .unwrap_or_default()
        .iter()
        .map(|m| message_toast(m, &style))
        .collect();
    let _ = send_line(
        stream,
        &frames::decision(&decision, text.as_deref(), &bubbles, pending.as_deref()),
    );
    if let TriageDecision::Offer { case, fix } = decision {
        // Off the sync path: read the output, keep it with the case, then
        // ask the model when no rule knew and the policy allows.
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
            if fix.is_none() && pending.is_some() {
                if let Err(e) = messages(&daemon, &settings, &state).fix(&case) {
                    log(&format!("fix for {}: {e}", case.outcome().command()));
                }
            }
        });
    }
}

/// `kintsu why` through the daemon: say which model is asked, answer later.
fn on_explain(daemon: &Arc<Daemon>, stream: &mut UnixStream, session: SessionId) {
    let settings = daemon.settings();
    let state = daemon.state();
    match messages(daemon, &settings, &state).explain_candidate(&session) {
        Err(e) => {
            let _ = send_line(stream, &frames::error(&e.to_string()));
        }
        Ok(model) => {
            let _ = send_line(stream, &frames::asked(&model));
            let daemon = Arc::clone(daemon);
            std::thread::spawn(move || {
                let state = daemon.state();
                if let Err(e) = messages(&daemon, &settings, &state).explain(&session) {
                    log(&format!("explain for {}: {e}", session.as_str()));
                }
            });
        }
    }
}

fn messages<'a>(daemon: &'a Daemon, settings: &'a Settings, state: &'a JsonState) -> Messages<'a> {
    Messages {
        settings,
        clock: &SystemClock,
        secrets: &EnvSecrets,
        models: &HttpModels,
        notifier: &daemon.sessions,
        cases: state,
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
