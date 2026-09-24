//! `kintsu agent`: hand the last failure to a command-line agent, in the
//! user's terminal. Two steps, so the user sees what is sent before the
//! agent takes the screen.

use thiserror::Error;

use crate::entities::{
    CaseId, ModelSpec, Provider, SessionId, Settings, hand_off_brief, suggest_fix,
};
use crate::use_cases::facts::gather_facts;
use crate::use_cases::ports::{AgentError, AgentLauncher, CaseStore, CaseStoreError, Environment};

/// What is about to be sent, and to whom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandOffPlan {
    pub case: CaseId,
    pub agent: ModelSpec,
    pub brief: String,
    /// How many secrets were masked in the brief.
    pub redactions: usize,
}

/// Why the hand-off did not happen.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HandOffError {
    #[error("no failure to hand off in this shell yet")]
    NoCase,
    #[error("no agent is configured; add one with provider = \"cli\"")]
    NoAgent,
    #[error("`{0}` is not a configured agent")]
    UnknownAgent(String),
    #[error(transparent)]
    Cases(#[from] CaseStoreError),
    #[error(transparent)]
    Launch(#[from] AgentError),
}

/// Prepares and launches a hand-off.
pub struct HandOff<'a> {
    pub settings: &'a Settings,
    pub cases: &'a dyn CaseStore,
    pub environment: &'a dyn Environment,
    pub launcher: &'a dyn AgentLauncher,
}

impl HandOff<'_> {
    /// Builds the brief for the named agent, or the first one routed for
    /// investigations, or the only one configured.
    pub fn prepare(
        &self,
        session: Option<&SessionId>,
        agent: Option<&str>,
        words: Option<&str>,
    ) -> Result<HandOffPlan, HandOffError> {
        let case = self.cases.last(session)?.ok_or(HandOffError::NoCase)?;
        let agent = self.pick(agent)?;
        let facts = gather_facts(self.environment, case.outcome(), case.cwd());
        let fix = suggest_fix(case.outcome(), &facts);
        let brief = hand_off_brief(&case, fix.as_ref(), words);
        let redactions = crate::entities::case_document(&case).redactions;
        Ok(HandOffPlan {
            case: case.id().clone(),
            agent: agent.clone(),
            brief,
            redactions,
        })
    }

    /// Starts the agent and waits for it.
    pub fn launch(&self, plan: &HandOffPlan) -> Result<(), HandOffError> {
        Ok(self.launcher.launch(&plan.agent, &plan.brief)?)
    }

    fn pick(&self, name: Option<&str>) -> Result<&ModelSpec, HandOffError> {
        let agents = |m: &&ModelSpec| m.provider == Provider::CliAgent;
        if let Some(name) = name {
            return self
                .settings
                .model(name)
                .filter(agents)
                .ok_or_else(|| HandOffError::UnknownAgent(name.to_string()));
        }
        self.settings
            .candidates(&self.settings.routing.investigate)
            .into_iter()
            .find(agents)
            .or_else(|| self.settings.models.iter().find(agents))
            .ok_or(HandOffError::NoAgent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{Routing, Tier};
    use crate::use_cases::testing::*;

    fn settings(investigate: &[&str]) -> Settings {
        Settings {
            models: vec![
                spec("local", Provider::Ollama, Tier::Small),
                spec("claude", Provider::CliAgent, Tier::Agent),
                spec("codex", Provider::CliAgent, Tier::Agent),
            ],
            routing: Routing {
                investigate: investigate.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn the_plan_names_the_routed_agent_and_carries_a_redacted_brief() {
        let cases = MemoryCases::default();
        cases
            .save(&case(
                "curl -H 'Authorization: Bearer sk-live-abcdefghijklmnop' https://x",
                22,
                Some("42"),
            ))
            .unwrap();
        let launcher = RecordingLauncher::default();
        let uc = HandOff {
            settings: &settings(&["codex", "claude"]),
            cases: &cases,
            environment: &FakeEnvironment::with_executables(&[]),
            launcher: &launcher,
        };
        let plan = uc
            .prepare(Some(&SessionId::new("42")), None, Some("do not touch prod"))
            .unwrap();
        assert_eq!(plan.agent.name, "codex");
        assert_eq!(plan.redactions, 1);
        assert!(plan.brief.contains("do not touch prod"));
        assert!(!plan.brief.contains("sk-live"));
        assert!(
            launcher.launched.borrow().is_empty(),
            "prepare launches nothing"
        );
        uc.launch(&plan).unwrap();
        assert_eq!(launcher.launched.borrow()[0].0, "codex");
    }

    #[test]
    fn an_agent_can_be_named_and_the_only_agent_is_the_default() {
        let cases = MemoryCases::default();
        cases.save(&case("gti status", 127, None)).unwrap();
        let launcher = RecordingLauncher::default();
        let env = FakeEnvironment::with_executables(&["git"]);
        let uc = HandOff {
            settings: &settings(&[]),
            cases: &cases,
            environment: &env,
            launcher: &launcher,
        };
        assert_eq!(
            uc.prepare(None, Some("codex"), None).unwrap().agent.name,
            "codex"
        );
        assert_eq!(
            uc.prepare(None, None, None).unwrap().agent.name,
            "claude",
            "first agent when nothing is routed"
        );
        assert_eq!(
            uc.prepare(None, Some("local"), None).unwrap_err(),
            HandOffError::UnknownAgent("local".into())
        );
        assert!(
            uc.prepare(None, None, None)
                .unwrap()
                .brief
                .contains("## A rule suggested\n\n`git status`")
        );
    }

    #[test]
    fn missing_pieces_are_named() {
        let cases = MemoryCases::default();
        let launcher = RecordingLauncher {
            failure: Some(AgentError::NotInstalled("codex".into())),
            ..Default::default()
        };
        let env = FakeEnvironment::with_executables(&[]);
        let uc = HandOff {
            settings: &settings(&[]),
            cases: &cases,
            environment: &env,
            launcher: &launcher,
        };
        assert_eq!(
            uc.prepare(None, None, None).unwrap_err(),
            HandOffError::NoCase
        );
        cases.save(&case("make", 2, None)).unwrap();
        let no_agents = Settings {
            models: vec![spec("local", Provider::Ollama, Tier::Small)],
            ..Default::default()
        };
        let bare = HandOff {
            settings: &no_agents,
            ..uc
        };
        assert_eq!(
            bare.prepare(None, None, None).unwrap_err(),
            HandOffError::NoAgent
        );
        let plan = uc.prepare(None, None, None).unwrap();
        assert_eq!(
            uc.launch(&plan).unwrap_err(),
            HandOffError::Launch(AgentError::NotInstalled("codex".into()))
        );
    }
}
