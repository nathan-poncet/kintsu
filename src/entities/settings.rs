//! What the user decided, already typed and validated at the edge.

use std::fmt;

use thiserror::Error;

use crate::entities::{Duration, Money};

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

impl Provider {
    /// The name the configuration file uses.
    pub const fn name(self) -> &'static str {
        match self {
            Provider::Ollama => "ollama",
            Provider::OpenAiCompatible => "openai_compatible",
            Provider::Anthropic => "anthropic",
            Provider::CliAgent => "cli_agent",
        }
    }
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

impl Tier {
    /// The name the configuration file uses.
    pub const fn name(self) -> &'static str {
        match self {
            Tier::Tiny => "tiny",
            Tier::Small => "small",
            Tier::Large => "large",
            Tier::Agent => "agent",
        }
    }
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

/// The key that expands the last bubble into the panel: one control
/// character, `^K` unless the configuration says otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey(char);

/// Why a configuration value is not a hotkey.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HotkeyError {
    #[error("`{0}` is not a control key; write one as `^K` or `ctrl-k`")]
    NotAControlKey(String),
    #[error("`^{0}` is {1}, which the terminal or the line editor already uses")]
    Taken(char, &'static str),
}

/// Control keys the terminal driver or the line editor own; binding them
/// would break typing, so they are refused at the edge.
const TAKEN_KEYS: &[(char, &str)] = &[
    ('C', "the interrupt"),
    ('D', "end of input"),
    ('H', "Backspace"),
    ('I', "Tab"),
    ('J', "Enter"),
    ('M', "Enter"),
    ('Q', "flow control"),
    ('S', "flow control"),
    ('Z', "suspend"),
];

impl Hotkey {
    pub const DEFAULT: Hotkey = Hotkey('K');

    /// From the configuration: `^K`, `ctrl-k`, `ctrl+k` or `C-k`, any case.
    pub fn parse(text: &str) -> Result<Self, HotkeyError> {
        let text = text.trim();
        let lower = text.to_ascii_lowercase();
        let letter = ["^", "ctrl-", "ctrl+", "control-", "c-"]
            .iter()
            .find_map(|prefix| lower.strip_prefix(prefix))
            .filter(|rest| rest.chars().count() == 1)
            .and_then(|rest| rest.chars().next())
            .filter(char::is_ascii_alphabetic)
            .ok_or_else(|| HotkeyError::NotAControlKey(text.to_string()))?
            .to_ascii_uppercase();
        if let Some((_, what)) = TAKEN_KEYS.iter().find(|(taken, _)| *taken == letter) {
            return Err(HotkeyError::Taken(letter, what));
        }
        Ok(Self(letter))
    }

    /// The letter, upper case: `K`.
    pub fn letter(self) -> char {
        self.0
    }

    /// As zsh's `bindkey` wants it: `^K`.
    pub fn zsh(self) -> String {
        format!("^{}", self.0)
    }

    /// As fish's `bind` wants it: `\ck`.
    pub fn fish(self) -> String {
        format!("\\c{}", self.0.to_ascii_lowercase())
    }

    /// As bash's `bind -x` wants it inside its quotes: `\C-k`.
    pub fn bash(self) -> String {
        format!("\\C-{}", self.0.to_ascii_lowercase())
    }
}

impl Default for Hotkey {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// For people: `^K`.
impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "^{}", self.0)
    }
}

/// The language the models answer in. Commands and code have none; rule
/// texts are code too and stay English.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
    French,
    German,
    Spanish,
    Italian,
    Portuguese,
    Dutch,
    Polish,
    Russian,
    Japanese,
    Chinese,
    Korean,
}

/// ISO 639-1 code, English name.
const LANGUAGES: &[(Language, &str, &str)] = &[
    (Language::English, "en", "English"),
    (Language::French, "fr", "French"),
    (Language::German, "de", "German"),
    (Language::Spanish, "es", "Spanish"),
    (Language::Italian, "it", "Italian"),
    (Language::Portuguese, "pt", "Portuguese"),
    (Language::Dutch, "nl", "Dutch"),
    (Language::Polish, "pl", "Polish"),
    (Language::Russian, "ru", "Russian"),
    (Language::Japanese, "ja", "Japanese"),
    (Language::Chinese, "zh", "Chinese"),
    (Language::Korean, "ko", "Korean"),
];

impl Language {
    /// From a code or an English name, any case: `fr`, `FR`, `french`.
    pub fn from_tag(tag: &str) -> Option<Self> {
        let tag = tag.trim().to_ascii_lowercase();
        LANGUAGES
            .iter()
            .find(|(_, code, name)| *code == tag || name.to_ascii_lowercase() == tag)
            .map(|(language, _, _)| *language)
    }

    /// From a locale as `LANG` holds it: `fr_FR.UTF-8`, `fr-CA`, `fr`.
    /// `C` and `POSIX` are English; a language this list does not know is
    /// nothing, and the caller falls back to English.
    pub fn from_locale(locale: &str) -> Option<Self> {
        let locale = locale.trim();
        let language = locale
            .split(['_', '-', '.', '@'])
            .next()
            .unwrap_or_default();
        match language {
            "" | "C" | "POSIX" => Some(Language::English),
            code => Self::from_tag(code),
        }
    }

    /// The ISO 639-1 code: `fr`.
    pub fn code(self) -> &'static str {
        LANGUAGES
            .iter()
            .find(|(language, _, _)| *language == self)
            .map_or("en", |(_, code, _)| code)
    }

    /// The English name, for a prompt: `French`.
    pub fn name(self) -> &'static str {
        LANGUAGES
            .iter()
            .find(|(language, _, _)| *language == self)
            .map_or("English", |(_, _, name)| name)
    }

    /// Every code the configuration accepts, for an error message.
    pub fn codes() -> impl Iterator<Item = &'static str> {
        LANGUAGES.iter().map(|(_, code, _)| *code)
    }
}

/// For people: `French`.
impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// `[ui] language`: follow the machine, or one language whatever it says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LanguageSetting {
    /// The shell's locale decides; English when it says nothing kintsu knows.
    #[default]
    Auto,
    Fixed(Language),
}

impl LanguageSetting {
    /// From the configuration: `auto`, a code or an English name.
    pub fn parse(text: &str) -> Result<Self, LanguageError> {
        let text = text.trim();
        if text.eq_ignore_ascii_case("auto") {
            return Ok(Self::Auto);
        }
        Language::from_tag(text)
            .map(Self::Fixed)
            .ok_or_else(|| LanguageError::Unknown(text.to_string()))
    }
}

/// Why a configuration value is not a language.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LanguageError {
    #[error("`{0}` is not a language kintsu knows; write `auto` or one of {codes}", codes = Language::codes().collect::<Vec<_>>().join(", "))]
    Unknown(String),
}

/// Presentation choices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiSettings {
    pub mode: UiMode,
    /// `| Enter ->` instead of `▎ ⏎ →`.
    pub ascii: bool,
    /// Ask the quick-fix model after every offer without a rule fix, and
    /// deliver the answer as a message.
    pub eager_fix: EagerFix,
    /// The bubble's words are OSC 8 links to `kintsu://` actions.
    pub links: bool,
    /// The key that opens the panel.
    pub hotkey: Hotkey,
    /// The language the models answer in.
    pub language: LanguageSetting,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            mode: UiMode::default(),
            ascii: false,
            eager_fix: EagerFix::default(),
            links: true,
            hotkey: Hotkey::DEFAULT,
            language: LanguageSetting::Auto,
        }
    }
}

/// Reading the failed command's output from the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureSettings {
    /// The sources to try, in order: `stderr` (the shell's own copy),
    /// `herdr`, `tmux`, `wezterm`, `kitty`, `iterm2`.
    pub sources: Vec<String>,
    /// How many lines of output a case keeps at most; 0 disables capture.
    pub max_lines: usize,
    /// Whether the zsh and bash hooks copy each command's stderr through
    /// `tee`, for terminals no source can read. Off by default: it costs a
    /// process per command and stderr stops being a tty for the command.
    pub stderr_tee: bool,
}

impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            sources: ["herdr", "tmux", "wezterm", "kitty", "iterm2"]
                .into_iter()
                .map(String::from)
                .collect(),
            max_lines: 400,
            stderr_tee: false,
        }
    }
}

/// The resident process, as the hooks talk to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonSettings {
    /// How long a hook waits for the daemon's decision before deciding
    /// locally; the prompt never waits longer.
    pub sync_budget: Duration,
}

impl Default for DaemonSettings {
    fn default() -> Self {
        Self {
            sync_budget: Duration::from_millis(40),
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
    /// The resident process.
    pub daemon: DaemonSettings,
    /// Never send a case with a secret to a model that is not local.
    pub sensitive_local_only: bool,
    /// Past this much in a UTC day, remote models are skipped until midnight.
    pub max_daily_cost: Option<Money>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            models: Vec::new(),
            routing: Routing::default(),
            quiet: QuietSettings::default(),
            ui: UiSettings::default(),
            capture: CaptureSettings::default(),
            daemon: DaemonSettings::default(),
            sensitive_local_only: true,
            max_daily_cost: None,
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

    /// The language the models answer in: the configured one, else what
    /// `with_machine_language` settled on, else English.
    pub fn language(&self) -> Language {
        match self.ui.language {
            LanguageSetting::Fixed(language) => language,
            LanguageSetting::Auto => Language::English,
        }
    }

    /// Settles `auto` on the machine's language, when the edge knows it. A
    /// configured language is left alone.
    pub fn with_machine_language(mut self, machine: Option<Language>) -> Self {
        if let (LanguageSetting::Auto, Some(language)) = (self.ui.language, machine) {
            self.ui.language = LanguageSetting::Fixed(language);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locale_names_its_language_and_c_and_posix_mean_english() {
        assert_eq!(Language::from_locale("fr_FR.UTF-8"), Some(Language::French));
        assert_eq!(Language::from_locale("fr-CA"), Some(Language::French));
        assert_eq!(Language::from_locale("de_DE@euro"), Some(Language::German));
        assert_eq!(
            Language::from_locale("en_US.UTF-8"),
            Some(Language::English)
        );
        for plain in ["C", "POSIX", "C.UTF-8", ""] {
            assert_eq!(
                Language::from_locale(plain),
                Some(Language::English),
                "{plain}"
            );
        }
        assert_eq!(
            Language::from_locale("xx_YY.UTF-8"),
            None,
            "unknown: the caller's default"
        );
        assert_eq!(Language::from_tag("FR"), Some(Language::French));
        assert_eq!(Language::from_tag("french"), Some(Language::French));
        assert_eq!(Language::from_tag("klingon"), None);
        assert_eq!(Language::French.code(), "fr");
        assert_eq!(Language::French.to_string(), "French");
    }

    #[test]
    fn the_language_setting_follows_the_machine_unless_the_configuration_fixes_one() {
        assert_eq!(LanguageSetting::parse("auto"), Ok(LanguageSetting::Auto));
        assert_eq!(
            LanguageSetting::parse("fr"),
            Ok(LanguageSetting::Fixed(Language::French))
        );
        let err = LanguageSetting::parse("xx").unwrap_err().to_string();
        assert!(
            err.contains("`xx`") && err.contains("auto") && err.contains("fr"),
            "{err}"
        );
        let auto = Settings::default();
        assert_eq!(auto.language(), Language::English, "nothing known: English");
        assert_eq!(
            auto.clone()
                .with_machine_language(Some(Language::French))
                .language(),
            Language::French
        );
        assert_eq!(
            auto.with_machine_language(None).language(),
            Language::English
        );
        let mut fixed = Settings::default();
        fixed.ui.language = LanguageSetting::Fixed(Language::German);
        assert_eq!(
            fixed
                .with_machine_language(Some(Language::French))
                .language(),
            Language::German,
            "the configuration wins over the machine"
        );
    }

    #[test]
    fn a_hotkey_is_one_control_letter_in_any_of_the_usual_notations() {
        for text in ["^O", "^o", "ctrl-o", "Ctrl+O", "C-o", " control-o "] {
            assert_eq!(Hotkey::parse(text), Ok(Hotkey('O')), "{text}");
        }
        let key = Hotkey::parse("^o").unwrap();
        assert_eq!(key.to_string(), "^O");
        assert_eq!(key.zsh(), "^O");
        assert_eq!(key.fish(), "\\co");
        assert_eq!(key.bash(), "\\C-o");
        assert_eq!(Hotkey::default(), Hotkey::parse("^K").unwrap());
    }

    #[test]
    fn what_is_not_a_control_letter_or_already_taken_is_refused_by_name() {
        for text in ["K", "^", "^KK", "^1", "^[", "alt-k", ""] {
            assert!(
                matches!(Hotkey::parse(text), Err(HotkeyError::NotAControlKey(_))),
                "{text}"
            );
        }
        assert_eq!(
            Hotkey::parse("^C"),
            Err(HotkeyError::Taken('C', "the interrupt"))
        );
        assert!(Hotkey::parse("^i").unwrap_err().to_string().contains("Tab"));
        assert!(
            Hotkey::parse("ctrl-m")
                .unwrap_err()
                .to_string()
                .contains("Enter")
        );
    }

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
        assert_eq!(s.daemon.sync_budget, Duration::from_millis(40));
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
