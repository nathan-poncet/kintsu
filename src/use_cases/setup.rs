//! `kintsu setup`: three answers become a configuration. What the machine
//! offers (Ollama, the agents on the PATH) is read through the ports; the
//! questions themselves are the controller's business.

use crate::entities::{
    EagerFix, KeySource, ModelSpec, Provider, Routing, Settings, Tier, UiSettings,
};
use crate::use_cases::ports::{Environment, ModelGateway};

/// The CLI agents Kintsu knows how to launch, by the name of their program.
pub const AGENT_PRESETS: [&str; 6] = ["claude", "codex", "opencode", "aider", "gemini", "copilot"];

/// Ollama's default endpoint.
pub const OLLAMA_URL: &str = "http://127.0.0.1:11434";

/// What the machine has, before any question.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Detected {
    pub ollama_installed: bool,
    pub ollama_running: bool,
    /// Agent programs found on the PATH, in preset order.
    pub agents: Vec<String>,
}

pub struct Detect<'a> {
    pub environment: &'a dyn Environment,
    pub models: &'a dyn ModelGateway,
}

impl Detect<'_> {
    pub fn run(&self) -> Detected {
        let executables = self.environment.executables();
        let has = |name: &str| executables.iter().any(|e| e == name);
        let ollama = ModelSpec {
            name: "local".into(),
            provider: Provider::Ollama,
            model: String::new(),
            base_url: Some(OLLAMA_URL.into()),
            key: KeySource::None,
            tier: Tier::Small,
            timeout: None,
            max_output_tokens: None,
        };
        Detected {
            ollama_installed: has("ollama"),
            ollama_running: self.models.is_reachable(&ollama),
            agents: AGENT_PRESETS
                .iter()
                .filter(|a| has(a))
                .map(|a| a.to_string())
                .collect(),
        }
    }
}

/// A remote model the user wants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudKind {
    Anthropic,
    OpenAi,
    Gemini,
}

impl CloudKind {
    pub fn default_model(&self) -> &'static str {
        match self {
            CloudKind::Anthropic => "claude-haiku-4-5-20251001",
            CloudKind::OpenAi => "gpt-5-mini",
            CloudKind::Gemini => "gemini-2.5-flash",
        }
    }

    pub fn default_key_var(&self) -> &'static str {
        match self {
            CloudKind::Anthropic => "ANTHROPIC_API_KEY",
            CloudKind::OpenAi => "OPENAI_API_KEY",
            CloudKind::Gemini => "GEMINI_API_KEY",
        }
    }

    /// The name the model gets in the file.
    pub fn name(&self) -> &'static str {
        match self {
            CloudKind::Anthropic => "claude",
            CloudKind::OpenAi => "openai",
            CloudKind::Gemini => "gemini",
        }
    }

    fn provider(&self) -> Provider {
        match self {
            CloudKind::Anthropic => Provider::Anthropic,
            CloudKind::OpenAi | CloudKind::Gemini => Provider::OpenAiCompatible,
        }
    }

    fn base_url(&self) -> &'static str {
        match self {
            CloudKind::Anthropic => "https://api.anthropic.com",
            CloudKind::OpenAi => "https://api.openai.com/v1",
            CloudKind::Gemini => "https://generativelanguage.googleapis.com/v1beta/openai",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudChoice {
    pub kind: CloudKind,
    pub model: String,
    pub key: KeySource,
}

/// The three answers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SetupAnswers {
    /// An Ollama model id, when the user wants a local model.
    pub local_model: Option<String>,
    pub cloud: Option<CloudChoice>,
    /// A preset name (`claude`, `codex`…), when the user wants an agent.
    pub agent: Option<String>,
}

/// The configuration the answers describe: the local model first for
/// quick fixes, the cloud model first for explanations, the agent for
/// investigations, and the other defaults untouched.
pub fn compose(answers: &SetupAnswers) -> Settings {
    let mut models = Vec::new();
    let mut quick_fix = Vec::new();
    let mut explain = Vec::new();
    let mut investigate = Vec::new();
    if let Some(model) = &answers.local_model {
        models.push(ModelSpec {
            name: "local".into(),
            provider: Provider::Ollama,
            model: model.clone(),
            base_url: Some(OLLAMA_URL.into()),
            key: KeySource::None,
            tier: Tier::Large,
            timeout: None,
            max_output_tokens: None,
        });
        quick_fix.push("local".to_string());
        explain.push("local".to_string());
    }
    if let Some(cloud) = &answers.cloud {
        let name = cloud.kind.name().to_string();
        models.push(ModelSpec {
            name: name.clone(),
            provider: cloud.kind.provider(),
            model: cloud.model.clone(),
            base_url: Some(cloud.kind.base_url().into()),
            key: cloud.key.clone(),
            tier: Tier::Small,
            timeout: None,
            max_output_tokens: None,
        });
        quick_fix.push(name.clone());
        explain.insert(0, name);
    }
    if let Some(agent) = &answers.agent {
        let name = format!("{agent}-cli");
        models.push(ModelSpec {
            name: name.clone(),
            provider: Provider::CliAgent,
            model: agent.clone(),
            base_url: None,
            key: KeySource::None,
            tier: Tier::Agent,
            timeout: None,
            max_output_tokens: None,
        });
        investigate.push(name);
    }
    Settings {
        models,
        routing: Routing {
            quick_fix,
            explain,
            investigate,
        },
        ui: UiSettings {
            eager_fix: EagerFix::Auto,
            ..UiSettings::default()
        },
        ..Settings::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::use_cases::testing::*;

    #[test]
    fn detection_reads_the_path_and_probes_ollama() {
        let env = FakeEnvironment::with_executables(&["ollama", "codex", "claude", "ls"]);
        let mut models = ScriptedModels::default();
        let found = Detect {
            environment: &env,
            models: &models,
        }
        .run();
        assert_eq!(
            found,
            Detected {
                ollama_installed: true,
                ollama_running: true,
                agents: vec!["claude".into(), "codex".into()]
            }
        );
        models.down.push("local".into());
        let stopped = Detect {
            environment: &env,
            models: &models,
        }
        .run();
        assert!(stopped.ollama_installed && !stopped.ollama_running);
        let bare = Detect {
            environment: &FakeEnvironment::with_executables(&[]),
            models: &models,
        }
        .run();
        assert_eq!(
            bare,
            Detected {
                ollama_installed: false,
                ollama_running: false,
                agents: vec![]
            }
        );
    }

    #[test]
    fn the_answers_become_models_and_routes() {
        let answers = SetupAnswers {
            local_model: Some("qwen2.5-coder:7b".into()),
            cloud: Some(CloudChoice {
                kind: CloudKind::Anthropic,
                model: "claude-haiku-4-5-20251001".into(),
                key: KeySource::Env("ANTHROPIC_API_KEY".into()),
            }),
            agent: Some("codex".into()),
        };
        let s = compose(&answers);
        assert_eq!(
            s.models.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            vec!["local", "claude", "codex-cli"]
        );
        assert_eq!(
            s.routing.quick_fix,
            vec!["local", "claude"],
            "local first for quick fixes"
        );
        assert_eq!(
            s.routing.explain,
            vec!["claude", "local"],
            "the cloud model first for explanations"
        );
        assert_eq!(s.routing.investigate, vec!["codex-cli"]);
        assert_eq!(
            s.model("codex-cli").unwrap().model,
            "codex",
            "the preset name; the TOML edge expands it"
        );
        assert_eq!(s.model("claude").unwrap().provider, Provider::Anthropic);
        assert_eq!(s.model("local").unwrap().tier, Tier::Large);
        assert_eq!(s.ui.eager_fix, EagerFix::Auto);
        assert_eq!(
            compose(&SetupAnswers::default()),
            Settings::default(),
            "no answer, the defaults"
        );
    }

    #[test]
    fn each_cloud_kind_has_its_defaults() {
        assert_eq!(CloudKind::Gemini.default_key_var(), "GEMINI_API_KEY");
        assert_eq!(CloudKind::OpenAi.name(), "openai");
        let s = compose(&SetupAnswers {
            cloud: Some(CloudChoice {
                kind: CloudKind::Gemini,
                model: "gemini-2.5-flash".into(),
                key: KeySource::Keychain("gemini".into()),
            }),
            ..Default::default()
        });
        assert_eq!(
            s.model("gemini").unwrap().base_url.as_deref(),
            Some("https://generativelanguage.googleapis.com/v1beta/openai")
        );
        assert_eq!(
            s.model("gemini").unwrap().provider,
            Provider::OpenAiCompatible
        );
    }
}
