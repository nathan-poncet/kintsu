//! One interactive shell, and what it ran recently.

use crate::entities::{CommandOutcome, Shell};

/// The identity of one interactive shell, chosen by its hook.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    /// Wraps the token the hook sends.
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    /// The token.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A shell session: which shell, and its last command lines, newest last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    id: SessionId,
    shell: Option<Shell>,
    recent: Vec<CommandOutcome>,
}

impl Session {
    /// How many recent outcomes a session keeps.
    pub const RECENT_CAPACITY: usize = 20;

    /// A session that has run nothing yet.
    pub fn new(id: SessionId, shell: Option<Shell>) -> Self {
        Self {
            id,
            shell,
            recent: Vec::new(),
        }
    }

    /// Restores a session with its known history, oldest first.
    pub fn with_recent(id: SessionId, shell: Option<Shell>, recent: Vec<CommandOutcome>) -> Self {
        let mut session = Self::new(id, shell);
        for outcome in recent {
            session.remember(outcome);
        }
        session
    }

    /// The identity.
    pub fn id(&self) -> &SessionId {
        &self.id
    }

    /// The shell, when the hook said which.
    pub fn shell(&self) -> Option<Shell> {
        self.shell
    }

    /// The last outcomes, oldest first, at most `RECENT_CAPACITY`.
    pub fn recent(&self) -> &[CommandOutcome] {
        &self.recent
    }

    /// Appends an outcome, forgetting the oldest past the capacity.
    pub fn remember(&mut self, outcome: CommandOutcome) {
        self.recent.push(outcome);
        if self.recent.len() > Self::RECENT_CAPACITY {
            self.recent.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{CommandLine, ExitStatus};

    fn outcome(text: &str) -> CommandOutcome {
        CommandOutcome::new(CommandLine::new(text).unwrap(), ExitStatus::new(0))
    }

    #[test]
    fn a_session_keeps_its_last_twenty_outcomes_oldest_first() {
        let mut s = Session::new(SessionId::new("42"), Some(Shell::Zsh));
        for i in 0..25 {
            s.remember(outcome(&format!("cmd{i}")));
        }
        assert_eq!(s.recent().len(), Session::RECENT_CAPACITY);
        assert_eq!(s.recent()[0].command().as_str(), "cmd5");
        assert_eq!(s.recent()[19].command().as_str(), "cmd24");
    }
}
