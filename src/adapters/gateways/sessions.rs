//! The shells the daemon has heard from: what each said about itself
//! when it started, the subscriber connection that receives bubbles live,
//! the pid to poke with SIGUSR1, and the messages not yet seen. This is
//! the daemon's `Notifier`. How a message reads is not decided here: the
//! composition root hands in the renderer.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::adapters::gateways::ndjson::send_line;
use crate::adapters::gateways::unix;
use crate::entities::{
    CaseId, Language, Message, SessionDetails, SessionId, Shell, ShellCommands, TerminalIdentity,
};
use crate::use_cases::ports::{Notifier, NotifyError};

/// Renders a message into the frame line a subscriber receives; the bool
/// is whether that subscriber's terminal wants colour.
pub type RenderBubble = Box<dyn Fn(&Message, bool) -> String + Send + Sync>;

struct SessionState {
    pending: VecDeque<Message>,
    subscriber: Option<(UnixStream, bool)>,
    signal_pid: Option<u32>,
    shell: Option<Shell>,
    /// The shell's own pid, from its registration: gone means forgotten.
    pid: Option<u32>,
    tty: Option<String>,
    terminal: Option<TerminalIdentity>,
    path: Option<String>,
    env: BTreeMap<String, String>,
    language: Option<Language>,
    /// The aliases and functions the shell listed at its start.
    commands: ShellCommands,
}

impl SessionState {
    fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            subscriber: None,
            signal_pid: None,
            shell: None,
            pid: None,
            tty: None,
            terminal: None,
            path: None,
            env: BTreeMap::new(),
            language: None,
            commands: ShellCommands::default(),
        }
    }
}

/// Clicks on older bubbles than this get "this case is gone".
const REMEMBERED_CASES: usize = 1000;

pub struct Sessions {
    inner: Mutex<HashMap<String, SessionState>>,
    /// Which session each offered case belongs to, for clicks.
    cases: Mutex<HashMap<String, SessionId>>,
    render: RenderBubble,
    ping: String,
    silent: AtomicBool,
}

impl Sessions {
    /// `ping` is the frame written to subscribers so dead ones are noticed.
    pub fn new(render: RenderBubble, ping: String) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            cases: Mutex::new(HashMap::new()),
            render,
            ping,
            silent: AtomicBool::new(false),
        }
    }

    /// A case was offered in a session; a click on it comes back here.
    pub fn remember_case(&self, case: &CaseId, session: &SessionId) {
        let mut cases = self.cases.lock().unwrap_or_else(|e| e.into_inner());
        if cases.len() >= REMEMBERED_CASES
            && let Some(victim) = cases.keys().next().cloned()
        {
            cases.remove(&victim);
        }
        cases.insert(case.as_str().to_string(), session.clone());
    }

    pub fn session_of(&self, case: &CaseId) -> Option<SessionId> {
        self.cases
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(case.as_str())
            .cloned()
    }

    /// Silent shells keep their proposals but get no message.
    pub fn set_silent(&self, silent: bool) {
        self.silent.store(silent, Ordering::Relaxed);
    }

    fn with<R>(&self, id: &SessionId, f: impl FnOnce(&mut SessionState, &Self) -> R) -> R {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let state = map
            .entry(id.as_str().to_string())
            .or_insert_with(SessionState::new);
        f(state, self)
    }

    /// A read: nothing is created for a shell the daemon never heard from.
    fn peek<R>(&self, id: &SessionId, f: impl FnOnce(&SessionState) -> R) -> Option<R> {
        let map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.get(id.as_str()).map(f)
    }

    /// What the shell said about itself when it started. Only what it
    /// said replaces what was known: a registration without a PATH keeps
    /// the last one.
    pub fn register(&self, id: &SessionId, details: SessionDetails) {
        self.with(id, |state, _| {
            state.shell = details.shell.or(state.shell);
            state.pid = details.pid.or(state.pid);
            state.tty = details.tty.or(state.tty.take());
            if details.terminal.is_known() {
                state.terminal = Some(details.terminal);
            }
            if details.path.is_some() {
                state.path = details.path;
            }
            if !details.env.is_empty() {
                state.env = details.env;
            }
            state.language = details.language.or(state.language);
            if !details.commands.is_empty() {
                state.commands = details.commands;
            }
        });
    }

    /// What the shell said it can run besides its PATH, for the rules that
    /// look up a program the way the shell would.
    pub fn commands_of(&self, id: &SessionId) -> ShellCommands {
        self.peek(id, |state| state.commands.clone())
            .unwrap_or_default()
    }

    /// One line about a registered session, for the log.
    pub fn describe(&self, id: &SessionId) -> String {
        self.peek(id, |state| {
            format!(
                "session {} registered: {} pid {} on {}",
                id.as_str(),
                state.shell.map_or("a shell", Shell::name),
                state.pid.map_or("?".to_string(), |p| p.to_string()),
                state.tty.as_deref().unwrap_or("no tty"),
            )
        })
        .unwrap_or_else(|| format!("session {} unknown", id.as_str()))
    }

    /// The pane the shell registered with, for the output capture when a
    /// frame does not name one.
    pub fn terminal_of(&self, id: &SessionId) -> Option<TerminalIdentity> {
        self.peek(id, |state| state.terminal.clone()).flatten()
    }

    /// Drops the sessions whose shell is gone and that nobody listens for:
    /// no subscriber, and a pid that `alive` denies. A session that gave
    /// no pid and has none to parse from its id is kept, there is no way
    /// to tell. Returns how many were forgotten.
    pub fn forget_gone(&self, alive: impl Fn(u32) -> bool) -> usize {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let before = map.len();
        map.retain(|id, state| {
            let pid = state.pid.or(state.signal_pid).or_else(|| id.parse().ok());
            state.subscriber.is_some() || pid.is_none_or(&alive)
        });
        before - map.len()
    }

    /// How many shells are known.
    pub fn count(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).len()
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

    /// The PATH the shell reported last: what its rules should look at
    /// when a click asks for a fix later.
    pub fn remember_path(&self, id: &SessionId, path: String) {
        self.with(id, |state, _| state.path = Some(path));
    }

    pub fn path_of(&self, id: &SessionId) -> Option<String> {
        self.peek(id, |state| state.path.clone()).flatten()
    }

    /// The key variables the shell forwarded last, for the models the
    /// daemon asks on its behalf. Memory only, never written anywhere.
    pub fn remember_env(&self, id: &SessionId, env: BTreeMap<String, String>) {
        self.with(id, |state, _| state.env = env);
    }

    pub fn env_of(&self, id: &SessionId) -> BTreeMap<String, String> {
        self.peek(id, |state| state.env.clone()).unwrap_or_default()
    }

    /// The language the shell's locale named last, for the models the
    /// daemon asks on its behalf.
    pub fn remember_language(&self, id: &SessionId, language: Language) {
        self.with(id, |state, _| state.language = Some(language));
    }

    pub fn language_of(&self, id: &SessionId) -> Option<Language> {
        self.peek(id, |state| state.language).flatten()
    }

    /// The messages not yet seen, oldest first, and forgotten.
    pub fn drain(&self, id: &SessionId) -> Vec<Message> {
        self.with(id, |state, _| state.pending.drain(..).collect())
    }

    /// Writes the ping to every subscriber; the ones that are gone are dropped.
    pub fn ping(&self) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        for state in map.values_mut() {
            if let Some((stream, _)) = state.subscriber.as_mut()
                && send_line(stream, &self.ping).is_err()
            {
                state.subscriber = None;
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
            if let Some(pid) = state.signal_pid
                && !unix::signal_usr1(pid)
            {
                state.signal_pid = None;
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
    fn a_case_remembers_its_session_for_clicks() {
        let s = sessions();
        assert_eq!(s.session_of(&CaseId::new("c")), None);
        s.remember_case(&CaseId::new("c"), &SessionId::new("42"));
        assert_eq!(s.session_of(&CaseId::new("c")), Some(SessionId::new("42")));
    }

    #[test]
    fn a_session_remembers_the_path_its_shell_reported_last() {
        let s = sessions();
        let id = SessionId::new("42");
        assert_eq!(s.path_of(&id), None);
        s.remember_path(&id, "/a/bin:/usr/bin".into());
        s.remember_path(&id, "/b/bin:/usr/bin".into());
        assert_eq!(s.path_of(&id).as_deref(), Some("/b/bin:/usr/bin"));
        assert_eq!(s.path_of(&SessionId::new("43")), None);
    }

    #[test]
    fn a_registration_is_kept_and_only_what_was_said_replaces_what_was_known() {
        let s = sessions();
        let id = SessionId::new("42");
        s.register(
            &id,
            SessionDetails {
                shell: Some(Shell::Zsh),
                pid: Some(42),
                tty: Some("/dev/ttys003".into()),
                terminal: TerminalIdentity {
                    tmux_pane: Some("%1".into()),
                    ..TerminalIdentity::default()
                },
                path: Some("/a/bin".into()),
                env: BTreeMap::from([("K".to_string(), "v".to_string())]),
                language: Some(Language::French),
                commands: ShellCommands::parse("hmz\t~/x/hmz\nmkcd\n"),
            },
        );
        assert_eq!(s.language_of(&id), Some(Language::French));
        assert_eq!(s.path_of(&id).as_deref(), Some("/a/bin"));
        assert_eq!(s.env_of(&id).get("K").map(String::as_str), Some("v"));
        assert_eq!(s.commands_of(&id).functions, vec!["mkcd"]);
        assert_eq!(
            s.commands_of(&id).aliases[0].expansion,
            "~/x/hmz".to_string()
        );
        assert_eq!(
            s.terminal_of(&id).and_then(|t| t.tmux_pane),
            Some("%1".to_string())
        );
        assert_eq!(
            s.describe(&id),
            "session 42 registered: zsh pid 42 on /dev/ttys003"
        );
        s.register(&id, SessionDetails::default());
        assert_eq!(s.path_of(&id).as_deref(), Some("/a/bin"), "kept");
        assert!(s.terminal_of(&id).is_some(), "kept");
        assert_eq!(s.commands_of(&id).functions, vec!["mkcd"], "kept");
        assert!(s.commands_of(&SessionId::new("43")).is_empty());
        assert_eq!(
            s.describe(&id),
            "session 42 registered: zsh pid 42 on /dev/ttys003",
            "kept too"
        );
        assert_eq!(s.count(), 1);
        assert_eq!(s.path_of(&SessionId::new("43")), None);
        assert_eq!(s.count(), 1, "a read creates nothing");
        assert_eq!(s.describe(&SessionId::new("43")), "session 43 unknown");
    }

    #[test]
    fn a_session_whose_shell_is_gone_and_nobody_listens_for_is_forgotten() {
        let s = sessions();
        s.register(
            &SessionId::new("a"),
            SessionDetails {
                pid: Some(1),
                ..Default::default()
            },
        );
        s.register(
            &SessionId::new("b"),
            SessionDetails {
                pid: Some(2),
                ..Default::default()
            },
        );
        s.remember_path(&SessionId::new("77"), "/x".into());
        s.remember_path(&SessionId::new("named"), "/y".into());
        let (client, _server) = UnixStream::pair().unwrap();
        s.register(
            &SessionId::new("c"),
            SessionDetails {
                pid: Some(3),
                ..Default::default()
            },
        );
        s.attach(&SessionId::new("c"), client, false);
        let alive = |pid: u32| pid == 1;
        assert_eq!(
            s.forget_gone(alive),
            2,
            "b (pid 2) and 77 (its id) are gone"
        );
        assert_eq!(s.count(), 3);
        assert_eq!(s.path_of(&SessionId::new("77")), None);
        assert_eq!(
            s.path_of(&SessionId::new("named")).as_deref(),
            Some("/y"),
            "no pid to check"
        );
        assert_eq!(
            s.count(),
            3,
            "c has a subscriber, a is alive, named is unknown"
        );
    }

    #[test]
    fn a_session_remembers_the_keys_its_shell_forwarded_last() {
        let s = sessions();
        let id = SessionId::new("42");
        assert!(s.env_of(&id).is_empty());
        s.remember_env(&id, BTreeMap::from([("K".to_string(), "old".to_string())]));
        s.remember_env(&id, BTreeMap::from([("K".to_string(), "new".to_string())]));
        assert_eq!(s.env_of(&id).get("K").map(String::as_str), Some("new"));
        assert!(s.env_of(&SessionId::new("43")).is_empty());
    }

    #[test]
    fn a_session_remembers_the_language_its_shell_named_last() {
        let s = sessions();
        let id = SessionId::new("42");
        assert_eq!(s.language_of(&id), None);
        s.remember_language(&id, Language::French);
        s.remember_language(&id, Language::German);
        assert_eq!(s.language_of(&id), Some(Language::German));
        s.register(&id, SessionDetails::default());
        assert_eq!(
            s.language_of(&id),
            Some(Language::German),
            "a registration that says nothing keeps what was known"
        );
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
