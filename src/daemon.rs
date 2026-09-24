//! `kintsu daemon run`: the resident process. One per user, started by the
//! first client that finds nobody on the socket, alive across every shell.
//! It answers `command_finished` within the sync budget, keeps the warm
//! configuration, and delivers the messages that arrive later into the
//! shells that subscribed. Composition only: it wires gateways to use
//! cases and presenters, and decides nothing itself.

use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::adapters::controllers::{Request, parse_frame};
use crate::adapters::gateways::{
    EnvSecrets, FsEnvironment, HttpModels, JsonState, RandomIds, SystemClock, load_settings,
};
use crate::adapters::presenters::{Style, frames, message_toast, pending_line, toast};
use crate::entities::{Message, SessionId, Settings, TriageDecision, UiMode};
use crate::use_cases::ports::{Notifier, NotifyError};
use crate::use_cases::{Explain, FollowUp, Triage};

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
    os::detach();
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
    let daemon = Arc::new(Daemon {
        sessions: Sessions::default(),
        settings: Mutex::new(None),
        cfg,
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
        self.sessions
            .ascii
            .store(settings.ui.ascii, Ordering::Relaxed);
        self.sessions
            .silent
            .store(settings.ui.mode == UiMode::Silent, Ordering::Relaxed);
        *cached = Some((settings.clone(), mtime));
        settings
    }

    fn quit(&self) -> ! {
        let _ = std::fs::remove_file(&self.cfg.socket);
        log("bye");
        std::process::exit(0)
    }
}

fn handle(daemon: &Arc<Daemon>, mut stream: UnixStream) {
    if os::peer_uid(&stream) != Some(os::uid()) {
        log("refused a connection from another user");
        return;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let Ok(mut reader) = stream.try_clone().map(BufReader::new) else {
        return;
    };
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let frame = match parse_frame(&line) {
        Ok(frame) => frame,
        Err(e) => {
            let _ = send(&mut stream, &frames::error(&e.to_string()));
            return;
        }
    };
    if frame
        .client_version
        .as_deref()
        .is_some_and(|v| v != VERSION)
    {
        let _ = send(&mut stream, &frames::outdated(VERSION));
        log(&format!(
            "a client runs {}; this daemon is {VERSION}: leaving so it can restart me",
            frame.client_version.unwrap_or_default()
        ));
        daemon.quit();
    }
    match frame.request {
        Request::Hello { .. } => {
            let _ = send(&mut stream, &frames::welcome(VERSION));
        }
        Request::CommandFinished {
            input,
            color,
            signal_pid,
        } => {
            let settings = daemon.settings();
            let style = Style {
                color,
                ascii: settings.ui.ascii,
                mode: settings.ui.mode,
            };
            let session = input.session.clone();
            if let (Some(session), Some(pid)) = (&session, signal_pid) {
                daemon.sessions.register_signal(session, pid);
            }
            let state = JsonState::new(&daemon.cfg.state_dir);
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
                    let _ = send(&mut stream, &frames::error(&e.to_string()));
                    return;
                }
            };
            let pending = match &decision {
                TriageDecision::Offer { case, fix: None } => {
                    let state = JsonState::new(&daemon.cfg.state_dir);
                    let follow_up = FollowUp {
                        settings: &settings,
                        clock: &SystemClock,
                        secrets: &EnvSecrets,
                        models: &HttpModels,
                        notifier: &daemon.sessions,
                        cases: &state,
                    };
                    follow_up.candidate(case)
                }
                _ => None,
            };
            let mut text = toast(&decision, &style);
            if let (Some(t), Some(model)) = (text.as_mut(), pending.as_deref()) {
                t.push('\n');
                t.push_str(&pending_line(model, &style));
            }
            let bubbles = session
                .as_ref()
                .map(|s| daemon.sessions.drain(s, color))
                .unwrap_or_default();
            let _ = send(
                &mut stream,
                &frames::decision(&decision, text.as_deref(), &bubbles, pending.as_deref()),
            );
            if let (TriageDecision::Offer { case, fix: None }, Some(_)) = (decision, &pending) {
                let daemon = Arc::clone(daemon);
                std::thread::spawn(move || {
                    let state = JsonState::new(&daemon.cfg.state_dir);
                    let follow_up = FollowUp {
                        settings: &settings,
                        clock: &SystemClock,
                        secrets: &EnvSecrets,
                        models: &HttpModels,
                        notifier: &daemon.sessions,
                        cases: &state,
                    };
                    if let Err(e) = follow_up.run(&case) {
                        log(&format!("follow-up for {}: {e}", case.outcome().command()));
                    }
                });
            }
        }
        Request::Subscribe { session, color } => {
            if send(&mut stream, &frames::ack()).is_ok() {
                let _ = stream.set_read_timeout(None);
                daemon.sessions.attach(&session, stream, color);
            }
        }
        Request::Pending { session, color } => {
            for text in daemon.sessions.drain(&session, color) {
                let placeholder = Message::new(
                    crate::entities::CaseId::new(""),
                    crate::entities::Timestamp::from_millis(0),
                    crate::entities::MessageBody::Explanation {
                        model: String::new(),
                        text: String::new(),
                    },
                );
                let _ = send(&mut stream, &frames::bubble(&placeholder, &text));
            }
            let _ = send(&mut stream, &frames::done());
        }
        Request::Explain { session, color: _ } => {
            let settings = daemon.settings();
            let state = JsonState::new(&daemon.cfg.state_dir);
            let explain = Explain {
                settings: &settings,
                cases: &state,
                secrets: &EnvSecrets,
                models: &HttpModels,
            };
            match explain.candidate(Some(&session)) {
                Err(e) => {
                    let _ = send(&mut stream, &frames::error(&e.to_string()));
                }
                Ok(model) => {
                    let _ = send(&mut stream, &frames::asked(&model));
                    let daemon = Arc::clone(daemon);
                    std::thread::spawn(move || {
                        let state = JsonState::new(&daemon.cfg.state_dir);
                        let explain = Explain {
                            settings: &settings,
                            cases: &state,
                            secrets: &EnvSecrets,
                            models: &HttpModels,
                        };
                        if let Err(e) = explain.deliver(&session, &daemon.sessions, &SystemClock) {
                            log(&format!("explain for {}: {e}", session.as_str()));
                        }
                    });
                }
            }
        }
        Request::Shutdown => {
            let _ = send(&mut stream, &frames::bye());
            daemon.quit();
        }
    }
}

struct SessionState {
    pending: VecDeque<Message>,
    subscriber: Option<(UnixStream, bool)>,
    signal_pid: Option<u32>,
}

/// Every shell the daemon has heard from, with what it owes them.
#[derive(Default)]
struct Sessions {
    inner: Mutex<HashMap<String, SessionState>>,
    ascii: AtomicBool,
    silent: AtomicBool,
}

impl Sessions {
    fn with<R>(&self, id: &SessionId, f: impl FnOnce(&mut SessionState, &Sessions) -> R) -> R {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let state = map
            .entry(id.as_str().to_string())
            .or_insert_with(|| SessionState {
                pending: VecDeque::new(),
                subscriber: None,
                signal_pid: None,
            });
        f(state, self)
    }

    fn style(&self, color: bool) -> Style {
        Style {
            color,
            ascii: self.ascii.load(Ordering::Relaxed),
            mode: UiMode::Toast,
        }
    }

    /// A new subscriber replaces the old one and receives what was waiting.
    fn attach(&self, id: &SessionId, mut stream: UnixStream, color: bool) {
        self.with(id, |state, sessions| {
            let style = sessions.style(color);
            while let Some(message) = state.pending.pop_front() {
                if send(
                    &mut stream,
                    &frames::bubble(&message, &message_toast(&message, &style)),
                )
                .is_err()
                {
                    state.pending.push_front(message);
                    return;
                }
            }
            state.subscriber = Some((stream, color));
        });
    }

    fn register_signal(&self, id: &SessionId, pid: u32) {
        self.with(id, |state, _| state.signal_pid = Some(pid));
    }

    /// The pending messages, rendered, and forgotten.
    fn drain(&self, id: &SessionId, color: bool) -> Vec<String> {
        self.with(id, |state, sessions| {
            let style = sessions.style(color);
            state
                .pending
                .drain(..)
                .map(|m| message_toast(&m, &style))
                .collect()
        })
    }

    /// Writes a ping to every subscriber; the ones that are gone are dropped.
    fn ping(&self) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        for state in map.values_mut() {
            if let Some((stream, _)) = state.subscriber.as_mut() {
                if send(stream, &frames::ping()).is_err() {
                    state.subscriber = None;
                }
            }
        }
    }
}

impl Notifier for Sessions {
    fn deliver(&self, session: &SessionId, message: Message) -> Result<(), NotifyError> {
        if self.silent.load(Ordering::Relaxed) {
            return Ok(());
        }
        self.with(session, |state, sessions| {
            if let Some((stream, color)) = state.subscriber.as_mut() {
                let text = message_toast(&message, &sessions.style(*color));
                if send(stream, &frames::bubble(&message, &text)).is_ok() {
                    return;
                }
                state.subscriber = None;
            }
            state.pending.push_back(message);
            if let Some(pid) = state.signal_pid {
                if !os::signal_usr1(pid) {
                    state.signal_pid = None;
                }
            }
        });
        Ok(())
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

fn send(stream: &mut UnixStream, line: &str) -> io::Result<()> {
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()
}

fn log(message: &str) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    eprintln!("{secs} {message}");
}

/// The few calls that need the C library: leaving the terminal's session,
/// knowing who is on the other end of the socket, poking a shell.
#[allow(unsafe_code)]
mod os {
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    /// A session of its own and no SIGHUP when the terminal closes.
    pub fn detach() {
        // SAFETY: setsid and signal take no pointers and cannot break memory safety.
        unsafe {
            libc::setsid();
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
        }
    }

    pub fn uid() -> u32 {
        // SAFETY: getuid has no preconditions.
        unsafe { libc::getuid() }
    }

    #[cfg(target_os = "linux")]
    pub fn peer_uid(stream: &UnixStream) -> Option<u32> {
        let mut cred = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: the buffer and its length describe a valid ucred.
        let rc = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut cred as *mut libc::ucred).cast(),
                &mut len,
            )
        };
        (rc == 0).then_some(cred.uid)
    }

    #[cfg(not(target_os = "linux"))]
    pub fn peer_uid(stream: &UnixStream) -> Option<u32> {
        let mut uid: libc::uid_t = 0;
        let mut gid: libc::gid_t = 0;
        // SAFETY: both out-pointers point to live locals.
        let rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
        (rc == 0).then_some(uid)
    }

    /// True when the signal was sent; false when the process is gone.
    pub fn signal_usr1(pid: u32) -> bool {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        // SAFETY: kill with a valid signal number has no memory effects.
        unsafe { libc::kill(pid, libc::SIGUSR1) == 0 }
    }
}
