//! `kintsu why`: what happened, from the first configured model that
//! answers; local ones only when the case holds a secret.

use thiserror::Error;

use crate::entities::{
    Explanation, FailureCase, ModelSpec, SessionId, Settings, Task, case_document,
};
use crate::use_cases::ports::{
    CaseStore, CaseStoreError, Clock, CostLedger, ModelError, ModelGateway, Secrets,
};
use crate::use_cases::prompts::explain_prompt;
use crate::use_cases::routing::{
    Meter, ask_first, ask_first_streaming, excluded_for_budget, excluded_for_sensitivity,
    model_candidates, over_budget,
};

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
    #[error(
        "today's model budget is spent; remote models wait for midnight UTC and no local model is routed"
    )]
    BudgetReached,
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
    pub ledger: &'a dyn CostLedger,
    pub clock: &'a dyn Clock,
}

impl Explain<'_> {
    /// The last case and the models allowed to explain it, or why not.
    fn prepare(
        &self,
        session: Option<&SessionId>,
    ) -> Result<(FailureCase, Vec<&ModelSpec>), ExplainError> {
        let case = self.cases.last(session)?.ok_or(ExplainError::NoCase)?;
        let names = &self.settings.routing.explain;
        let over = over_budget(self.settings, self.ledger, self.clock);
        let candidates = model_candidates(self.settings, names, &case, over);
        if candidates.is_empty() {
            return Err(if excluded_for_sensitivity(self.settings, names, &case) {
                ExplainError::SensitiveWithoutLocalModel
            } else if excluded_for_budget(self.settings, names, over) {
                ExplainError::BudgetReached
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
        let answered = ask_first(
            self.models,
            self.secrets,
            &candidates,
            &explain_prompt(&case),
            &self.meter(),
        );
        self.keep(case, answered)
    }

    /// `run`, with the prose handed over as the model produces it: what the
    /// panel shows while it waits. `on_chunk` gets the model's name and
    /// the piece; a caller starts afresh when the name changes.
    pub fn run_streaming(
        &self,
        session: Option<&SessionId>,
        on_chunk: &mut dyn FnMut(&str, &str),
    ) -> Result<Explained, ExplainError> {
        let (case, candidates) = self.prepare(session)?;
        let answered = ask_first_streaming(
            self.models,
            self.secrets,
            &candidates,
            &explain_prompt(&case),
            &self.meter(),
            on_chunk,
        );
        self.keep(case, answered)
    }

    fn meter(&self) -> Meter<'_> {
        Meter {
            ledger: self.ledger,
            clock: self.clock,
            task: Task::Explain,
        }
    }

    fn keep(
        &self,
        case: FailureCase,
        answered: Result<(String, String), Vec<(String, ModelError)>>,
    ) -> Result<Explained, ExplainError> {
        let redactions = case_document(&case).redactions;
        let (model, text) = answered.map_err(ExplainError::AllFailed)?;
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
    use crate::entities::{Duration, LedgerEntry, Money, Provider, Routing, Tier, Tokens};
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
            ledger: &MemoryLedger::default(),
            clock: &FakeClock::at(0),
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
            ledger: &MemoryLedger::default(),
            clock: &FakeClock::at(0),
        };
        assert_eq!(none.run(None).unwrap_err(), ExplainError::NoCase);
        cases.save(&case("make", 2, None)).unwrap();
        let unconfigured = Explain {
            settings: &settings(&[]),
            cases: &cases,
            secrets: &secrets,
            models: &models,
            ledger: &MemoryLedger::default(),
            clock: &FakeClock::at(0),
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
            ledger: &MemoryLedger::default(),
            clock: &FakeClock::at(0),
        };
        uc.run(Some(&SessionId::new("42"))).unwrap();
        let kept = cases.last(Some(&SessionId::new("42"))).unwrap().unwrap();
        assert_eq!(
            kept.explanation(),
            Some(&Explanation::new("local", "The target is missing."))
        );
    }

    #[test]
    fn a_streamed_explanation_arrives_in_pieces_and_is_kept_whole() {
        let cases = MemoryCases::default();
        cases.save(&case("make", 2, Some("42"))).unwrap();
        let models = ScriptedModels::answering(&[
            ("cloud", Err(ModelError::Refused("HTTP 429".into()))),
            ("local", Ok("The target is missing.\n")),
        ]);
        let uc = Explain {
            settings: &settings(&["cloud", "local"]),
            cases: &cases,
            secrets: &MapSecrets::with(&[]),
            models: &models,
            ledger: &MemoryLedger::default(),
            clock: &FakeClock::at(0),
        };
        let mut pieces = Vec::new();
        let e = uc
            .run_streaming(Some(&SessionId::new("42")), &mut |model, chunk| {
                pieces.push((model.to_string(), chunk.to_string()))
            })
            .unwrap();
        assert_eq!(e.text, "The target is missing.");
        assert_eq!(pieces.len(), 4);
        assert!(pieces.iter().all(|(m, _)| m == "local"));
        let kept = cases.last(Some(&SessionId::new("42"))).unwrap().unwrap();
        assert_eq!(
            kept.explanation(),
            Some(&Explanation::new("local", "The target is missing."))
        );
    }

    #[test]
    fn once_the_budget_is_spent_remote_models_wait_and_local_ones_still_answer() {
        let cases = MemoryCases::default();
        cases.save(&case("make", 2, Some("42"))).unwrap();
        let ledger = MemoryLedger::default();
        let clock = FakeClock::at(1_790_637_207_000);
        ledger
            .record(&LedgerEntry {
                at: clock.now(),
                model: "cloud".into(),
                task: Task::Explain,
                tokens: Tokens::new(1, 1),
                cost: Some(Money::from_micro_usd(1_000)),
                latency: Duration::from_millis(1),
            })
            .unwrap();
        let mut remote_only = settings(&["cloud"]);
        remote_only.max_daily_cost = Some(Money::from_micro_usd(1_000));
        let models = ScriptedModels::answering(&[
            ("cloud", Ok("from the cloud")),
            ("local", Ok("from here")),
        ]);
        let uc = Explain {
            settings: &remote_only,
            cases: &cases,
            secrets: &MapSecrets::with(&[]),
            models: &models,
            ledger: &ledger,
            clock: &clock,
        };
        assert_eq!(
            uc.run(Some(&SessionId::new("42"))).unwrap_err(),
            ExplainError::BudgetReached
        );
        let mut with_local = settings(&["cloud", "local"]);
        with_local.max_daily_cost = Some(Money::from_micro_usd(1_000));
        let uc = Explain {
            settings: &with_local,
            ..uc
        };
        assert_eq!(uc.run(Some(&SessionId::new("42"))).unwrap().model, "local");
        assert_eq!(models.asked(), vec!["local"]);
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
            ledger: &MemoryLedger::default(),
            clock: &FakeClock::at(0),
        };
        assert_eq!(uc.candidate(Some(&SessionId::new("42"))).unwrap(), "cloud");
        assert_eq!(
            uc.candidate(Some(&SessionId::new("other"))).unwrap_err(),
            ExplainError::NoCase
        );
        assert!(models.asked().is_empty());
    }
}
