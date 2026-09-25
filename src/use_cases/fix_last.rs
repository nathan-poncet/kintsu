//! `kintsu fix`: the corrected command for the last failure, from a rule
//! when one knows, from a model when the user configured one.

use thiserror::Error;

use crate::entities::{FailureCase, Fix, SessionId, Settings, suggest_fix};
use crate::use_cases::facts::gather_facts;
use crate::use_cases::ports::{
    CaseStore, CaseStoreError, Environment, ModelError, ModelGateway, Secrets,
};
use crate::use_cases::prompts::{parse_quick_fix, quick_fix_prompt};
use crate::use_cases::routing::{ask_first, model_candidates};

/// The last case and what to type instead, when anyone knows.
#[derive(Debug, Clone, PartialEq)]
pub struct FixProposal {
    pub case: FailureCase,
    pub fix: Option<Fix>,
    /// Models that were asked and did not help.
    pub failures: Vec<(String, ModelError)>,
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
}

impl FixLast<'_> {
    /// Rules, then the proposal a model already left: what is known
    /// without asking anyone.
    pub fn known(&self, case: &FailureCase) -> Option<Fix> {
        let facts = gather_facts(self.environment, case.outcome(), case.cwd());
        suggest_fix(case.outcome(), &facts).or_else(|| case.proposal().cloned())
    }

    /// The quick-fix model that would be asked first, if one is routed.
    pub fn candidate(&self, case: &FailureCase) -> Option<String> {
        model_candidates(self.settings, &self.settings.routing.quick_fix, case)
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
            });
        }
        let candidates = model_candidates(self.settings, &self.settings.routing.quick_fix, &case);
        if candidates.is_empty() {
            return Ok(FixProposal {
                case,
                fix: None,
                failures: Vec::new(),
            });
        }
        match ask_first(
            self.models,
            self.secrets,
            &candidates,
            &quick_fix_prompt(&case),
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
                })
            }
            Err(failures) => Ok(FixProposal {
                case,
                fix: None,
                failures,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{CommandLine, Confidence, Fix, FixSource, Provider, Tier};
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
        };
        assert_eq!(none.run(Some(&SessionId::new("42"))).unwrap().fix, None);
        let down = FixLast {
            settings: &settings(&["local"]),
            cases: &cases,
            environment: &env,
            secrets: &MapSecrets::with(&[]),
            models: &models,
        };
        let proposal = down.run(Some(&SessionId::new("42"))).unwrap();
        assert_eq!(proposal.fix, None);
        assert_eq!(
            proposal.failures,
            vec![("local".to_string(), ModelError::Unreachable("down".into()))]
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
