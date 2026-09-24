//! `kintsu why`: what happened, from the first configured model that
//! answers; local ones only when the case holds a secret.

use thiserror::Error;

use crate::entities::{
    FailureCase, Message, MessageBody, ModelSpec, SessionId, Settings, case_document,
};
use crate::use_cases::ports::{
    CaseStore, CaseStoreError, Clock, ModelError, ModelGateway, Notifier, NotifyError, Secrets,
};
use crate::use_cases::prompts::explain_prompt;
use crate::use_cases::routing::{ask_first, excluded_for_sensitivity, model_candidates};

/// An explanation and where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Explanation {
    pub case: FailureCase,
    pub model: String,
    pub text: String,
    /// How many secrets were masked before the model saw the case.
    pub redactions: usize,
}

/// Why there is no explanation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExplainError {
    #[error("no failure to explain in this shell yet")]
    NoCase,
    #[error("no model is configured for explanations")]
    NoModel,
    #[error("this case holds a secret and only local models may see it; none is configured")]
    SensitiveWithoutLocalModel,
    #[error("no model answered ({})", .0.iter().map(|(n, e)| format!("{n}: {e}")).collect::<Vec<_>>().join("; "))]
    AllFailed(Vec<(String, ModelError)>),
    #[error(transparent)]
    Cases(#[from] CaseStoreError),
    #[error(transparent)]
    Notify(#[from] NotifyError),
}

/// Explains the last failure.
pub struct Explain<'a> {
    pub settings: &'a Settings,
    pub cases: &'a dyn CaseStore,
    pub secrets: &'a dyn Secrets,
    pub models: &'a dyn ModelGateway,
}

impl Explain<'_> {
    /// The last case and the models allowed to explain it, or why not.
    fn prepare(
        &self,
        session: Option<&SessionId>,
    ) -> Result<(FailureCase, Vec<&ModelSpec>), ExplainError> {
        let case = self.cases.last(session)?.ok_or(ExplainError::NoCase)?;
        let names = &self.settings.routing.explain;
        let candidates = model_candidates(self.settings, names, &case);
        if candidates.is_empty() {
            return Err(if excluded_for_sensitivity(self.settings, names, &case) {
                ExplainError::SensitiveWithoutLocalModel
            } else {
                ExplainError::NoModel
            });
        }
        Ok((case, candidates))
    }

    /// The model that would be asked first, without asking it: what the
    /// "asking…" line names. Errors are the ones `run` would give at once.
    pub fn candidate(&self, session: Option<&SessionId>) -> Result<String, ExplainError> {
        let (_, candidates) = self.prepare(session)?;
        Ok(candidates[0].name.clone())
    }

    /// Asks and sends the answer to the shell as a message; a failure is
    /// sent as one line too, so the "asking…" line never dangles.
    pub fn deliver(
        &self,
        session: &SessionId,
        notifier: &dyn Notifier,
        clock: &dyn Clock,
    ) -> Result<Message, ExplainError> {
        let case_id = match self.cases.last(Some(session))? {
            Some(case) => case.id().clone(),
            None => return Err(ExplainError::NoCase),
        };
        let body = match self.run(Some(session)) {
            Ok(explanation) => MessageBody::Explanation {
                model: explanation.model,
                text: explanation.text,
            },
            Err(e) => MessageBody::Note(e.to_string()),
        };
        let message = Message::new(case_id, clock.now(), body);
        notifier.deliver(session, message.clone())?;
        Ok(message)
    }

    pub fn run(&self, session: Option<&SessionId>) -> Result<Explanation, ExplainError> {
        let (case, candidates) = self.prepare(session)?;
        let redactions = case_document(&case).redactions;
        let (model, text) = ask_first(
            self.models,
            self.secrets,
            &candidates,
            &explain_prompt(&case),
        )
        .map_err(ExplainError::AllFailed)?;
        Ok(Explanation {
            case,
            model,
            text: text.trim().to_string(),
            redactions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{Provider, Routing, Tier};
    use crate::use_cases::testing::*;

    fn settings(explain: &[&str]) -> Settings {
        Settings {
            models: vec![
                spec("cloud", Provider::Anthropic, Tier::Small),
                spec("local", Provider::Ollama, Tier::Small),
            ],
            routing: Routing {
                explain: explain.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    const SECRET: &str = "curl -H 'Authorization: Bearer sk-live-abcdefghijklmnop' https://x";

    #[test]
    fn the_first_model_that_answers_explains_and_the_prompt_is_redacted() {
        let cases = MemoryCases::default();
        cases.save(&case(SECRET, 22, Some("42"))).unwrap();
        let models = ScriptedModels::answering(&[("local", Ok("  The token expired.\n"))]);
        let uc = Explain {
            settings: &settings(&["cloud", "local"]),
            cases: &cases,
            secrets: &MapSecrets::with(&[]),
            models: &models,
        };
        let e = uc.run(Some(&SessionId::new("42"))).unwrap();
        assert_eq!(
            (e.model.as_str(), e.text.as_str(), e.redactions),
            ("local", "The token expired.", 1)
        );
        assert_eq!(models.asked(), vec!["local"], "cloud never sees a secret");
        assert!(!models.calls.borrow()[0].2.user.contains("sk-live"));
    }

    #[test]
    fn the_errors_say_what_is_missing() {
        let cases = MemoryCases::default();
        let models = ScriptedModels::answering(&[(
            "cloud",
            Err(ModelError::MissingKey("ANTHROPIC_API_KEY".into())),
        )]);
        let secrets = MapSecrets::with(&[]);
        let none = Explain {
            settings: &settings(&["cloud"]),
            cases: &cases,
            secrets: &secrets,
            models: &models,
        };
        assert_eq!(none.run(None).unwrap_err(), ExplainError::NoCase);
        cases.save(&case("make", 2, None)).unwrap();
        let unconfigured = Explain {
            settings: &settings(&[]),
            cases: &cases,
            secrets: &secrets,
            models: &models,
        };
        assert_eq!(unconfigured.run(None).unwrap_err(), ExplainError::NoModel);
        assert!(matches!(none.run(None).unwrap_err(), ExplainError::AllFailed(f) if f.len() == 1));
        cases.save(&case(SECRET, 22, None)).unwrap();
        assert_eq!(
            none.run(None).unwrap_err(),
            ExplainError::SensitiveWithoutLocalModel
        );
    }

    #[test]
    fn deliver_sends_the_explanation_or_the_reason_as_a_message() {
        let cases = MemoryCases::default();
        cases.save(&case("make", 2, Some("42"))).unwrap();
        let models = ScriptedModels::answering(&[
            ("cloud", Ok("Because.")),
            ("local", Err(ModelError::Unreachable("down".into()))),
        ]);
        let notifier = MemoryNotifier::default();
        let clock = FakeClock::at(7);
        let secrets = MapSecrets::with(&[]);
        let uc = Explain {
            settings: &settings(&["cloud"]),
            cases: &cases,
            secrets: &secrets,
            models: &models,
        };
        assert_eq!(uc.candidate(Some(&SessionId::new("42"))).unwrap(), "cloud");
        let m = uc
            .deliver(&SessionId::new("42"), &notifier, &clock)
            .unwrap();
        assert_eq!(
            m.body(),
            &MessageBody::Explanation {
                model: "cloud".into(),
                text: "Because.".into()
            }
        );
        let down = Explain {
            settings: &settings(&["local"]),
            ..uc
        };
        let m = down
            .deliver(&SessionId::new("42"), &notifier, &clock)
            .unwrap();
        assert_eq!(
            m.body(),
            &MessageBody::Note("no model answered (local: unreachable: down)".into())
        );
        assert_eq!(notifier.delivered.borrow().len(), 2);
        assert_eq!(
            uc.candidate(Some(&SessionId::new("other"))).unwrap_err(),
            ExplainError::NoCase
        );
    }
}
