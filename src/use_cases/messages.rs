//! What the daemon sends to a shell later, as messages: the quick-fix
//! model's answer after a bubble no rule could fix, and the explanation
//! `kintsu why` asked for. One place for the rule that a model which had
//! nothing, or failed, says so in one line, so an "asking…" line under
//! the bubble never dangles.

use thiserror::Error;

use crate::entities::{CaseId, FailureCase, Message, MessageBody, SessionId, Settings};
use crate::use_cases::explain::{Explain, ExplainError};
use crate::use_cases::fix_last::{FixError, FixLast};
use crate::use_cases::focus::{Focus, FocusError};
use crate::use_cases::ports::{
    CaseStore, CaseStoreError, Clock, Environment, ModelError, ModelGateway, Notifier, NotifyError,
    Secrets, SessionRegistry,
};
use crate::use_cases::prompts::{parse_quick_fix, quick_fix_prompt};
use crate::use_cases::routing::{ask_first, model_candidates};

/// Why nothing was sent.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MessagesError {
    #[error(transparent)]
    Explain(#[from] ExplainError),
    #[error(transparent)]
    Fix(#[from] FixError),
    #[error("eager fixes are off for this model")]
    Disabled,
    #[error("no model is routed for quick fixes, or none may see this case")]
    NoModel,
    #[error("{0} is not running")]
    NotRunning(String),
    #[error("the model had no fix")]
    NoFix,
    #[error("no model answered ({})", .0.iter().map(|(n, e)| format!("{n}: {e}")).collect::<Vec<_>>().join("; "))]
    AllFailed(Vec<(String, ModelError)>),
    #[error(transparent)]
    Notify(#[from] NotifyError),
    #[error(transparent)]
    Cases(#[from] CaseStoreError),
    #[error(transparent)]
    Focus(#[from] FocusError),
}

/// Asks models in the background and delivers what they said.
pub struct Messages<'a> {
    pub settings: &'a Settings,
    pub clock: &'a dyn Clock,
    pub secrets: &'a dyn Secrets,
    pub models: &'a dyn ModelGateway,
    pub notifier: &'a dyn Notifier,
    pub cases: &'a dyn CaseStore,
    pub sessions: &'a dyn SessionRegistry,
}

impl Messages<'_> {
    /// The model that will be asked for a fix first, when one will be.
    pub fn fix_candidate(&self, case: &FailureCase) -> Option<String> {
        case.session()?;
        let candidates = model_candidates(self.settings, &self.settings.routing.quick_fix, case);
        let first = candidates.first()?;
        (self.settings.ui.eager_fix.allows(first) && self.models.is_reachable(first))
            .then(|| first.name.clone())
    }

    /// Runs when a case was offered without a rule fix. The model is slow
    /// and the user may have typed on: an answer that lands once the shell
    /// moved past the failure is still shown, but late, naming its command,
    /// so it never reads as being about the current one.
    pub fn fix(&self, case: &FailureCase) -> Result<Message, MessagesError> {
        let session = case
            .session()
            .ok_or_else(|| NotifyError::UnknownSession("none".into()))?;
        let candidates = model_candidates(self.settings, &self.settings.routing.quick_fix, case);
        if candidates.is_empty() {
            return Err(MessagesError::NoModel);
        }
        if !self.settings.ui.eager_fix.allows(candidates[0]) {
            return Err(MessagesError::Disabled);
        }
        if !self.models.is_reachable(candidates[0]) {
            return Err(MessagesError::NotRunning(candidates[0].name.clone()));
        }
        let first = candidates[0].name.clone();
        let answer = ask_first(
            self.models,
            self.secrets,
            &candidates,
            &quick_fix_prompt(case),
        );
        let in_focus = self.focus().holds(case)?;
        let (name, answer) = match answer {
            Ok(answered) => answered,
            Err(failures) => {
                let detail: Vec<String> =
                    failures.iter().map(|(n, e)| format!("{n}: {e}")).collect();
                let text = format!("{first} did not answer ({})", detail.join("; "));
                self.notifier.deliver(
                    session,
                    self.message(case, MessageBody::Note(text), in_focus),
                )?;
                return Err(MessagesError::AllFailed(failures));
            }
        };
        let Some(fix) = parse_quick_fix(&answer, &name, case.outcome().command()) else {
            let text = format!("{name} had no fix for this one.");
            self.notifier.deliver(
                session,
                self.message(case, MessageBody::Note(text), in_focus),
            )?;
            return Err(MessagesError::NoFix);
        };
        if self.cases.still_current(case)? {
            self.cases
                .save(&case.clone().with_proposal(Some(fix.clone())))?;
        }
        let message = self.message(case, MessageBody::Fix(fix), in_focus);
        self.notifier.deliver(session, message.clone())?;
        Ok(message)
    }

    /// The model `kintsu why` will ask first, or why it cannot.
    pub fn explain_candidate(&self, session: &SessionId) -> Result<String, ExplainError> {
        self.explain_use_case().candidate(Some(session))
    }

    /// Explains the session's last failure and sends the answer, or the
    /// reason there is none, as a message; late and labelled when the
    /// shell has moved on meanwhile.
    pub fn explain(&self, session: &SessionId) -> Result<Message, MessagesError> {
        let Some(case) = self.cases.last(Some(session))? else {
            return Err(ExplainError::NoCase.into());
        };
        let body = match self.explain_use_case().run(Some(session)) {
            Ok(explanation) => MessageBody::Explanation {
                model: explanation.model,
                text: explanation.text,
            },
            Err(e) => MessageBody::Note(e.to_string()),
        };
        let in_focus = self.focus().holds(&case)?;
        let message = self.message(&case, body, in_focus);
        self.notifier.deliver(session, message.clone())?;
        Ok(message)
    }

    /// A message about `case`, dated now, late when the shell has moved on.
    fn message(&self, case: &FailureCase, body: MessageBody, in_focus: bool) -> Message {
        let message = Message::new(case.id().clone(), self.clock.now(), body)
            .about(case.outcome().command().clone());
        if in_focus { message } else { message.late() }
    }

    fn focus(&self) -> Focus<'_> {
        Focus {
            sessions: self.sessions,
            cases: self.cases,
        }
    }

    fn explain_use_case(&self) -> Explain<'_> {
        Explain {
            settings: self.settings,
            cases: self.cases,
            secrets: self.secrets,
            models: self.models,
        }
    }

    /// One line about a case, sent to its shell.
    pub fn note(
        &self,
        session: &SessionId,
        case: &CaseId,
        text: String,
    ) -> Result<(), NotifyError> {
        self.notifier.deliver(
            session,
            Message::new(case.clone(), self.clock.now(), MessageBody::Note(text)),
        )
    }

    /// The fix for the session's last failure, sent as a message: what a
    /// click on the word does.
    pub fn fix_now(
        &self,
        session: &SessionId,
        environment: &dyn Environment,
    ) -> Result<Message, MessagesError> {
        let fix_last = FixLast {
            settings: self.settings,
            cases: self.cases,
            environment,
            secrets: self.secrets,
            models: self.models,
        };
        let proposal = fix_last.run(Some(session))?;
        let body = match proposal.fix {
            Some(fix) => MessageBody::Fix(fix),
            None => MessageBody::Note(format!(
                "no fix known for {}",
                proposal.case.outcome().command()
            )),
        };
        let message = Message::new(proposal.case.id().clone(), self.clock.now(), body);
        self.notifier.deliver(session, message.clone())?;
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{EagerFix, Provider, Routing, Session, SessionId, Tier, UiSettings};
    use crate::use_cases::testing::*;

    fn settings(eager: EagerFix, quick_fix: &[&str]) -> Settings {
        Settings {
            models: vec![
                spec("local", Provider::Ollama, Tier::Small),
                spec("cloud", Provider::Anthropic, Tier::Small),
            ],
            routing: Routing {
                quick_fix: quick_fix.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            },
            ui: UiSettings {
                eager_fix: eager,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn an_eager_fix_is_asked_and_delivered_to_the_shell() {
        let sessions = MemoryRegistry::default();
        let models = ScriptedModels::answering(&[("local", Ok("nvm use 22 && npm run build"))]);
        let notifier = MemoryNotifier::default();
        let cases = MemoryCases::default();
        let uc = Messages {
            settings: &settings(EagerFix::On, &["local"]),
            clock: &FakeClock::at(5),
            secrets: &MapSecrets::with(&[]),
            models: &models,
            notifier: &notifier,
            cases: &cases,
            sessions: &sessions,
        };
        let message = uc.fix(&case("npm run build", 1, Some("42"))).unwrap();
        assert!(!message.is_late(), "nothing ran since");
        assert!(
            cases
                .last(Some(&SessionId::new("42")))
                .unwrap()
                .unwrap()
                .proposal()
                .is_some(),
            "kintsu fix reuses it"
        );
        assert!(
            matches!(message.body(), MessageBody::Fix(f) if f.command().as_str() == "nvm use 22 && npm run build")
        );
        let delivered = notifier.delivered.borrow();
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].0.as_str(), "42");
        assert_eq!(delivered[0].1, message);
    }

    #[test]
    fn a_late_answer_names_its_command_and_is_not_saved_over_a_newer_failure() {
        let sessions = MemoryRegistry::default();
        use crate::entities::{CaseId, Timestamp};
        let models = ScriptedModels::answering(&[("local", Ok("make -j4"))]);
        let notifier = MemoryNotifier::default();
        let cases = MemoryCases::default();
        let old = case("make", 2, Some("42"));
        cases.save(&old).unwrap();
        let newer = FailureCase::new(
            CaseId::new("c2"),
            Timestamp::from_millis(1),
            outcome("ls nope", 1),
            None,
        )
        .with_session(Some(SessionId::new("42")));
        cases.save(&newer).unwrap();
        let uc = Messages {
            settings: &settings(EagerFix::On, &["local"]),
            clock: &FakeClock::at(5),
            secrets: &MapSecrets::with(&[]),
            models: &models,
            notifier: &notifier,
            cases: &cases,
            sessions: &sessions,
        };
        let message = uc.fix(&old).unwrap();
        assert!(message.is_late(), "the shell looks at another failure");
        assert_eq!(message.command().map(|c| c.as_str()), Some("make"));
        assert_eq!(notifier.delivered.borrow().len(), 1, "shown, labelled");
        let last = cases.last(Some(&SessionId::new("42"))).unwrap().unwrap();
        assert_eq!(last.id().as_str(), "c2");
        assert!(
            last.proposal().is_none(),
            "kintsu fix answers for the newer failure"
        );
    }

    #[test]
    fn a_late_answer_after_a_success_is_kept_for_kintsu_fix_and_shown_labelled() {
        let sessions = MemoryRegistry::default();
        sessions
            .save(&Session::with_recent(
                SessionId::new("42"),
                None,
                vec![outcome("make", 2), outcome("ls", 0)],
            ))
            .unwrap();
        let models = ScriptedModels::answering(&[("local", Ok("make -j4"))]);
        let notifier = MemoryNotifier::default();
        let cases = MemoryCases::default();
        let failure = case("make", 2, Some("42"));
        cases.save(&failure).unwrap();
        let uc = Messages {
            settings: &settings(EagerFix::On, &["local"]),
            clock: &FakeClock::at(5),
            secrets: &MapSecrets::with(&[]),
            models: &models,
            notifier: &notifier,
            cases: &cases,
            sessions: &sessions,
        };
        let message = uc.fix(&failure).unwrap();
        assert!(message.is_late());
        assert_eq!(notifier.delivered.borrow().len(), 1);
        let kept = cases.last(Some(&SessionId::new("42"))).unwrap().unwrap();
        assert_eq!(
            kept.proposal().map(|f| f.command().as_str()),
            Some("make -j4")
        );
    }

    #[test]
    fn nothing_is_sent_when_off_unrouted_declined_or_sensitive_without_local() {
        let sessions = MemoryRegistry::default();
        let notifier = MemoryNotifier::default();
        let secrets = MapSecrets::with(&[]);
        let clock = FakeClock::at(0);
        let declined = ScriptedModels::answering(&[("local", Ok("NONE"))]);
        let cases = MemoryCases::default();
        let off = Messages {
            settings: &settings(EagerFix::Off, &["local"]),
            clock: &clock,
            secrets: &secrets,
            models: &declined,
            notifier: &notifier,
            cases: &cases,
            sessions: &sessions,
        };
        assert_eq!(
            off.fix(&case("make", 2, Some("42"))).unwrap_err(),
            MessagesError::Disabled
        );
        let unrouted = Messages {
            settings: &settings(EagerFix::On, &[]),
            ..off
        };
        assert_eq!(
            unrouted.fix(&case("make", 2, Some("42"))).unwrap_err(),
            MessagesError::NoModel
        );
        let asked = Messages {
            settings: &settings(EagerFix::On, &["local"]),
            ..off
        };
        assert_eq!(
            asked.fix(&case("make", 2, Some("42"))).unwrap_err(),
            MessagesError::NoFix
        );
        let cloud_only = Messages {
            settings: &settings(EagerFix::On, &["cloud"]),
            ..off
        };
        let secret = case(
            "curl -H 'Authorization: Bearer sk-live-abcdefghijklmnop' https://x",
            22,
            Some("42"),
        );
        assert_eq!(cloud_only.fix(&secret).unwrap_err(), MessagesError::NoModel);
        let down = ScriptedModels::answering(&[(
            "local",
            Err(ModelError::MissingKey("$X is not set".into())),
        )]);
        let named = Messages {
            models: &down,
            ..asked
        };
        assert_eq!(
            named
                .fix(&case("make", 2, Some("42")))
                .unwrap_err()
                .to_string(),
            "no model answered (local: no key: $X is not set)"
        );
        let notes: Vec<String> = notifier
            .delivered
            .borrow()
            .iter()
            .map(|(_, m)| match m.body() {
                MessageBody::Note(t) => t.clone(),
                other => panic!("expected a note, got {other:?}"),
            })
            .collect();
        assert_eq!(
            notes,
            vec![
                "local had no fix for this one.",
                "local did not answer (local: no key: $X is not set)"
            ]
        );
        assert_eq!(off.fix_candidate(&case("make", 2, Some("42"))), None, "off");
        assert_eq!(
            asked.fix_candidate(&case("make", 2, Some("42"))).as_deref(),
            Some("local")
        );
        assert_eq!(
            cloud_only.fix_candidate(&secret),
            None,
            "sensitive, cloud only"
        );
        let auto_local = Messages {
            settings: &settings(EagerFix::Auto, &["local"]),
            ..off
        };
        assert_eq!(
            auto_local
                .fix_candidate(&case("make", 2, Some("42")))
                .as_deref(),
            Some("local"),
            "auto asks a local model"
        );
        let auto_cloud = Messages {
            settings: &settings(EagerFix::Auto, &["cloud", "local"]),
            ..off
        };
        assert_eq!(
            auto_cloud.fix_candidate(&case("make", 2, Some("42"))),
            None,
            "auto never asks a remote model on its own"
        );
        let mut stopped = ScriptedModels::answering(&[("local", Ok("x"))]);
        stopped.down.push("local".into());
        let not_running = Messages {
            settings: &settings(EagerFix::Auto, &["local"]),
            models: &stopped,
            ..off
        };
        assert_eq!(
            not_running.fix_candidate(&case("make", 2, Some("42"))),
            None,
            "a stopped server is not announced"
        );
        assert_eq!(
            not_running.fix(&case("make", 2, Some("42"))).unwrap_err(),
            MessagesError::NotRunning("local".into())
        );
        assert_eq!(
            auto_cloud.fix(&case("make", 2, Some("42"))).unwrap_err(),
            MessagesError::Disabled
        );
    }

    #[test]
    fn an_explanation_or_the_reason_there_is_none_is_sent_as_a_message() {
        let sessions = MemoryRegistry::default();
        let cases = MemoryCases::default();
        cases.save(&case("make", 2, Some("42"))).unwrap();
        let models = ScriptedModels::answering(&[
            ("cloud", Ok("Because.")),
            ("local", Err(ModelError::Unreachable("down".into()))),
        ]);
        let notifier = MemoryNotifier::default();
        let clock = FakeClock::at(7);
        let secrets = MapSecrets::with(&[]);
        let explain_via = |names: &[&str]| Settings {
            routing: Routing {
                explain: names.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            },
            ..settings(EagerFix::Off, &[])
        };
        let cloud = explain_via(&["cloud"]);
        let uc = Messages {
            settings: &cloud,
            clock: &clock,
            secrets: &secrets,
            models: &models,
            notifier: &notifier,
            cases: &cases,
            sessions: &sessions,
        };
        assert_eq!(
            uc.explain_candidate(&SessionId::new("42")).unwrap(),
            "cloud"
        );
        let m = uc.explain(&SessionId::new("42")).unwrap();
        assert_eq!(
            m.body(),
            &MessageBody::Explanation {
                model: "cloud".into(),
                text: "Because.".into()
            }
        );
        let local = explain_via(&["local"]);
        let down = Messages {
            settings: &local,
            ..uc
        };
        let m = down.explain(&SessionId::new("42")).unwrap();
        assert_eq!(
            m.body(),
            &MessageBody::Note("no model answered (local: unreachable: down)".into())
        );
        assert_eq!(notifier.delivered.borrow().len(), 2);
        assert_eq!(
            uc.explain_candidate(&SessionId::new("other")).unwrap_err(),
            ExplainError::NoCase
        );
        assert!(matches!(
            uc.explain(&SessionId::new("other")).unwrap_err(),
            MessagesError::Explain(ExplainError::NoCase)
        ));
    }
}
