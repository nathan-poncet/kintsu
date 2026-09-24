//! The shells the daemon has heard from: the subscriber connection that
//! receives bubbles live, the pid to poke with SIGUSR1, and the messages
//! not yet seen. This is the daemon's `Notifier`. How a message reads is
//! not decided here: the composition root hands in the renderer.

use std::collections::{HashMap, VecDeque};
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::adapters::gateways::ndjson::send_line;
use crate::adapters::gateways::unix;
use crate::entities::{Message, SessionId};
use crate::use_cases::ports::{Notifier, NotifyError};

/// Renders a message into the frame line a subscriber receives; the bool
/// is whether that subscriber's terminal wants colour.
pub type RenderBubble = Box<dyn Fn(&Message, bool) -> String + Send + Sync>;

struct SessionState {
    pending: VecDeque<Message>,
    subscriber: Option<(UnixStream, bool)>,
    signal_pid: Option<u32>,
}

pub struct Sessions {
    inner: Mutex<HashMap<String, SessionState>>,
    render: RenderBubble,
    ping: String,
    silent: AtomicBool,
}

impl Sessions {
    /// `ping` is the frame written to subscribers so dead ones are noticed.
    pub fn new(render: RenderBubble, ping: String) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            render,
            ping,
            silent: AtomicBool::new(false),
        }
    }

    /// Silent shells keep their proposals but get no message.
    pub fn set_silent(&self, silent: bool) {
        self.silent.store(silent, Ordering::Relaxed);
    }

    fn with<R>(&self, id: &SessionId, f: impl FnOnce(&mut SessionState, &Self) -> R) -> R {
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

    /// A new subscriber replaces the old one and receives what was waiting.
    pub fn attach(&self, id: &SessionId, mut stream: UnixStream, color: bool) {
        self.with(id, |state, sessions| {
            while let Some(message) = state.pending.pop_front() {
                if send_line(&mut stream, &(sessions.render)(&message, color)).is_err() {
                    state.pending.push_front(message);
                    return;
                }
            }
            state.subscriber = Some((stream, color));
        });
    }

    /// The shell wants SIGUSR1 when a message waits for it.
    pub fn register_signal(&self, id: &SessionId, pid: u32) {
        self.with(id, |state, _| state.signal_pid = Some(pid));
    }

    /// The messages not yet seen, oldest first, and forgotten.
    pub fn drain(&self, id: &SessionId) -> Vec<Message> {
        self.with(id, |state, _| state.pending.drain(..).collect())
    }

    /// Writes the ping to every subscriber; the ones that are gone are dropped.
    pub fn ping(&self) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        for state in map.values_mut() {
            if let Some((stream, _)) = state.subscriber.as_mut() {
                if send_line(stream, &self.ping).is_err() {
                    state.subscriber = None;
                }
            }
        }
    }
}

impl Notifier for Sessions {
    /// A live subscriber gets the message at once; otherwise it waits, and
    /// a shell that asked for it is poked.
    fn deliver(&self, session: &SessionId, message: Message) -> Result<(), NotifyError> {
        if self.silent.load(Ordering::Relaxed) {
            return Ok(());
        }
        self.with(session, |state, sessions| {
            if let Some((stream, color)) = state.subscriber.as_mut() {
                if send_line(stream, &(sessions.render)(&message, *color)).is_ok() {
                    return;
                }
                state.subscriber = None;
            }
            state.pending.push_back(message);
            if let Some(pid) = state.signal_pid {
                if !unix::signal_usr1(pid) {
                    state.signal_pid = None;
                }
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::gateways::ndjson::read_line;
    use crate::entities::{CaseId, MessageBody, Timestamp};

    fn sessions() -> Sessions {
        Sessions::new(
            Box::new(|m, color| match m.body() {
                MessageBody::Note(t) => format!("{t}{}", if color { "!" } else { "" }),
                _ => "?".into(),
            }),
            "ping".into(),
        )
    }

    fn note(text: &str) -> Message {
        Message::new(
            CaseId::new("c"),
            Timestamp::from_millis(0),
            MessageBody::Note(text.into()),
        )
    }

    #[test]
    fn a_subscriber_gets_messages_live_and_what_waited_before_it_came() {
        let s = sessions();
        let id = SessionId::new("42");
        s.deliver(&id, note("early")).unwrap();
        let (client, mut server_side) = UnixStream::pair().unwrap();
        s.attach(&id, client, true);
        assert_eq!(read_line(&mut server_side).unwrap(), "early!\n");
        s.deliver(&id, note("live")).unwrap();
        assert_eq!(read_line(&mut server_side).unwrap(), "live!\n");
        assert!(s.drain(&id).is_empty());
        s.ping();
        assert_eq!(read_line(&mut server_side).unwrap(), "ping\n");
    }

    #[test]
    fn a_gone_subscriber_is_dropped_and_messages_wait_again() {
        let s = sessions();
        let id = SessionId::new("42");
        // A stream that refuses every write, deterministically on every
        // platform: our own end with its write half shut. What the kernel
        // does with a peer that just closed is not this test's business.
        let (client, _server_side) = UnixStream::pair().unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        s.attach(&id, client, false);
        s.deliver(&id, note("first")).unwrap();
        s.deliver(&id, note("second")).unwrap();
        let waiting: Vec<String> = s
            .drain(&id)
            .iter()
            .map(|m| match m.body() {
                MessageBody::Note(t) => t.clone(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            waiting,
            vec!["first", "second"],
            "dropped on the first failed write, everything waits"
        );
    }

    #[test]
    fn silence_drops_messages_and_sessions_are_independent() {
        let s = sessions();
        s.set_silent(true);
        s.deliver(&SessionId::new("a"), note("x")).unwrap();
        assert!(s.drain(&SessionId::new("a")).is_empty());
        s.set_silent(false);
        s.deliver(&SessionId::new("a"), note("x")).unwrap();
        assert!(s.drain(&SessionId::new("b")).is_empty());
        assert_eq!(s.drain(&SessionId::new("a")).len(), 1);
    }
}
