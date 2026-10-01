//! `kintsu models`: what is configured, whether each key is where the
//! configuration says, whether each server answers; and `kintsu models
//! test`, one word asked of each model. Keys are looked up, never shown.

use crate::entities::{Duration, KeySource, ModelSpec, Provider, Settings, Tier};
use crate::use_cases::ports::{
    AnswerShape, Clock, Environment, ModelError, ModelGateway, Prompt, Secrets,
};

/// Whether a model's key is there, without the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyStatus {
    /// A local server or a CLI agent: nothing to find.
    NotNeeded,
    /// The source yields a key.
    Found(KeySource),
    /// The source yields nothing; `KeySource::None` on a remote model.
    Missing(KeySource),
    /// A literal in the configuration file: found, and worth a warning.
    WrittenInConfig,
}

/// Whether the model can be spoken to right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// A local server that answers.
    Running,
    /// A local server that does not.
    NotRunning,
    /// A remote endpoint: only a request tells.
    Remote,
    /// A CLI agent whose program is on the PATH.
    OnPath,
    /// A CLI agent whose program is not.
    NotOnPath,
}

/// One configured model, as the table shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRow {
    pub name: String,
    pub provider: Provider,
    pub model: String,
    pub tier: Tier,
    pub key: KeyStatus,
    pub reach: Reach,
}

/// Reads the configuration against the machine, calling no model.
pub struct ListModels<'a> {
    pub settings: &'a Settings,
    pub secrets: &'a dyn Secrets,
    pub environment: &'a dyn Environment,
    pub models: &'a dyn ModelGateway,
}

impl ListModels<'_> {
    pub fn run(&self) -> Vec<ModelRow> {
        let executables = self.environment.executables();
        self.settings
            .models
            .iter()
            .map(|spec| ModelRow {
                name: spec.name.clone(),
                provider: spec.provider,
                model: spec.model.clone(),
                tier: spec.tier,
                key: key_status(spec, self.secrets),
                reach: match spec.provider {
                    Provider::CliAgent => {
                        let program = spec.model.split_whitespace().next().unwrap_or_default();
                        if executables.iter().any(|e| e == program) {
                            Reach::OnPath
                        } else {
                            Reach::NotOnPath
                        }
                    }
                    _ if !spec.is_local() => Reach::Remote,
                    _ if self.models.is_reachable(spec) => Reach::Running,
                    _ => Reach::NotRunning,
                },
            })
            .collect()
    }
}

fn key_status(spec: &ModelSpec, secrets: &dyn Secrets) -> KeyStatus {
    match &spec.key {
        KeySource::Literal(_) => KeyStatus::WrittenInConfig,
        KeySource::None if spec.provider == Provider::CliAgent || spec.is_local() => {
            KeyStatus::NotNeeded
        }
        KeySource::None => KeyStatus::Missing(KeySource::None),
        source if secrets.lookup(source).is_some() => KeyStatus::Found(source.clone()),
        source => KeyStatus::Missing(source.clone()),
    }
}

/// One model asked one word: how long it took, or why it did not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub name: String,
    pub outcome: Result<Duration, ModelError>,
}

/// `kintsu models test`: asks every model that can be asked.
pub struct ProbeModels<'a> {
    pub settings: &'a Settings,
    pub secrets: &'a dyn Secrets,
    pub models: &'a dyn ModelGateway,
    pub clock: &'a dyn Clock,
}

/// Short enough for a tiny model, unambiguous enough for a large one.
fn probe_prompt() -> Prompt {
    Prompt {
        system: "Answer with the single word: ok".into(),
        user: "Say ok.".into(),
        max_tokens: 8,
        shape: AnswerShape::Prose,
    }
}

impl ProbeModels<'_> {
    pub fn run(&self) -> Vec<Probe> {
        self.settings
            .models
            .iter()
            .map(|spec| Probe {
                name: spec.name.clone(),
                outcome: self.ask(spec),
            })
            .collect()
    }

    fn ask(&self, spec: &ModelSpec) -> Result<Duration, ModelError> {
        if spec.provider == Provider::CliAgent {
            return Err(ModelError::Unsupported(
                "a CLI agent is launched by `kintsu agent`, not asked".into(),
            ));
        }
        let key = self.secrets.lookup(&spec.key);
        if key.is_none() && !spec.is_local() {
            return Err(ModelError::MissingKey(describe(&spec.key)));
        }
        let started = self.clock.now();
        let answer = self
            .models
            .complete(spec, key.as_deref(), &probe_prompt())?;
        if answer.trim().is_empty() {
            return Err(ModelError::Malformed("empty answer".into()));
        }
        let ended = self.clock.now();
        Ok(Duration::from_millis(
            ended.as_millis().saturating_sub(started.as_millis()),
        ))
    }
}

/// The source, for the error, never the key.
pub fn describe(source: &KeySource) -> String {
    match source {
        KeySource::None => "no key source configured".into(),
        KeySource::Env(var) => format!("${var} is not set"),
        KeySource::Command(cmd) => format!("`{cmd}` gave no key"),
        KeySource::Keychain(account) => format!("no keychain entry kintsu/{account}"),
        KeySource::Literal(_) => "the key written in the config".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::use_cases::testing::*;

    fn settings() -> Settings {
        let mut cloud = spec("cloud", Provider::Anthropic, Tier::Large);
        cloud.key = KeySource::Env("ANTHROPIC_API_KEY".into());
        let mut vault = spec("vault", Provider::Anthropic, Tier::Large);
        vault.key = KeySource::Keychain("vault".into());
        let mut bare = spec("bare", Provider::OpenAiCompatible, Tier::Small);
        bare.base_url = Some("https://api.example.com/v1".into());
        let mut literal = spec("literal", Provider::Anthropic, Tier::Small);
        literal.key = KeySource::Literal("sk-x".into());
        let mut local = spec("local", Provider::Ollama, Tier::Small);
        local.base_url = Some("http://127.0.0.1:11434".into());
        let mut claude = spec("claude", Provider::CliAgent, Tier::Agent);
        claude.model = "claude --permission-mode plan".into();
        Settings {
            models: vec![
                cloud,
                vault,
                bare,
                literal,
                local,
                claude,
                spec("codex", Provider::CliAgent, Tier::Agent),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn every_model_gets_its_key_status_and_its_reach_without_a_call() {
        let settings = settings();
        let models = ScriptedModels::default();
        let uc = ListModels {
            settings: &settings,
            secrets: &MapSecrets::with(&[("ANTHROPIC_API_KEY", "k")]),
            environment: &FakeEnvironment::with_executables(&["claude"]),
            models: &models,
        };
        let rows = uc.run();
        let row = |name: &str| rows.iter().find(|r| r.name == name).unwrap();
        assert_eq!(
            row("cloud").key,
            KeyStatus::Found(KeySource::Env("ANTHROPIC_API_KEY".into()))
        );
        assert_eq!(row("cloud").reach, Reach::Remote);
        assert_eq!(
            row("vault").key,
            KeyStatus::Missing(KeySource::Keychain("vault".into()))
        );
        assert_eq!(row("bare").key, KeyStatus::Missing(KeySource::None));
        assert_eq!(row("literal").key, KeyStatus::WrittenInConfig);
        assert_eq!(row("local").key, KeyStatus::NotNeeded);
        assert_eq!(row("local").reach, Reach::Running);
        assert_eq!(row("claude").key, KeyStatus::NotNeeded);
        assert_eq!(row("claude").reach, Reach::OnPath);
        assert_eq!(row("codex").reach, Reach::NotOnPath);
        assert!(models.asked().is_empty(), "the table asks nobody");
        let mut stopped = ScriptedModels::default();
        stopped.down.push("local".into());
        let uc = ListModels {
            models: &stopped,
            ..uc
        };
        assert_eq!(
            uc.run().iter().find(|r| r.name == "local").unwrap().reach,
            Reach::NotRunning
        );
    }

    #[test]
    fn a_probe_times_each_answer_and_says_why_a_model_was_not_asked() {
        let settings = settings();
        let models = ScriptedModels::answering(&[
            ("cloud", Ok("ok")),
            ("local", Ok("")),
            ("literal", Err(ModelError::Refused("401".into()))),
        ]);
        let clock = FakeClock::at(1_000);
        let uc = ProbeModels {
            settings: &settings,
            secrets: &MapSecrets::with(&[("ANTHROPIC_API_KEY", "k")]),
            models: &models,
            clock: &clock,
        };
        let probes = uc.run();
        let probe = |name: &str| probes.iter().find(|p| p.name == name).unwrap();
        assert_eq!(probe("cloud").outcome, Ok(Duration::from_millis(0)));
        assert!(matches!(
            probe("vault").outcome,
            Err(ModelError::MissingKey(ref why)) if why == "no keychain entry kintsu/vault"
        ));
        assert!(matches!(
            probe("bare").outcome,
            Err(ModelError::MissingKey(_))
        ));
        assert!(matches!(
            probe("literal").outcome,
            Err(ModelError::Refused(_))
        ));
        assert!(matches!(
            probe("local").outcome,
            Err(ModelError::Malformed(_))
        ));
        assert!(matches!(
            probe("claude").outcome,
            Err(ModelError::Unsupported(_))
        ));
        let asked = models.asked();
        assert!(asked.contains(&"cloud".to_string()) && asked.contains(&"literal".to_string()));
        assert!(
            !asked.contains(&"vault".to_string()) && !asked.contains(&"claude".to_string()),
            "no key, or an agent: not asked"
        );
        let calls = models.calls.borrow();
        let literal = calls.iter().find(|(n, _, _)| n == "literal").unwrap();
        assert_eq!(
            literal.1.as_deref(),
            Some("sk-x"),
            "the literal key is used"
        );
        assert_eq!(literal.2.max_tokens, 8, "one word is enough");
    }
}
