//! `kintsu ignore` and `kintsu mute`: silence, scoped and remembered.

use thiserror::Error;

use crate::entities::{Duration, IgnoreEntry, IgnoreScope, IgnoreTarget, SessionId};
use crate::use_cases::ports::{CaseStore, CaseStoreError, Clock, IgnoreStore, IgnoreStoreError};

/// Which silence the user asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeChoice {
    /// This exact command line, everywhere.
    Command,
    /// This program, in this directory.
    Directory(String),
    /// This program, in this shell session.
    Session,
    /// This program, everywhere, forever.
    Always,
}

/// What the user asked to silence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IgnoreRequest {
    /// The last failure of this shell, or a named program.
    Last {
        program: Option<String>,
        scope: ScopeChoice,
    },
    /// Everything, for a while.
    Mute(Duration),
}

/// Why nothing was silenced.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IgnoreError {
    #[error("nothing failed in this shell yet; name the program: kintsu ignore <program>")]
    NoFailure,
    #[error("this shell has no kintsu session; is the hook installed?")]
    NoSession,
    #[error(transparent)]
    Cases(#[from] CaseStoreError),
    #[error(transparent)]
    Ignores(#[from] IgnoreStoreError),
}

/// Adds to the ignore list.
pub struct Ignore<'a> {
    pub clock: &'a dyn Clock,
    pub cases: &'a dyn CaseStore,
    pub ignores: &'a dyn IgnoreStore,
}

impl Ignore<'_> {
    /// Records the entry, dropping expired ones on the way.
    pub fn run(
        &self,
        session: Option<&SessionId>,
        request: IgnoreRequest,
    ) -> Result<IgnoreEntry, IgnoreError> {
        let now = self.clock.now();
        let entry = match request {
            IgnoreRequest::Mute(for_how_long) => IgnoreEntry::mute_until(now.plus(for_how_long)),
            IgnoreRequest::Last { program, scope } => {
                let last = self.cases.last(session)?;
                let target = match (&scope, program) {
                    (ScopeChoice::Command, None) => {
                        IgnoreTarget::Command(IgnoreEntry::command_key(
                            last.as_ref()
                                .ok_or(IgnoreError::NoFailure)?
                                .outcome()
                                .command(),
                        ))
                    }
                    (_, Some(p)) => IgnoreTarget::Program(p),
                    (_, None) => IgnoreTarget::Program(
                        last.as_ref()
                            .ok_or(IgnoreError::NoFailure)?
                            .outcome()
                            .command()
                            .program()
                            .to_string(),
                    ),
                };
                let scope = match scope {
                    ScopeChoice::Command | ScopeChoice::Always => IgnoreScope::Everywhere,
                    ScopeChoice::Directory(dir) => IgnoreScope::Directory(dir),
                    ScopeChoice::Session => {
                        IgnoreScope::Session(session.cloned().ok_or(IgnoreError::NoSession)?)
                    }
                };
                IgnoreEntry::new(target, scope)
            }
        };
        let mut entries: Vec<IgnoreEntry> = self
            .ignores
            .entries()?
            .into_iter()
            .filter(|e| !e.is_expired(now))
            .collect();
        if !entries.contains(&entry) {
            entries.push(entry.clone());
        }
        self.ignores.replace(&entries)?;
        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::Timestamp;
    use crate::use_cases::testing::*;

    fn world() -> (MemoryCases, MemoryIgnores, FakeClock) {
        let cases = MemoryCases::default();
        cases.save(&case("make  test", 2, Some("42"))).unwrap();
        (cases, MemoryIgnores::default(), FakeClock::at(1_000))
    }

    fn last(program: Option<&str>, scope: ScopeChoice) -> IgnoreRequest {
        IgnoreRequest::Last {
            program: program.map(String::from),
            scope,
        }
    }

    #[test]
    fn the_default_ignores_the_exact_last_command_line_everywhere() {
        let (cases, ignores, clock) = world();
        let uc = Ignore {
            clock: &clock,
            cases: &cases,
            ignores: &ignores,
        };
        let entry = uc
            .run(
                Some(&SessionId::new("42")),
                last(None, ScopeChoice::Command),
            )
            .unwrap();
        assert_eq!(
            entry,
            IgnoreEntry::new(
                IgnoreTarget::Command("make test".into()),
                IgnoreScope::Everywhere
            )
        );
        uc.run(
            Some(&SessionId::new("42")),
            last(None, ScopeChoice::Command),
        )
        .unwrap();
        assert_eq!(ignores.entries().unwrap().len(), 1, "no duplicates");
    }

    #[test]
    fn the_other_scopes_are_about_the_program() {
        let (cases, ignores, clock) = world();
        let uc = Ignore {
            clock: &clock,
            cases: &cases,
            ignores: &ignores,
        };
        let id = SessionId::new("42");
        let dir = uc
            .run(Some(&id), last(None, ScopeChoice::Directory("/w".into())))
            .unwrap();
        assert_eq!(
            dir,
            IgnoreEntry::new(
                IgnoreTarget::Program("make".into()),
                IgnoreScope::Directory("/w".into())
            )
        );
        let session = uc.run(Some(&id), last(None, ScopeChoice::Session)).unwrap();
        assert_eq!(session.scope(), &IgnoreScope::Session(id.clone()));
        let named = uc
            .run(Some(&id), last(Some("cargo"), ScopeChoice::Always))
            .unwrap();
        assert_eq!(
            named,
            IgnoreEntry::new(
                IgnoreTarget::Program("cargo".into()),
                IgnoreScope::Everywhere
            )
        );
        assert_eq!(ignores.entries().unwrap().len(), 3);
    }

    #[test]
    fn a_mute_expires_and_expired_entries_are_pruned() {
        let (cases, ignores, clock) = world();
        let uc = Ignore {
            clock: &clock,
            cases: &cases,
            ignores: &ignores,
        };
        let mute = uc
            .run(None, IgnoreRequest::Mute(Duration::from_mins(60)))
            .unwrap();
        assert_eq!(
            mute,
            IgnoreEntry::mute_until(Timestamp::from_millis(1_000 + 3_600_000))
        );
        clock.0.set(Timestamp::from_millis(10_000_000));
        uc.run(None, last(Some("make"), ScopeChoice::Always))
            .unwrap();
        assert_eq!(
            ignores.entries().unwrap(),
            vec![IgnoreEntry::new(
                IgnoreTarget::Program("make".into()),
                IgnoreScope::Everywhere
            )]
        );
    }

    #[test]
    fn a_session_scope_needs_a_session_and_the_default_needs_a_failure() {
        let cases = MemoryCases::default();
        let ignores = MemoryIgnores::default();
        let uc = Ignore {
            clock: &FakeClock::at(0),
            cases: &cases,
            ignores: &ignores,
        };
        assert_eq!(
            uc.run(None, last(Some("make"), ScopeChoice::Session))
                .unwrap_err(),
            IgnoreError::NoSession
        );
        assert_eq!(
            uc.run(Some(&SessionId::new("42")), last(None, ScopeChoice::Always))
                .unwrap_err(),
            IgnoreError::NoFailure
        );
        assert_eq!(
            uc.run(
                Some(&SessionId::new("42")),
                last(None, ScopeChoice::Command)
            )
            .unwrap_err(),
            IgnoreError::NoFailure
        );
        assert!(ignores.entries().unwrap().is_empty());
    }
}
