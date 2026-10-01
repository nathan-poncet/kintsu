//! `kintsu fix`: the corrected command for the last failure, from a rule
//! when one knows, from a model when the user configured one.

use thiserror::Error;

use crate::entities::{FailureCase, Fix, SessionId, Settings, Task};
use crate::use_cases::facts::rule_fix;
use crate::use_cases::ports::{
    CaseStore, CaseStoreError, Clock, CostLedger, Environment, LearnedFixes, ModelError,
    ModelGateway, Secrets,
};
use crate::use_cases::prompts::{parse_quick_fix, quick_fix_prompt};
use crate::use_cases::routing::{
    Meter, ask_first, excluded_for_budget, model_candidates, over_budget,
};

/// The last case and what to type instead, when anyone knows.
#[derive(Debug, Clone, PartialEq)]
pub struct FixProposal {
    pub case: FailureCase,
    pub fix: Option<Fix>,
    /// Models that were asked and did not help.
    pub failures: Vec<(String, ModelError)>,
    /// A remote model would have been asked, but today's budget is spent.
    pub budget_reached: bool,
}

/// Why there is nothing to fix.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FixError {
    #[error("no failure to fix in this shell yet")]
    NoCase,
    #[error(transparent)]
    Cases(#[from] CaseStoreError),
}

/// Proposes a fix for the last failure.
pub struct FixLast<'a> {
    pub settings: &'a Settings,
    pub cases: &'a dyn CaseStore,
    pub environment: &'a dyn Environment,
    pub secrets: &'a dyn Secrets,
    pub models: &'a dyn ModelGateway,
    pub ledger: &'a dyn CostLedger,
    pub clock: &'a dyn Clock,
    pub learned: &'a dyn LearnedFixes,
}

impl FixLast<'_> {
    /// Rules, on the line then on the output, then what the user taught,
    /// then the proposal a model already left: what is known without
    /// asking anyone.
    pub fn known(&self, case: &FailureCase) -> Option<Fix> {
        rule_fix(self.environment, self.learned, case).or_else(|| case.proposal().cloned())
    }

    /// The quick-fix model that would be asked first, if one is routed and
    /// may still be paid for today.
    pub fn candidate(&self, case: &FailureCase) -> Option<String> {
        let over = over_budget(self.settings, self.ledger, self.clock);
        model_candidates(self.settings, &self.settings.routing.quick_fix, case, over)
            .first()
            .map(|model| model.name.clone())
    }

    /// Rules first; a quick-fix model only when no rule matched.
    pub fn run(&self, session: Option<&SessionId>) -> Result<FixProposal, FixError> {
        let case = self.cases.last(session)?.ok_or(FixError::NoCase)?;
        if let Some(fix) = self.known(&case) {
            return Ok(FixProposal {
                case,
                fix: Some(fix),
                failures: Vec::new(),
                budget_reached: false,
            });
        }
        let names = &self.settings.routing.quick_fix;
        let over = over_budget(self.settings, self.ledger, self.clock);
        let candidates = model_candidates(self.settings, names, &case, over);
        if candidates.is_empty() {
            return Ok(FixProposal {
                case,
                fix: None,
                failures: Vec::new(),
                budget_reached: excluded_for_budget(self.settings, names, over),
            });
        }
        let meter = Meter {
            ledger: self.ledger,
            clock: self.clock,
            task: Task::QuickFix,
        };
        match ask_first(
            self.models,
            self.secrets,
            &candidates,
            &quick_fix_prompt(&case, self.settings.language()),
            &meter,
        ) {
            Ok((name, answer)) => {
                let fix = parse_quick_fix(&answer, &name, case.outcome().command());
                if fix.is_some() && self.cases.still_current(&case)? {
                    self.cases.save(&case.clone().with_proposal(fix.clone()))?;
                }
                Ok(FixProposal {
                    fix,
                    case,
                    failures: Vec::new(),
                    budget_reached: false,
                })
            }
            Err(failures) => Ok(FixProposal {
                case,
                fix: None,
                failures,
                budget_reached: false,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{CommandLine, Confidence, Fix, FixSource, Money, Provider, Tier, Tokens};
    use crate::use_cases::testing::*;

    fn settings(quick_fix: &[&str]) -> Settings {
        Settings {
            models: vec![
                spec("local", Provider::Ollama, Tier::Small),
                spec("cloud", Provider::Anthropic, Tier::Small),
            ],
            routing: crate::entities::Routing {
                quick_fix: quick_fix.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn what_is_known_comes_from_rules_or_a_stored_proposal_and_the_candidate_is_the_first_routed_model()
     {
        let cases = MemoryCases::default();
        let models = ScriptedModels::default();
        let secrets = MapSecrets::with(&[]);
        let environment = FakeEnvironment::with_executables(&["git"]);
        let routed = settings(&["local", "cloud"]);
        let uc = FixLast {
            settings: &routed,
            cases: &cases,
            environment: &environment,
            secrets: &secrets,
            models: &models,
            ledger: &MemoryLedger::default(),
            clock: &FakeClock::at(0),
            learned: &MemoryLearned::default(),
        };
        let typo = case("gti status", 127, Some("42"));
        assert_eq!(uc.known(&typo).unwrap().command().as_str(), "git status");
        let proposal = Fix::new(
            CommandLine::new("make -j4").unwrap(),
            Confidence::new(0.6),
            FixSource::Model("local".into()),
            "",
        );
        let stored = case("make", 2, Some("42")).with_proposal(Some(proposal));
        assert_eq!(uc.known(&stored).unwrap().command().as_str(), "make -j4");
        assert_eq!(uc.known(&case("make", 2, Some("42"))), None);
        assert_eq!(uc.candidate(&typo), Some("local".to_string()));
        let unrouted = settings(&[]);
        let quiet = FixLast {
            settings: &unrouted,
            ..uc
        };
        assert_eq!(quiet.candidate(&typo), None);
        assert!(models.asked().is_empty(), "knowing asks nobody");
    }

    #[test]
    fn a_rule_that_reads_the_output_is_known_without_asking_a_model() {
        let cases = MemoryCases::default();
        let refused = case("touch /etc/hosts.new", 1, Some("42"))
            .with_output("touch: /etc/hosts.new: Permission denied".into());
        cases.save(&refused).unwrap();
        let models = ScriptedModels::answering(&[("local", Ok("sudo -i"))]);
        let uc = FixLast {
            settings: &settings(&["local"]),
            cases: &cases,
            environment: &FakeEnvironment::with_executables(&[]),
            secrets: &MapSecrets::with(&[]),
            models: &models,
            ledger: &MemoryLedger::default(),
            clock: &FakeClock::at(0),
            learned: &MemoryLearned::default(),
        };
        let fix = uc.run(Some(&SessionId::new("42"))).unwrap().fix.unwrap();
        assert_eq!(fix.command().as_str(), "sudo touch /etc/hosts.new");
        assert_eq!(fix.source(), &FixSource::Rule("needs root".into()));
        assert!(models.asked().is_empty());
    }

    #[test]
    fn a_rule_fix_needs_no_model() {
        let cases = MemoryCases::default();
        cases.save(&case("gti status", 127, Some("42"))).unwrap();
        let models = ScriptedModels::default();
        let uc = FixLast {
            settings: &settings(&["local"]),
            cases: &cases,
            environment: &FakeEnvironment::with_executables(&["git"]),
            secrets: &MapSecrets::with(&[]),
            models: &models,
            ledger: &MemoryLedger::default(),
            clock: &FakeClock::at(0),
            learned: &MemoryLearned::default(),
        };
        let proposal = uc.run(Some(&SessionId::new("42"))).unwrap();
        assert_eq!(proposal.fix.unwrap().command().as_str(), "git status");
        assert!(models.asked().is_empty());
    }

    #[test]
    fn without_a_rule_the_quick_fix_model_is_asked_and_its_answer_kept_as_the_proposal() {
        let cases = MemoryCases::default();
        cases.save(&case("npm test", 1, Some("42"))).unwrap();
        let models = ScriptedModels::answering(&[("local", Ok("npm test -- --runInBand"))]);
        let uc = FixLast {
            settings: &settings(&["local"]),
            cases: &cases,
            environment: &FakeEnvironment::with_executables(&["npm"]),
            secrets: &MapSecrets::with(&[]),
            models: &models,
            ledger: &MemoryLedger::default(),
            clock: &FakeClock::at(0),
            learned: &MemoryLearned::default(),
        };
        let fix = uc.run(Some(&SessionId::new("42"))).unwrap().fix.unwrap();
        assert_eq!(fix.command().as_str(), "npm test -- --runInBand");
        assert_eq!(fix.source(), &FixSource::Model("local".into()));
        let kept = cases.last(Some(&SessionId::new("42"))).unwrap().unwrap();
        assert_eq!(kept.proposal(), Some(&fix), "the panel reuses it");
    }

    #[test]
    fn no_rule_and_no_model_means_no_fix_and_no_error() {
        let cases = MemoryCases::default();
        cases.save(&case("npm test", 1, Some("42"))).unwrap();
        let models =
            ScriptedModels::answering(&[("local", Err(ModelError::Unreachable("down".into())))]);
        let env = FakeEnvironment::with_executables(&["npm"]);
        let none = FixLast {
            settings: &settings(&[]),
            cases: &cases,
            environment: &env,
            secrets: &MapSecrets::with(&[]),
            models: &models,
            ledger: &MemoryLedger::default(),
            clock: &FakeClock::at(0),
            learned: &MemoryLearned::default(),
        };
        assert_eq!(none.run(Some(&SessionId::new("42"))).unwrap().fix, None);
        let down = FixLast {
            settings: &settings(&["local"]),
            cases: &cases,
            environment: &env,
            secrets: &MapSecrets::with(&[]),
            models: &models,
            ledger: &MemoryLedger::default(),
            clock: &FakeClock::at(0),
            learned: &MemoryLearned::default(),
        };
        let proposal = down.run(Some(&SessionId::new("42"))).unwrap();
        assert_eq!(proposal.fix, None);
        assert_eq!(
            proposal.failures,
            vec![("local".to_string(), ModelError::Unreachable("down".into()))]
        );
    }

    #[test]
    fn the_second_remote_call_of_the_day_is_refused_once_the_budget_is_spent() {
        let cases = MemoryCases::default();
        cases.save(&case("npm test", 1, Some("42"))).unwrap();
        let mut settings = settings(&["cloud"]);
        settings.models[1].model = "claude-haiku-4-5-20251001".into();
        settings.max_daily_cost = Some(Money::from_micro_usd(1_000));
        let models = ScriptedModels::answering(&[("cloud", Ok("npm test -- --runInBand"))])
            .counting(&[("cloud", Tokens::new(1_000, 200))]);
        let ledger = MemoryLedger::default();
        let env = FakeEnvironment::with_executables(&["npm"]);
        let uc = FixLast {
            settings: &settings,
            cases: &cases,
            environment: &env,
            secrets: &MapSecrets::with(&[]),
            models: &models,
            ledger: &ledger,
            clock: &FakeClock::at(1_790_637_207_000),
            learned: &MemoryLearned::default(),
        };
        let first = uc.run(Some(&SessionId::new("42"))).unwrap();
        assert!(first.fix.is_some());
        assert!(!first.budget_reached);
        assert_eq!(
            ledger.entries.borrow()[0].cost,
            Some(Money::from_micro_usd(2_000)),
            "the ledger holds the first call, over the cap on its own"
        );
        cases.save(&case("npm run build", 1, Some("42"))).unwrap();
        let second = uc.run(Some(&SessionId::new("42"))).unwrap();
        assert_eq!(second.fix, None);
        assert!(
            second.budget_reached,
            "a remote model was skipped for the budget"
        );
        assert!(
            second.failures.is_empty(),
            "nobody was asked, nobody failed"
        );
        assert_eq!(models.asked().len(), 1);
        assert_eq!(
            uc.candidate(&case("npm run build", 1, Some("42"))),
            None,
            "nothing to announce as asking"
        );
    }

    #[test]
    fn a_fix_a_message_already_proposed_is_reused_without_asking_again() {
        let cases = MemoryCases::default();
        let proposed = Fix::new(
            CommandLine::new("nvm use 22").unwrap(),
            Confidence::new(0.6),
            FixSource::Model("local".into()),
            "suggested by local",
        );
        cases
            .save(&case("npm test", 1, Some("42")).with_proposal(Some(proposed.clone())))
            .unwrap();
        let models = ScriptedModels::answering(&[("local", Ok("something else"))]);
        let env = FakeEnvironment::with_executables(&["npm"]);
        let uc = FixLast {
            settings: &settings(&["local"]),
            cases: &cases,
            environment: &env,
            secrets: &MapSecrets::with(&[]),
            models: &models,
            ledger: &MemoryLedger::default(),
            clock: &FakeClock::at(0),
            learned: &MemoryLearned::default(),
        };
        assert_eq!(
            uc.run(Some(&SessionId::new("42"))).unwrap().fix,
            Some(proposed)
        );
        assert!(models.asked().is_empty());
    }

    #[test]
    fn no_case_in_this_session_is_an_error() {
        let cases = MemoryCases::default();
        cases.save(&case("npm test", 1, Some("other"))).unwrap();
        let models = ScriptedModels::default();
        let env = FakeEnvironment::with_executables(&[]);
        let uc = FixLast {
            settings: &settings(&[]),
            cases: &cases,
            environment: &env,
            secrets: &MapSecrets::with(&[]),
            models: &models,
            ledger: &MemoryLedger::default(),
            clock: &FakeClock::at(0),
            learned: &MemoryLearned::default(),
        };
        assert_eq!(
            uc.run(Some(&SessionId::new("42"))).unwrap_err(),
            FixError::NoCase
        );
        assert!(
            uc.run(None).is_ok(),
            "no session known: the last case at all"
        );
    }
}
