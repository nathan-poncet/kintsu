//! After the bubble, when no rule knew: ask the quick-fix model in the
//! background and send the answer to the shell as a message. Only when
//! the user opted in (`ui.eager_fix`) and a model is routed for it.

use thiserror::Error;

use crate::entities::{FailureCase, Message, MessageBody, Settings};
use crate::use_cases::ports::{
    CaseStore, CaseStoreError, Clock, ModelError, ModelGateway, Notifier, NotifyError, Secrets,
};
use crate::use_cases::prompts::{parse_quick_fix, quick_fix_prompt};
use crate::use_cases::routing::{ask_first, model_candidates};

/// Why nothing was sent.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FollowUpError {
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
}

/// Asks the quick-fix model about an offered case and delivers the answer.
pub struct FollowUp<'a> {
    pub settings: &'a Settings,
    pub clock: &'a dyn Clock,
    pub secrets: &'a dyn Secrets,
    pub models: &'a dyn ModelGateway,
    pub notifier: &'a dyn Notifier,
    pub cases: &'a dyn CaseStore,
}

impl FollowUp<'_> {
    /// The model that will be asked first, when one will be.
    pub fn candidate(&self, case: &FailureCase) -> Option<String> {
        case.session()?;
        let candidates = model_candidates(self.settings, &self.settings.routing.quick_fix, case);
        let first = candidates.first()?;
        (self.settings.ui.eager_fix.allows(first) && self.models.is_reachable(first))
            .then(|| first.name.clone())
    }

    /// Runs when a case was offered without a rule fix. A model that had
    /// nothing, or failed, is reported to the shell in one line, so the
    /// "asking…" line under the bubble never dangles.
    pub fn run(&self, case: &FailureCase) -> Result<Message, FollowUpError> {
        let session = case
            .session()
            .ok_or_else(|| NotifyError::UnknownSession("none".into()))?;
        let candidates = model_candidates(self.settings, &self.settings.routing.quick_fix, case);
        if candidates.is_empty() {
            return Err(FollowUpError::NoModel);
        }
        if !self.settings.ui.eager_fix.allows(candidates[0]) {
            return Err(FollowUpError::Disabled);
        }
        if !self.models.is_reachable(candidates[0]) {
            return Err(FollowUpError::NotRunning(candidates[0].name.clone()));
        }
        let first = candidates[0].name.clone();
        let (name, answer) = match ask_first(
            self.models,
            self.secrets,
            &candidates,
            &quick_fix_prompt(case),
        ) {
            Ok(answered) => answered,
            Err(failures) => {
                let detail: Vec<String> =
                    failures.iter().map(|(n, e)| format!("{n}: {e}")).collect();
                self.note(
                    session,
                    case,
                    format!("{first} did not answer ({})", detail.join("; ")),
                )?;
                return Err(FollowUpError::AllFailed(failures));
            }
        };
        let Some(fix) = parse_quick_fix(&answer, &name) else {
            self.note(session, case, format!("{name} had no fix for this one."))?;
            return Err(FollowUpError::NoFix);
        };
        self.cases
            .save(&case.clone().with_proposal(Some(fix.clone())))?;
        let message = Message::new(case.id().clone(), self.clock.now(), MessageBody::Fix(fix));
        self.notifier.deliver(session, message.clone())?;
        Ok(message)
    }

    fn note(
        &self,
        session: &crate::entities::SessionId,
        case: &FailureCase,
        text: String,
    ) -> Result<(), NotifyError> {
        self.notifier.deliver(
            session,
            Message::new(case.id().clone(), self.clock.now(), MessageBody::Note(text)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{EagerFix, Provider, Routing, SessionId, Tier, UiSettings};
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
        let models = ScriptedModels::answering(&[("local", Ok("nvm use 22 && npm run build"))]);
        let notifier = MemoryNotifier::default();
        let cases = MemoryCases::default();
        let uc = FollowUp {
            settings: &settings(EagerFix::On, &["local"]),
            clock: &FakeClock::at(5),
            secrets: &MapSecrets::with(&[]),
            models: &models,
            notifier: &notifier,
            cases: &cases,
        };
        let message = uc.run(&case("npm run build", 1, Some("42"))).unwrap();
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
    fn nothing_is_sent_when_off_unrouted_declined_or_sensitive_without_local() {
        let notifier = MemoryNotifier::default();
        let secrets = MapSecrets::with(&[]);
        let clock = FakeClock::at(0);
        let declined = ScriptedModels::answering(&[("local", Ok("NONE"))]);
        let cases = MemoryCases::default();
        let off = FollowUp {
            settings: &settings(EagerFix::Off, &["local"]),
            clock: &clock,
            secrets: &secrets,
            models: &declined,
            notifier: &notifier,
            cases: &cases,
        };
        assert_eq!(
            off.run(&case("make", 2, Some("42"))).unwrap_err(),
            FollowUpError::Disabled
        );
        let unrouted = FollowUp {
            settings: &settings(EagerFix::On, &[]),
            ..off
        };
        assert_eq!(
            unrouted.run(&case("make", 2, Some("42"))).unwrap_err(),
            FollowUpError::NoModel
        );
        let asked = FollowUp {
            settings: &settings(EagerFix::On, &["local"]),
            ..off
        };
        assert_eq!(
            asked.run(&case("make", 2, Some("42"))).unwrap_err(),
            FollowUpError::NoFix
        );
        let cloud_only = FollowUp {
            settings: &settings(EagerFix::On, &["cloud"]),
            ..off
        };
        let secret = case(
            "curl -H 'Authorization: Bearer sk-live-abcdefghijklmnop' https://x",
            22,
            Some("42"),
        );
        assert_eq!(cloud_only.run(&secret).unwrap_err(), FollowUpError::NoModel);
        let down = ScriptedModels::answering(&[(
            "local",
            Err(ModelError::MissingKey("$X is not set".into())),
        )]);
        let named = FollowUp {
            models: &down,
            ..asked
        };
        assert_eq!(
            named
                .run(&case("make", 2, Some("42")))
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
        assert_eq!(off.candidate(&case("make", 2, Some("42"))), None, "off");
        assert_eq!(
            asked.candidate(&case("make", 2, Some("42"))).as_deref(),
            Some("local")
        );
        assert_eq!(cloud_only.candidate(&secret), None, "sensitive, cloud only");
        let auto_local = FollowUp {
            settings: &settings(EagerFix::Auto, &["local"]),
            ..off
        };
        assert_eq!(
            auto_local
                .candidate(&case("make", 2, Some("42")))
                .as_deref(),
            Some("local"),
            "auto asks a local model"
        );
        let auto_cloud = FollowUp {
            settings: &settings(EagerFix::Auto, &["cloud", "local"]),
            ..off
        };
        assert_eq!(
            auto_cloud.candidate(&case("make", 2, Some("42"))),
            None,
            "auto never asks a remote model on its own"
        );
        let mut stopped = ScriptedModels::answering(&[("local", Ok("x"))]);
        stopped.down.push("local".into());
        let not_running = FollowUp {
            settings: &settings(EagerFix::Auto, &["local"]),
            models: &stopped,
            ..off
        };
        assert_eq!(
            not_running.candidate(&case("make", 2, Some("42"))),
            None,
            "a stopped server is not announced"
        );
        assert_eq!(
            not_running.run(&case("make", 2, Some("42"))).unwrap_err(),
            FollowUpError::NotRunning("local".into())
        );
        assert_eq!(
            auto_cloud.run(&case("make", 2, Some("42"))).unwrap_err(),
            FollowUpError::Disabled
        );
    }
}
