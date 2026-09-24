//! The daemon protocol, client to daemon: one JSON object per line, `v: 1`
//! on every frame. This is the only place that reads frames.

use serde_json::Value;
use thiserror::Error;

use crate::entities::{CommandLine, CommandOutcome, Duration, ExitStatus, SessionId, Shell};
use crate::use_cases::TriageInput;

/// The protocol version every frame carries.
pub const PROTOCOL_VERSION: u64 = 1;

/// One parsed line: the client's version when it said it, and the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub client_version: Option<String>,
    pub request: Request,
}

/// What a client asked the daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Version handshake.
    Hello { version: String },
    /// A hook reports a finished command line; wants a decision within the budget.
    CommandFinished {
        input: TriageInput,
        color: bool,
        signal_pid: Option<u32>,
    },
    /// A shell wants bubbles as they come, on this connection.
    Subscribe { session: SessionId, color: bool },
    /// A shell asks for the bubbles it has not seen.
    Pending { session: SessionId, color: bool },
    /// `kintsu why`: explain the session's last failure later, as a message.
    Explain { session: SessionId },
    /// Stop the daemon.
    Shutdown,
}

/// Why a line was not a frame.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FrameError {
    #[error("not JSON: {0}")]
    Json(String),
    #[error("unsupported protocol version {0}")]
    Version(u64),
    #[error("unknown frame type `{0}`")]
    UnknownType(String),
    #[error("frame lacks `{0}`")]
    Missing(&'static str),
    #[error("`{0}` has the wrong shape")]
    Invalid(&'static str),
}

pub fn parse_frame(line: &str) -> Result<Frame, FrameError> {
    let v: Value = serde_json::from_str(line).map_err(|e| FrameError::Json(e.to_string()))?;
    let client_version = v.get("version").and_then(Value::as_str).map(String::from);
    Ok(Frame {
        client_version,
        request: parse_request(&v)?,
    })
}

fn parse_request(v: &Value) -> Result<Request, FrameError> {
    let version = v
        .get("v")
        .and_then(Value::as_u64)
        .unwrap_or(PROTOCOL_VERSION);
    if version != PROTOCOL_VERSION {
        return Err(FrameError::Version(version));
    }
    let kind = v
        .get("type")
        .and_then(Value::as_str)
        .ok_or(FrameError::Missing("type"))?;
    let required = |key: &'static str| -> Result<String, FrameError> {
        v.get(key)
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or(FrameError::Missing(key))
    };
    let optional = |key: &str| {
        v.get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    let color = v.get("color").and_then(Value::as_bool).unwrap_or(false);
    match kind {
        "hello" => Ok(Request::Hello {
            version: required("version")?,
        }),
        "command_finished" => {
            let status = v
                .get("status")
                .and_then(Value::as_i64)
                .ok_or(FrameError::Missing("status"))?;
            let status = i32::try_from(status).map_err(|_| FrameError::Invalid("status"))?;
            let command = CommandLine::new(required("command")?)
                .map_err(|_| FrameError::Invalid("command"))?;
            let mut outcome = CommandOutcome::new(command, ExitStatus::new(status));
            if let Some(ms) = v.get("duration_ms").and_then(Value::as_u64) {
                outcome = outcome.lasting(Duration::from_millis(ms));
            }
            let input = TriageInput {
                outcome,
                cwd: optional("cwd"),
                session: optional("session").map(SessionId::new),
                shell: optional("shell").and_then(|s| Shell::from_name(&s)),
            };
            let signal_pid = v
                .get("signal_pid")
                .and_then(Value::as_u64)
                .and_then(|p| u32::try_from(p).ok());
            Ok(Request::CommandFinished {
                input,
                color,
                signal_pid,
            })
        }
        "subscribe" => Ok(Request::Subscribe {
            session: SessionId::new(required("session")?),
            color,
        }),
        "pending" => Ok(Request::Pending {
            session: SessionId::new(required("session")?),
            color,
        }),
        "explain" => Ok(Request::Explain {
            session: SessionId::new(required("session")?),
        }),
        "shutdown" => Ok(Request::Shutdown),
        other => Err(FrameError::UnknownType(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_frame(line: &str) -> Result<Request, FrameError> {
        super::parse_frame(line).map(|f| f.request)
    }

    #[test]
    fn the_clients_version_travels_with_any_frame() {
        assert_eq!(
            super::parse_frame(r#"{"type":"shutdown","version":"0.1.0"}"#)
                .unwrap()
                .client_version
                .as_deref(),
            Some("0.1.0")
        );
        assert_eq!(
            super::parse_frame(r#"{"type":"shutdown"}"#)
                .unwrap()
                .client_version,
            None
        );
    }

    #[test]
    fn a_finished_command_becomes_a_triage_input() {
        let line = r#"{"v":1,"type":"command_finished","session":"42","command":"make test","status":2,"duration_ms":12000,"cwd":"/w","shell":"zsh","color":true,"signal_pid":4242}"#;
        let Request::CommandFinished {
            input,
            color,
            signal_pid,
        } = parse_frame(line).unwrap()
        else {
            panic!()
        };
        assert_eq!(input.outcome.command().as_str(), "make test");
        assert_eq!(input.outcome.status(), ExitStatus::new(2));
        assert_eq!(input.outcome.duration(), Some(Duration::from_secs(12)));
        assert_eq!(input.cwd.as_deref(), Some("/w"));
        assert_eq!(input.session, Some(SessionId::new("42")));
        assert_eq!(input.shell, Some(Shell::Zsh));
        assert!(color);
        assert_eq!(signal_pid, Some(4242));
        let bare = parse_frame(
            r#"{"v":1,"type":"command_finished","command":"ls","status":0,"session":""}"#,
        )
        .unwrap();
        let Request::CommandFinished {
            input,
            color,
            signal_pid,
        } = bare
        else {
            panic!()
        };
        assert_eq!(
            (input.session, input.cwd, input.shell, color, signal_pid),
            (None, None, None, false, None)
        );
    }

    #[test]
    fn the_other_frames_parse() {
        assert_eq!(
            parse_frame(r#"{"v":1,"type":"hello","version":"0.1.0"}"#).unwrap(),
            Request::Hello {
                version: "0.1.0".into()
            }
        );
        assert_eq!(
            parse_frame(r#"{"type":"subscribe","session":"7","color":true}"#).unwrap(),
            Request::Subscribe {
                session: SessionId::new("7"),
                color: true
            }
        );
        assert_eq!(
            parse_frame(r#"{"type":"pending","session":"7"}"#).unwrap(),
            Request::Pending {
                session: SessionId::new("7"),
                color: false
            }
        );
        assert_eq!(
            parse_frame(r#"{"type":"shutdown"}"#).unwrap(),
            Request::Shutdown
        );
        assert_eq!(
            parse_frame(r#"{"type":"explain","session":"7"}"#).unwrap(),
            Request::Explain {
                session: SessionId::new("7")
            }
        );
    }

    #[test]
    fn bad_frames_are_named() {
        assert!(matches!(parse_frame("nope"), Err(FrameError::Json(_))));
        assert_eq!(
            parse_frame(r#"{"v":2,"type":"shutdown"}"#),
            Err(FrameError::Version(2))
        );
        assert_eq!(
            parse_frame(r#"{"type":"dance"}"#),
            Err(FrameError::UnknownType("dance".into()))
        );
        assert_eq!(
            parse_frame(r#"{"type":"command_finished","status":1}"#),
            Err(FrameError::Missing("command"))
        );
        assert_eq!(
            parse_frame(r#"{"type":"command_finished","status":1,"command":"  "}"#),
            Err(FrameError::Invalid("command"))
        );
        assert_eq!(
            parse_frame(r#"{"type":"subscribe"}"#),
            Err(FrameError::Missing("session"))
        );
    }
}
