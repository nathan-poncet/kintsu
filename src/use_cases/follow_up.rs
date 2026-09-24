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
    #[error("eager fixes are off")]
    Disabled,
    #[error("no model is routed for quick fixes, or none may see this case")]
    NoModel,
    #[error("the model had no fix")]
    NoFix,
    #[error("no model answered")]
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
    /// Runs when a case was offered without a rule fix.
    pub fn run(&self, case: &FailureCase) -> Result<Message, FollowUpError> {
        if !self.settings.ui.eager_fix {
            return Err(FollowUpError::Disabled);
        }
        let session = case
            .session()
            .ok_or_else(|| NotifyError::UnknownSession("none".into()))?;
        let candidates = model_candidates(self.settings, &self.settings.routing.quick_fix, case);
        if candidates.is_empty() {
            return Err(FollowUpError::NoModel);
        }
        let (name, answer) = ask_first(
            self.models,
            self.secrets,
            &candidates,
            &quick_fix_prompt(case),
        )
        .map_err(FollowUpError::AllFailed)?;
        let fix = parse_quick_fix(&answer, &name).ok_or(FollowUpError::NoFix)?;
        self.cases
            .save(&case.clone().with_proposal(Some(fix.clone())))?;
        let message = Message::new(case.id().clone(), self.clock.now(), MessageBody::Fix(fix));
        self.notifier.deliver(session, message.clone())?;
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{Provider, Routing, SessionId, Tier, UiSettings};
    use crate::use_cases::testing::*;

    fn settings(eager: bool, quick_fix: &[&str]) -> Settings {
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
            settings: &settings(true, &["local"]),
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
            settings: &settings(false, &["local"]),
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
            settings: &settings(true, &[]),
            ..off
        };
        assert_eq!(
            unrouted.run(&case("make", 2, Some("42"))).unwrap_err(),
            FollowUpError::NoModel
        );
        let asked = FollowUp {
            settings: &settings(true, &["local"]),
            ..off
        };
        assert_eq!(
            asked.run(&case("make", 2, Some("42"))).unwrap_err(),
            FollowUpError::NoFix
        );
        let cloud_only = FollowUp {
            settings: &settings(true, &["cloud"]),
            ..off
        };
        let secret = case(
            "curl -H 'Authorization: Bearer sk-live-abcdefghijklmnop' https://x",
            22,
            Some("42"),
        );
        assert_eq!(cloud_only.run(&secret).unwrap_err(), FollowUpError::NoModel);
        assert!(notifier.delivered.borrow().is_empty());
    }
}
