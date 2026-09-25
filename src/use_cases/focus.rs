//! What the shell is still looking at: its last failure, while it is also
//! its last command. Once something else ran, the failure is over: the
//! panel has nothing to expand and a late answer nobody to tell.

use thiserror::Error;

use crate::entities::{CommandOutcome, FailureCase, Message, SessionId};
use crate::use_cases::ports::{CaseStore, CaseStoreError, RegistryError, SessionRegistry};

/// Why the case could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FocusError {
    #[error(transparent)]
    Sessions(#[from] RegistryError),
    #[error(transparent)]
    Cases(#[from] CaseStoreError),
}

/// Finds the failure the shell still has under its eyes.
pub struct Focus<'a> {
    pub sessions: &'a dyn SessionRegistry,
    pub cases: &'a dyn CaseStore,
}

impl Focus<'_> {
    /// The session's last failure, only while it is the last command the
    /// session ran; kintsu's own commands are never recorded, so they do
    /// not end it. Without a session, the last failure overall.
    pub fn case(&self, session: Option<&SessionId>) -> Result<Option<FailureCase>, FocusError> {
        let Some(case) = self.cases.last(session)? else {
            return Ok(None);
        };
        let Some(id) = session else {
            return Ok(Some(case));
        };
        let moved_on = self.moved_on(id, case.outcome())?;
        Ok((!moved_on).then_some(case))
    }

    /// Whether `case` is still what its shell looks at: no newer failure
    /// replaced it and no other command ran since.
    pub fn holds(&self, case: &FailureCase) -> Result<bool, FocusError> {
        if !self.cases.still_current(case)? {
            return Ok(false);
        }
        match case.session() {
            Some(id) => Ok(!self.moved_on(id, case.outcome())?),
            None => Ok(true),
        }
    }

    /// Messages that waited for a prompt: those about a failure the shell
    /// no longer looks at are marked late, so they name their command when
    /// shown.
    pub fn mark_late(
        &self,
        session: &SessionId,
        messages: Vec<Message>,
    ) -> Result<Vec<Message>, FocusError> {
        let watched = self.case(Some(session))?;
        Ok(messages
            .into_iter()
            .map(|m| {
                if watched.as_ref().is_some_and(|c| c.id() == m.case()) {
                    m
                } else {
                    m.late()
                }
            })
            .collect())
    }

    /// Whether the session's last recorded command is another one.
    fn moved_on(&self, id: &SessionId, outcome: &CommandOutcome) -> Result<bool, FocusError> {
        Ok(self
            .sessions
            .load(id)?
            .and_then(|session| {
                session
                    .recent()
                    .last()
                    .map(|last| last.fingerprint() != outcome.fingerprint())
            })
            .unwrap_or(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::Session;
    use crate::use_cases::testing::*;

    fn world(recent: &[(&str, i32)]) -> (MemoryRegistry, MemoryCases) {
        let sessions = MemoryRegistry::default();
        let outcomes = recent
            .iter()
            .map(|(text, code)| outcome(text, *code))
            .collect();
        sessions
            .save(&Session::with_recent(SessionId::new("42"), None, outcomes))
            .unwrap();
        let cases = MemoryCases::default();
        cases.save(&case("make test", 2, Some("42"))).unwrap();
        (sessions, cases)
    }

    fn expanded(
        sessions: &MemoryRegistry,
        cases: &MemoryCases,
        session: Option<&str>,
    ) -> Option<String> {
        let uc = Focus { sessions, cases };
        uc.case(session.map(SessionId::new).as_ref())
            .unwrap()
            .map(|c| c.outcome().command().as_str().to_string())
    }

    #[test]
    fn the_failure_just_before_is_expanded() {
        let (sessions, cases) = world(&[("ls", 0), ("make test", 2)]);
        assert_eq!(
            expanded(&sessions, &cases, Some("42")),
            Some("make test".into())
        );
    }

    #[test]
    fn after_a_success_there_is_nothing_to_expand() {
        let (sessions, cases) = world(&[("make test", 2), ("ls", 0)]);
        assert_eq!(expanded(&sessions, &cases, Some("42")), None);
    }

    #[test]
    fn the_same_failure_repeated_is_still_the_one_to_expand() {
        let (sessions, cases) = world(&[("make test", 2), ("make  test", 2)]);
        assert_eq!(
            expanded(&sessions, &cases, Some("42")),
            Some("make test".into())
        );
    }

    #[test]
    fn without_a_session_the_last_failure_overall_is_expanded() {
        let (sessions, cases) = world(&[("make test", 2), ("ls", 0)]);
        assert_eq!(expanded(&sessions, &cases, None), Some("make test".into()));
    }

    #[test]
    fn a_case_is_held_until_another_command_ran() {
        let (sessions, cases) = world(&[("make test", 2)]);
        let uc = Focus {
            sessions: &sessions,
            cases: &cases,
        };
        let held = cases.last(Some(&SessionId::new("42"))).unwrap().unwrap();
        assert!(uc.holds(&held).unwrap());
        sessions
            .save(&Session::with_recent(
                SessionId::new("42"),
                None,
                vec![outcome("make test", 2), outcome("ls", 0)],
            ))
            .unwrap();
        assert!(!uc.holds(&held).unwrap());
    }

    #[test]
    fn messages_about_a_failure_no_longer_watched_are_marked_late() {
        use crate::entities::{CaseId, MessageBody, Timestamp};
        let (sessions, cases) = world(&[("make test", 2)]);
        let uc = Focus {
            sessions: &sessions,
            cases: &cases,
        };
        let note = |id: &str| {
            Message::new(
                CaseId::new(id),
                Timestamp::from_millis(0),
                MessageBody::Note("late".into()),
            )
        };
        let session = SessionId::new("42");
        let marked = uc
            .mark_late(&session, vec![note("c"), note("older")])
            .unwrap();
        assert_eq!(
            marked.iter().map(Message::is_late).collect::<Vec<_>>(),
            [false, true]
        );
        sessions
            .save(&Session::with_recent(
                session.clone(),
                None,
                vec![outcome("make test", 2), outcome("ls", 0)],
            ))
            .unwrap();
        assert!(uc.mark_late(&session, vec![note("c")]).unwrap()[0].is_late());
    }

    #[test]
    fn a_shell_that_never_failed_has_nothing_to_expand() {
        let (sessions, _) = world(&[("ls", 0)]);
        assert_eq!(
            expanded(&sessions, &MemoryCases::default(), Some("42")),
            None
        );
    }
}
