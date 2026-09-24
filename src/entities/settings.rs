//! What the user decided, already typed and validated at the edge.

use crate::entities::Duration;

/// Which client speaks to a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// Ollama's native server.
    Ollama,
    /// Anything with `/chat/completions`: OpenAI, OpenRouter, Groq, LM Studio, Gemini's compatible endpoint…
    OpenAiCompatible,
    /// The Anthropic Messages API.
    Anthropic,
    /// A command-line agent, launched with the brief.
    CliAgent,
}

/// What a model may be asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Small, local, fast: classification and summaries.
    Tiny,
    /// One-line fixes and explanations.
    Small,
    /// Long explanations.
    Large,
    /// A CLI with tools, for investigations.
    Agent,
}

/// Where a key comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySource {
    /// No key: a local server, or a CLI with its own login.
    None,
    /// An environment variable.
    Env(String),
    /// The trimmed output of a command, such as `op read …`.
    Command(String),
    /// The OS keychain, under this account name.
    Keychain(String),
    /// Written in the file. Works; `doctor` warns.
    Literal(String),
}

/// One configured model or agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSpec {
    /// The name the user gave it.
    pub name: String,
    /// The client to use.
    pub provider: Provider,
    /// The model id; for an agent, the shell template that launches it,
    /// with `{brief}` standing for the path of the brief file.
    pub model: String,
    /// The endpoint, when the provider needs one.
    pub base_url: Option<String>,
    /// Where the key is.
    pub key: KeySource,
    /// What it is good for.
    pub tier: Tier,
    /// How long to wait for an answer.
    pub timeout: Option<Duration>,
    /// A ceiling on the answer length.
    pub max_output_tokens: Option<u32>,
}

impl ModelSpec {
    /// Whether answers stay on this machine.
    pub fn is_local(&self) -> bool {
        match self.provider {
            Provider::Ollama => true,
            Provider::CliAgent | Provider::Anthropic => false,
            Provider::OpenAiCompatible => self.base_url.as_deref().is_some_and(|u| {
                u.contains("localhost") || u.contains("127.0.0.1") || u.contains("[::1]")
            }),
        }
    }
}

/// Which models to try, in order, for each task.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Routing {
    /// For `kintsu why`.
    pub explain: Vec<String>,
    /// For one-line fixes when no rule knew.
    pub quick_fix: Vec<String>,
    /// For `kintsu agent`.
    pub investigate: Vec<String>,
}

/// When to stay quiet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuietSettings {
    /// Programs that are never triaged: editors, pagers, ssh…
    pub never_triage: Vec<String>,
    /// Exit statuses that are not failures, beyond the interruptions.
    pub ok_statuses: Vec<i32>,
    /// Per program, exit statuses that are not failures: `grep` and 1.
    pub ok_commands: Vec<(String, Vec<i32>)>,
    /// The same failure twice in a row is reported once.
    pub same_failure_once: bool,
    /// Directories, and what is under them, where nothing is ever shown.
    pub off_in: Vec<String>,
}

impl QuietSettings {
    /// Whether this status is fine for this program.
    pub fn accepts(&self, program: &str, status: i32) -> bool {
        self.ok_statuses.contains(&status)
            || self
                .ok_commands
                .iter()
                .any(|(p, codes)| p == program && codes.contains(&status))
    }

    /// Whether the directory is one where nothing is shown.
    pub fn is_off_in(&self, cwd: Option<&str>) -> bool {
        cwd.is_some_and(|c| {
            self.off_in
                .iter()
                .any(|d| c == d || c.starts_with(&format!("{d}/")))
        })
    }
}

impl Default for QuietSettings {
    fn default() -> Self {
        Self {
            never_triage: [
                "vim", "nvim", "vi", "nano", "emacs", "less", "more", "man", "ssh", "mosh", "top",
                "htop", "watch", "tmux", "fzf", "kintsu",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            ok_statuses: vec![],
            ok_commands: vec![
                ("grep".into(), vec![1]),
                ("rg".into(), vec![1]),
                ("diff".into(), vec![1]),
                ("test".into(), vec![1]),
            ],
            same_failure_once: true,
            off_in: vec![],
        }
    }
}

/// How the bubble is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiMode {
    /// A sentence and the actions.
    #[default]
    Toast,
    /// One dim line.
    Hint,
    /// Nothing; `kintsu fix`, `why`, `agent` still work.
    Silent,
}

/// Whether the quick-fix model is asked after every failure no rule can
/// fix, without being told to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EagerFix {
    /// When the model that would be asked first is local: nothing leaves
    /// the machine on its own.
    #[default]
    Auto,
    On,
    Off,
}

impl EagerFix {
    /// Whether this model may be asked without being told to.
    pub fn allows(self, model: &ModelSpec) -> bool {
        match self {
            EagerFix::Off => false,
            EagerFix::On => true,
            EagerFix::Auto => model.is_local(),
        }
    }
}

/// Presentation choices.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UiSettings {
    pub mode: UiMode,
    /// `| Enter ->` instead of `▎ ⏎ →`.
    pub ascii: bool,
    /// Ask the quick-fix model after every offer without a rule fix, and
    /// deliver the answer as a message.
    pub eager_fix: EagerFix,
}

/// Reading the failed command's output from the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureSettings {
    /// The sources to try, in order: `herdr`, `tmux`, `wezterm`, `kitty`, `iterm2`.
    pub sources: Vec<String>,
    /// How many lines of output a case keeps at most; 0 disables capture.
    pub max_lines: usize,
}

impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            sources: ["herdr", "tmux", "wezterm", "kitty", "iterm2"]
                .into_iter()
                .map(String::from)
                .collect(),
            max_lines: 400,
        }
    }
}

/// Everything the user configured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The models and agents, by name.
    pub models: Vec<ModelSpec>,
    /// Which model for which task.
    pub routing: Routing,
    /// When to stay quiet.
    pub quiet: QuietSettings,
    /// How to draw.
    pub ui: UiSettings,
    /// Where the output comes from.
    pub capture: CaptureSettings,
    /// Never send a case with a secret to a model that is not local.
    pub sensitive_local_only: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            models: Vec::new(),
            routing: Routing::default(),
            quiet: QuietSettings::default(),
            ui: UiSettings::default(),
            capture: CaptureSettings::default(),
            sensitive_local_only: true,
        }
    }
}

impl Settings {
    /// The model with that name.
    pub fn model(&self, name: &str) -> Option<&ModelSpec> {
        self.models.iter().find(|m| m.name == name)
    }

    /// The candidates for a task, in order, existing ones only.
    pub fn candidates(&self, names: &[String]) -> Vec<&ModelSpec> {
        names.iter().filter_map(|n| self.model(n)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str, provider: Provider, base_url: Option<&str>) -> ModelSpec {
        ModelSpec {
            name: name.into(),
            provider,
            model: "m".into(),
            base_url: base_url.map(String::from),
            key: KeySource::None,
            tier: Tier::Small,
            timeout: None,
            max_output_tokens: None,
        }
    }

    #[test]
    fn ollama_and_loopback_endpoints_are_local_the_rest_is_not() {
        assert!(spec("t", Provider::Ollama, None).is_local());
        assert!(
            spec(
                "l",
                Provider::OpenAiCompatible,
                Some("http://localhost:1234/v1")
            )
            .is_local()
        );
        assert!(
            !spec(
                "r",
                Provider::OpenAiCompatible,
                Some("https://openrouter.ai/api/v1")
            )
            .is_local()
        );
        assert!(!spec("a", Provider::Anthropic, None).is_local());
    }

    #[test]
    fn candidates_keep_the_order_and_skip_unknown_names() {
        let s = Settings {
            models: vec![
                spec("a", Provider::Anthropic, None),
                spec("b", Provider::Ollama, None),
            ],
            ..Default::default()
        };
        let names = vec!["b".to_string(), "zzz".to_string(), "a".to_string()];
        assert_eq!(
            s.candidates(&names)
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "a"]
        );
    }

    #[test]
    fn the_defaults_keep_editors_quiet_accept_grep_one_and_protect_secrets() {
        let s = Settings::default();
        for p in ["vim", "less", "ssh", "kintsu"] {
            assert!(s.quiet.never_triage.iter().any(|n| n == p), "{p}");
        }
        assert!(s.quiet.accepts("grep", 1));
        assert!(!s.quiet.accepts("grep", 2));
        assert!(!s.quiet.accepts("make", 1));
        assert!(s.quiet.same_failure_once);
        assert!(s.sensitive_local_only);
        assert_eq!(s.ui.mode, UiMode::Toast);
        assert_eq!(s.ui.eager_fix, EagerFix::Auto);
        assert!(EagerFix::Auto.allows(&spec("t", Provider::Ollama, None)));
        assert!(!EagerFix::Auto.allows(&spec("a", Provider::Anthropic, None)));
        assert!(EagerFix::On.allows(&spec("a", Provider::Anthropic, None)));
        assert!(!EagerFix::Off.allows(&spec("t", Provider::Ollama, None)));
    }

    #[test]
    fn off_in_covers_a_directory_and_what_is_under_it() {
        let q = QuietSettings {
            off_in: vec!["/home/me/scratch".into()],
            ..Default::default()
        };
        assert!(q.is_off_in(Some("/home/me/scratch")));
        assert!(q.is_off_in(Some("/home/me/scratch/x")));
        assert!(!q.is_off_in(Some("/home/me/scratchy")));
        assert!(!q.is_off_in(None));
    }
}
