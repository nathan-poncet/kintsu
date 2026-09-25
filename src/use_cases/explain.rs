//! `kintsu why`: what happened, from the first configured model that
//! answers; local ones only when the case holds a secret.

use thiserror::Error;

use crate::entities::{Explanation, FailureCase, ModelSpec, SessionId, Settings, case_document};
use crate::use_cases::ports::{CaseStore, CaseStoreError, ModelError, ModelGateway, Secrets};
use crate::use_cases::prompts::explain_prompt;
use crate::use_cases::routing::{ask_first, excluded_for_sensitivity, model_candidates};

/// An explanation and where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Explained {
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

    /// Asks, and keeps the answer with the case while the case is still the
    /// shell's last, so the panel shows it again instead of asking again.
    pub fn run(&self, session: Option<&SessionId>) -> Result<Explained, ExplainError> {
        let (case, candidates) = self.prepare(session)?;
        let redactions = case_document(&case).redactions;
        let (model, text) = ask_first(
            self.models,
            self.secrets,
            &candidates,
            &explain_prompt(&case),
        )
        .map_err(ExplainError::AllFailed)?;
        let text = text.trim().to_string();
        if self.cases.still_current(&case)? {
            self.cases.save(
                &case
                    .clone()
                    .with_explanation(Explanation::new(&model, &text)),
            )?;
        }
        Ok(Explained {
            case,
            model,
            text,
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
    fn the_explanation_is_kept_with_the_case_so_the_panel_shows_it_again() {
        let cases = MemoryCases::default();
        cases.save(&case("make", 2, Some("42"))).unwrap();
        let models = ScriptedModels::answering(&[("local", Ok("The target is missing.\n"))]);
        let uc = Explain {
            settings: &settings(&["local"]),
            cases: &cases,
            secrets: &MapSecrets::with(&[]),
            models: &models,
        };
        uc.run(Some(&SessionId::new("42"))).unwrap();
        let kept = cases.last(Some(&SessionId::new("42"))).unwrap().unwrap();
        assert_eq!(
            kept.explanation(),
            Some(&Explanation::new("local", "The target is missing."))
        );
    }

    #[test]
    fn the_candidate_is_named_without_asking_anything() {
        let cases = MemoryCases::default();
        cases.save(&case("make", 2, Some("42"))).unwrap();
        let models = ScriptedModels::default();
        let uc = Explain {
            settings: &settings(&["cloud", "local"]),
            cases: &cases,
            secrets: &MapSecrets::with(&[]),
            models: &models,
        };
        assert_eq!(uc.candidate(Some(&SessionId::new("42"))).unwrap(), "cloud");
        assert_eq!(
            uc.candidate(Some(&SessionId::new("other"))).unwrap_err(),
            ExplainError::NoCase
        );
        assert!(models.asked().is_empty());
    }
}
