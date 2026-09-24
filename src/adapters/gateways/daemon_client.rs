//! The thin client side of the daemon protocol: connect, send one frame,
//! read the answer. Starts the daemon when nothing answers, at most once
//! a minute, and never waits longer than the caller allows.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Value, json};

use crate::entities::SessionId;
use crate::use_cases::TriageInput;

/// What the daemon decided, already rendered for this terminal.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DecisionView {
    pub toast: Option<String>,
    pub bubbles: Vec<String>,
    /// The model being asked in the background, when one is.
    pub pending: Option<String>,
}

pub struct DaemonClient {
    socket: PathBuf,
    exe: PathBuf,
    log: PathBuf,
    version: &'static str,
}

/// How long a freshly spawned daemon may take to listen.
const SPAWN_WAIT: Duration = Duration::from_millis(400);
/// How long to wait before trying to spawn again after a failure.
const SPAWN_BACKOFF: Duration = Duration::from_secs(60);

impl DaemonClient {
    pub fn new(
        socket: impl Into<PathBuf>,
        exe: impl Into<PathBuf>,
        log: impl Into<PathBuf>,
    ) -> Self {
        Self {
            socket: socket.into(),
            exe: exe.into(),
            log: log.into(),
            version: env!("CARGO_PKG_VERSION"),
        }
    }

    /// Reports a finished command line and waits `budget` for the decision.
    /// `None` means the caller should decide locally.
    pub fn command_finished(
        &self,
        input: &TriageInput,
        color: bool,
        signal_pid: Option<u32>,
        budget: Duration,
    ) -> Option<DecisionView> {
        let mut stream = self.connect_or_spawn().ok()?;
        stream.set_read_timeout(Some(budget)).ok()?;
        stream.set_write_timeout(Some(budget)).ok()?;
        let frame = json!({
            "v": 1,
            "type": "command_finished",
            "version": self.version,
            "session": input.session.as_ref().map(|s| s.as_str()),
            "command": input.outcome.command().as_str(),
            "status": input.outcome.status().code(),
            "duration_ms": input.outcome.duration().map(|d| d.as_millis()),
            "cwd": input.cwd,
            "shell": input.shell.map(|s| s.name()),
            "color": color,
            "signal_pid": signal_pid,
        });
        send_line(&mut stream, &frame.to_string()).ok()?;
        let answer: Value = serde_json::from_str(&read_line(&mut stream).ok()?).ok()?;
        if answer["type"] != "decision" {
            return None;
        }
        Some(DecisionView {
            toast: answer["toast"].as_str().map(String::from),
            bubbles: texts(&answer["bubbles"]),
            pending: answer["pending"].as_str().map(String::from),
        })
    }

    /// Opens a subscription; the caller reads bubble frames from the stream.
    pub fn subscribe(&self, session: &SessionId, color: bool) -> io::Result<UnixStream> {
        let mut stream = self.connect_or_spawn()?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        let frame = json!({"v": 1, "type": "subscribe", "version": self.version, "session": session.as_str(), "color": color});
        send_line(&mut stream, &frame.to_string())?;
        let answer: Value =
            serde_json::from_str(&read_line(&mut stream)?).map_err(io::Error::other)?;
        if answer["type"] != "ack" {
            return Err(io::Error::other(format!(
                "daemon answered {}",
                answer["type"]
            )));
        }
        Ok(stream)
    }

    /// The bubbles a session has not seen; nothing when no daemon runs.
    pub fn pending(&self, session: &SessionId, color: bool) -> io::Result<Vec<String>> {
        let mut stream = match self.connect() {
            Ok(s) => s,
            Err(_) => return Ok(Vec::new()),
        };
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let frame = json!({"v": 1, "type": "pending", "version": self.version, "session": session.as_str(), "color": color});
        send_line(&mut stream, &frame.to_string())?;
        let mut out = Vec::new();
        let mut reader = BufReader::new(stream);
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                break;
            }
            let v: Value = serde_json::from_str(&line).map_err(io::Error::other)?;
            match v["type"].as_str() {
                Some("bubble") => out.extend(v["text"].as_str().map(String::from)),
                Some("done") | Some("bye") => break,
                _ => {}
            }
        }
        Ok(out)
    }

    /// The daemon's version, when one answers.
    pub fn hello(&self) -> io::Result<String> {
        let mut stream = self.connect()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        send_line(
            &mut stream,
            &json!({"v": 1, "type": "hello", "version": self.version}).to_string(),
        )?;
        let v: Value = serde_json::from_str(&read_line(&mut stream)?).map_err(io::Error::other)?;
        v["version"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| io::Error::other("no version in the answer"))
    }

    /// Asks the daemon to exit. `Ok(false)` when none was running.
    pub fn shutdown(&self) -> io::Result<bool> {
        let mut stream = match self.connect() {
            Ok(s) => s,
            Err(_) => return Ok(false),
        };
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        send_line(
            &mut stream,
            &json!({"v": 1, "type": "shutdown", "version": self.version}).to_string(),
        )?;
        let _ = read_line(&mut stream);
        // A deliberate stop must not keep the next hook call from restarting it.
        let _ = std::fs::remove_file(self.socket.with_extension("spawn"));
        Ok(true)
    }

    /// The text of a bubble frame, for the subscriber loop.
    pub fn bubble_text(line: &str) -> Option<String> {
        let v: Value = serde_json::from_str(line).ok()?;
        (v["type"] == "bubble")
            .then(|| v["text"].as_str().map(String::from))
            .flatten()
    }

    fn connect(&self) -> io::Result<UnixStream> {
        UnixStream::connect(&self.socket)
    }

    fn connect_or_spawn(&self) -> io::Result<UnixStream> {
        let first = match self.connect() {
            Ok(stream) => return Ok(stream),
            Err(e) => e,
        };
        if !self.may_spawn() {
            return Err(first);
        }
        self.spawn()?;
        let deadline = Instant::now() + SPAWN_WAIT;
        loop {
            std::thread::sleep(Duration::from_millis(15));
            if let Ok(stream) = self.connect() {
                return Ok(stream);
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the daemon did not start in time",
                ));
            }
        }
    }

    /// True once per backoff window: a daemon that cannot start must not
    /// cost every prompt a wait.
    fn may_spawn(&self) -> bool {
        let marker = self.socket.with_extension("spawn");
        let recent = std::fs::metadata(&marker)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < SPAWN_BACKOFF);
        if recent {
            return false;
        }
        if let Some(parent) = marker.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&marker, b"").is_ok()
    }

    fn spawn(&self) -> io::Result<()> {
        if let Some(parent) = self.log.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log)?;
        let log_err = log.try_clone()?;
        Command::new(&self.exe)
            .args(["daemon", "run"])
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(log_err)
            .spawn()
            .map(|_| ())
    }
}

fn send_line(stream: &mut UnixStream, line: &str) -> io::Result<()> {
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()
}

fn read_line(stream: &mut UnixStream) -> io::Result<String> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "the daemon closed the connection",
        ));
    }
    Ok(line)
}

fn texts(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|b| b.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bubble_text_reads_bubbles_and_nothing_else() {
        assert_eq!(
            DaemonClient::bubble_text(r#"{"v":1,"type":"bubble","case":"c","text":"▎ hi"}"#)
                .as_deref(),
            Some("▎ hi")
        );
        assert_eq!(DaemonClient::bubble_text(r#"{"v":1,"type":"ping"}"#), None);
        assert_eq!(DaemonClient::bubble_text("nope"), None);
    }

    #[test]
    fn without_a_daemon_pending_is_empty_and_shutdown_says_so() {
        let dir = std::env::temp_dir().join(format!("kintsu-client-{}", std::process::id()));
        let client = DaemonClient::new(
            dir.join("d.sock"),
            "/definitely/not/kintsu",
            dir.join("d.log"),
        );
        assert_eq!(
            client.pending(&SessionId::new("s"), false).unwrap(),
            Vec::<String>::new()
        );
        assert!(!client.shutdown().unwrap());
        assert!(client.hello().is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
