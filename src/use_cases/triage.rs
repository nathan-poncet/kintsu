//! After every command line: remember it, and decide whether to say
//! anything. This is the quiet path; it reads the PATH only when it must
//! and never touches the network.

use thiserror::Error;

use crate::entities::{
    CommandLine, CommandOutcome, FailureCase, QuietReason, Session, SessionId, Settings, Shell,
    TerminalIdentity, TriageDecision, suggest_fix,
};
use crate::use_cases::facts::gather_facts;
use crate::use_cases::ports::{
    CaseStore, CaseStoreError, Clock, Environment, IdGenerator, IgnoreStore, IgnoreStoreError,
    RegistryError, SessionRegistry,
};

/// What the hook reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriageInput {
    /// The finished command line.
    pub outcome: CommandOutcome,
    /// Where it ran.
    pub cwd: Option<String>,
    /// Which shell session.
    pub session: Option<SessionId>,
    /// Which shell.
    pub shell: Option<Shell>,
    /// Which pane, for the output capture that follows an offer.
    pub terminal: TerminalIdentity,
}

/// Why triage could not finish.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TriageError {
    #[error(transparent)]
    Sessions(#[from] RegistryError),
    #[error(transparent)]
    Cases(#[from] CaseStoreError),
    #[error(transparent)]
    Ignores(#[from] IgnoreStoreError),
}

/// How many recent command lines a case carries as context.
const CONTEXT_LINES: usize = 10;

/// Triages one finished command line.
pub struct Triage<'a> {
    pub settings: &'a Settings,
    pub clock: &'a dyn Clock,
    pub ids: &'a dyn IdGenerator,
    pub sessions: &'a dyn SessionRegistry,
    pub cases: &'a dyn CaseStore,
    pub ignores: &'a dyn IgnoreStore,
    pub environment: &'a dyn Environment,
}

impl Triage<'_> {
    /// Records the outcome, then decides. Quiet reasons are checked from
    /// the cheapest to the dearest; the machine is read only for an offer.
    pub fn run(&self, input: TriageInput) -> Result<TriageDecision, TriageError> {
        let now = self.clock.now();
        let (previous, recent) = self.record(&input)?;
        let outcome = &input.outcome;
        let status = outcome.status();
        if status.is_success() {
            return Ok(TriageDecision::Quiet(QuietReason::Succeeded));
        }
        if status.is_interruption() {
            return Ok(TriageDecision::Quiet(QuietReason::Interrupted));
        }
        let quiet = &self.settings.quiet;
        let program = outcome.command().program();
        if quiet.accepts(program, status.code()) {
            return Ok(TriageDecision::Quiet(QuietReason::AcceptedStatus));
        }
        if quiet.never_triage.iter().any(|p| p == program) {
            return Ok(TriageDecision::Quiet(QuietReason::NeverTriaged(
                program.to_string(),
            )));
        }
        if quiet.same_failure_once
            && previous.is_some_and(|p| p.fingerprint() == outcome.fingerprint())
        {
            return Ok(TriageDecision::Quiet(QuietReason::Duplicate));
        }
        let cwd = input.cwd.as_deref();
        let silenced = quiet.is_off_in(cwd)
            || self
                .ignores
                .entries()?
                .iter()
                .any(|e| e.silences(outcome.command(), cwd, input.session.as_ref(), now));
        if silenced {
            return Ok(TriageDecision::Quiet(QuietReason::Ignored));
        }
        let case = FailureCase::new(self.ids.case_id(), now, outcome.clone(), input.cwd.clone())
            .with_session(input.session.clone())
            .with_recent(recent);
        let facts = gather_facts(self.environment, case.outcome(), case.cwd());
        let fix = suggest_fix(case.outcome(), &facts);
        let case = case.with_proposal(fix.clone());
        self.cases.save(&case)?;
        Ok(TriageDecision::Offer {
            case: Box::new(case),
            fix,
        })
    }

    /// Appends the outcome to its session; returns what ran just before
    /// and the last command lines, oldest first, for context.
    fn record(
        &self,
        input: &TriageInput,
    ) -> Result<(Option<CommandOutcome>, Vec<CommandLine>), TriageError> {
        let Some(id) = &input.session else {
            return Ok((None, Vec::new()));
        };
        let mut session = self
            .sessions
            .load(id)?
            .unwrap_or_else(|| Session::new(id.clone(), input.shell));
        let previous = session.recent().last().cloned();
        let recent: Vec<CommandLine> = session
            .recent()
            .iter()
            .rev()
            .take(CONTEXT_LINES)
            .map(|o| o.command().clone())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        session.remember(input.outcome.clone());
        self.sessions.save(&session)?;
        Ok((previous, recent))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{FixSource, IgnoreEntry, IgnoreScope, IgnoreTarget, Timestamp};
    use crate::use_cases::testing::*;

    struct World {
        settings: Settings,
        clock: FakeClock,
        ids: SequentialIds,
        sessions: MemoryRegistry,
        cases: MemoryCases,
        ignores: MemoryIgnores,
        environment: FakeEnvironment,
    }

    impl World {
        fn new() -> Self {
            Self {
                settings: Settings::default(),
                clock: FakeClock::at(1_000),
                ids: SequentialIds::default(),
                sessions: MemoryRegistry::default(),
                cases: MemoryCases::default(),
                ignores: MemoryIgnores::default(),
                environment: FakeEnvironment::with_executables(&["git", "make", "cargo"]),
            }
        }

        fn triage(&self) -> Triage<'_> {
            Triage {
                settings: &self.settings,
                clock: &self.clock,
                ids: &self.ids,
                sessions: &self.sessions,
                cases: &self.cases,
                ignores: &self.ignores,
                environment: &self.environment,
            }
        }

        fn run(&self, text: &str, code: i32) -> TriageDecision {
            self.triage()
                .run(TriageInput {
                    outcome: outcome(text, code),
                    cwd: Some("/w".into()),
                    session: Some(SessionId::new("42")),
                    shell: Some(Shell::Zsh),
                    terminal: TerminalIdentity::default(),
                })
                .unwrap()
        }
    }

    #[test]
    fn a_successful_command_stays_quiet_but_is_remembered() {
        let w = World::new();
        assert_eq!(
            w.run("ls", 0),
            TriageDecision::Quiet(QuietReason::Succeeded)
        );
        let session = w.sessions.load(&SessionId::new("42")).unwrap().unwrap();
        assert_eq!(session.recent().len(), 1);
        assert_eq!(session.shell(), Some(Shell::Zsh));
        assert!(w.cases.0.borrow().is_empty());
    }

    #[test]
    fn interruptions_and_accepted_statuses_stay_quiet() {
        let mut w = World::new();
        assert_eq!(
            w.run("sleep 10", 130),
            TriageDecision::Quiet(QuietReason::Interrupted)
        );
        assert_eq!(
            w.run("grep nothing file", 1),
            TriageDecision::Quiet(QuietReason::AcceptedStatus)
        );
        assert!(matches!(
            w.run("grep nothing missing", 2),
            TriageDecision::Offer { .. }
        ));
        w.settings.quiet.ok_statuses = vec![2];
        assert_eq!(
            w.run("grep nothing missing", 2),
            TriageDecision::Quiet(QuietReason::AcceptedStatus)
        );
    }

    #[test]
    fn editors_pagers_and_the_users_list_are_never_triaged() {
        let mut w = World::new();
        assert_eq!(
            w.run("vim x", 1),
            TriageDecision::Quiet(QuietReason::NeverTriaged("vim".into()))
        );
        w.settings.quiet.never_triage.push("make".into());
        assert_eq!(
            w.run("make test", 2),
            TriageDecision::Quiet(QuietReason::NeverTriaged("make".into()))
        );
    }

    #[test]
    fn a_failure_is_offered_with_its_context_and_saved() {
        let w = World::new();
        w.run("git pull", 0);
        w.run("npm ci", 0);
        let decision = w.run("make test", 2);
        let TriageDecision::Offer { case, fix } = decision else {
            panic!("expected an offer")
        };
        assert_eq!(case.id().as_str(), "case-1");
        assert_eq!(case.at(), Timestamp::from_millis(1_000));
        assert_eq!(case.cwd(), Some("/w"));
        assert_eq!(case.session(), Some(&SessionId::new("42")));
        assert_eq!(
            case.recent().iter().map(|c| c.as_str()).collect::<Vec<_>>(),
            vec!["git pull", "npm ci"]
        );
        assert!(fix.is_none());
        assert_eq!(
            w.cases.last(Some(&SessionId::new("42"))).unwrap().unwrap(),
            *case
        );
        assert_eq!(
            w.environment.path_reads.get(),
            0,
            "no PATH scan for a plain failure"
        );
    }

    #[test]
    fn a_typo_is_offered_with_its_fix_after_one_path_scan() {
        let w = World::new();
        let TriageDecision::Offer { fix, .. } = w.run("gti status", 127) else {
            panic!("expected an offer")
        };
        let fix = fix.unwrap();
        assert_eq!(fix.command().as_str(), "git status");
        assert_eq!(fix.source(), &FixSource::Rule("command typo".into()));
        assert_eq!(w.environment.path_reads.get(), 1);
    }

    #[test]
    fn the_same_failure_twice_in_a_row_is_offered_once() {
        let w = World::new();
        assert!(matches!(
            w.run("make test", 2),
            TriageDecision::Offer { .. }
        ));
        assert_eq!(
            w.run("make  test", 2),
            TriageDecision::Quiet(QuietReason::Duplicate)
        );
        assert_eq!(
            w.run("make test", 2),
            TriageDecision::Quiet(QuietReason::Duplicate)
        );
        assert!(
            matches!(w.run("make test", 1), TriageDecision::Offer { .. }),
            "another status is another failure"
        );
        w.run("ls", 0);
        assert!(
            matches!(w.run("make test", 1), TriageDecision::Offer { .. }),
            "not in a row any more"
        );
        assert_eq!(w.cases.0.borrow().len(), 3);
    }

    #[test]
    fn duplicates_can_be_allowed_by_settings() {
        let mut w = World::new();
        w.settings.quiet.same_failure_once = false;
        w.run("make test", 2);
        assert!(matches!(
            w.run("make test", 2),
            TriageDecision::Offer { .. }
        ));
    }

    #[test]
    fn ignore_entries_and_mutes_silence_a_failure() {
        let w = World::new();
        w.ignores.0.borrow_mut().push(IgnoreEntry::new(
            IgnoreTarget::Program("make".into()),
            IgnoreScope::Directory("/w".into()),
        ));
        assert_eq!(
            w.run("make test", 2),
            TriageDecision::Quiet(QuietReason::Ignored)
        );
        assert!(matches!(
            w.run("cargo test", 101),
            TriageDecision::Offer { .. }
        ));
        w.ignores
            .0
            .borrow_mut()
            .push(IgnoreEntry::mute_until(Timestamp::from_millis(5_000)));
        assert_eq!(
            w.run("cargo build", 101),
            TriageDecision::Quiet(QuietReason::Ignored)
        );
        w.clock.0.set(Timestamp::from_millis(5_000));
        assert!(matches!(
            w.run("cargo run", 101),
            TriageDecision::Offer { .. }
        ));
    }

    #[test]
    fn nothing_is_shown_in_a_directory_switched_off() {
        let mut w = World::new();
        w.settings.quiet.off_in = vec!["/w".into()];
        assert_eq!(
            w.run("make test", 2),
            TriageDecision::Quiet(QuietReason::Ignored)
        );
    }

    #[test]
    fn without_a_session_nothing_is_remembered_but_failures_are_still_offered() {
        let w = World::new();
        let decision = w
            .triage()
            .run(TriageInput {
                outcome: outcome("make", 2),
                cwd: None,
                session: None,
                shell: None,
                terminal: TerminalIdentity::default(),
            })
            .unwrap();
        let TriageDecision::Offer { case, .. } = decision else {
            panic!("expected an offer")
        };
        assert!(case.recent().is_empty());
        assert!(w.sessions.0.borrow().is_empty());
        assert_eq!(w.cases.last(None).unwrap().unwrap(), *case);
    }

    #[test]
    fn a_case_carries_at_most_ten_recent_lines() {
        let w = World::new();
        for i in 0..15 {
            w.run(&format!("echo {i}"), 0);
        }
        let TriageDecision::Offer { case, .. } = w.run("make", 2) else {
            panic!("expected an offer")
        };
        assert_eq!(case.recent().len(), 10);
        assert_eq!(case.recent()[0].as_str(), "echo 5");
        assert_eq!(case.recent()[9].as_str(), "echo 14");
    }
}
