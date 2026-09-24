//! Silence, with a target, a scope and sometimes an end.

use crate::entities::{CommandLine, SessionId, Timestamp};

/// What an ignore entry is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IgnoreTarget {
    /// A program, whatever its arguments.
    Program(String),
    /// One exact command line, spacing aside.
    Command(String),
    /// Every command: a mute.
    Everything,
}

/// Where an ignore applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IgnoreScope {
    /// Everywhere, forever.
    Everywhere,
    /// In this directory and under it.
    Directory(String),
    /// In this shell session.
    Session(SessionId),
    /// Until an instant: `kintsu mute 1h`.
    Until(Timestamp),
}

/// One entry of the ignore list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoreEntry {
    target: IgnoreTarget,
    scope: IgnoreScope,
}

impl IgnoreEntry {
    pub fn new(target: IgnoreTarget, scope: IgnoreScope) -> Self {
        Self { target, scope }
    }

    /// `kintsu mute`: everything, until then.
    pub fn mute_until(until: Timestamp) -> Self {
        Self::new(IgnoreTarget::Everything, IgnoreScope::Until(until))
    }

    /// A command line normalised for comparison: its words, single spaced.
    pub fn command_key(command: &CommandLine) -> String {
        command.words().join(" ")
    }

    pub fn target(&self) -> &IgnoreTarget {
        &self.target
    }

    pub fn scope(&self) -> &IgnoreScope {
        &self.scope
    }

    /// Whether this entry silences `command` run in `cwd` by `session` at `now`.
    pub fn silences(
        &self,
        command: &CommandLine,
        cwd: Option<&str>,
        session: Option<&SessionId>,
        now: Timestamp,
    ) -> bool {
        let target_matches = match &self.target {
            IgnoreTarget::Everything => true,
            IgnoreTarget::Program(p) => p == command.program(),
            IgnoreTarget::Command(c) => *c == Self::command_key(command),
        };
        let scope_matches = match &self.scope {
            IgnoreScope::Everywhere => true,
            IgnoreScope::Directory(dir) => {
                cwd.is_some_and(|c| c == dir || c.starts_with(&format!("{dir}/")))
            }
            IgnoreScope::Session(id) => session == Some(id),
            IgnoreScope::Until(until) => now < *until,
        };
        target_matches && scope_matches
    }

    /// Whether the entry can never apply again and may be dropped.
    pub fn is_expired(&self, now: Timestamp) -> bool {
        matches!(&self.scope, IgnoreScope::Until(until) if now >= *until)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Timestamp {
        Timestamp::from_millis(10_000)
    }

    fn cmd(text: &str) -> CommandLine {
        CommandLine::new(text).unwrap()
    }

    #[test]
    fn a_program_everywhere_silences_it_whatever_the_arguments() {
        let e = IgnoreEntry::new(
            IgnoreTarget::Program("make".into()),
            IgnoreScope::Everywhere,
        );
        assert!(e.silences(&cmd("make test"), Some("/a"), None, now()));
        assert!(e.silences(&cmd("make"), None, None, now()));
        assert!(!e.silences(&cmd("cargo make"), Some("/a"), None, now()));
    }

    #[test]
    fn an_exact_command_line_is_matched_on_its_words() {
        let e = IgnoreEntry::new(
            IgnoreTarget::Command("make test".into()),
            IgnoreScope::Everywhere,
        );
        assert!(e.silences(&cmd("make   test"), None, None, now()));
        assert!(!e.silences(&cmd("make"), None, None, now()));
        assert_eq!(
            IgnoreEntry::command_key(&cmd("  make   test ")),
            "make test"
        );
    }

    #[test]
    fn a_directory_scope_covers_the_directory_and_what_is_under_it() {
        let e = IgnoreEntry::new(
            IgnoreTarget::Program("make".into()),
            IgnoreScope::Directory("/home/me/acme".into()),
        );
        assert!(e.silences(&cmd("make"), Some("/home/me/acme"), None, now()));
        assert!(e.silences(&cmd("make"), Some("/home/me/acme/api"), None, now()));
        assert!(!e.silences(&cmd("make"), Some("/home/me/acme-old"), None, now()));
        assert!(!e.silences(&cmd("make"), None, None, now()));
    }

    #[test]
    fn a_session_scope_covers_that_shell_only() {
        let e = IgnoreEntry::new(
            IgnoreTarget::Program("make".into()),
            IgnoreScope::Session(SessionId::new("42")),
        );
        assert!(e.silences(&cmd("make"), None, Some(&SessionId::new("42")), now()));
        assert!(!e.silences(&cmd("make"), None, Some(&SessionId::new("43")), now()));
    }

    #[test]
    fn a_mute_silences_everything_until_it_expires() {
        let e = IgnoreEntry::mute_until(Timestamp::from_millis(20_000));
        assert!(e.silences(&cmd("anything at all"), None, None, now()));
        assert!(!e.silences(&cmd("anything"), None, None, Timestamp::from_millis(20_000)));
        assert!(e.is_expired(Timestamp::from_millis(20_000)));
        assert!(!e.is_expired(now()));
    }
}
