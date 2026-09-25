//! The configuration file, validated at the edge into typed `Settings`.
//! Unknown keys are ignored so a file written for a later version loads.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

use crate::entities::{
    CaptureSettings, Duration, EagerFix, KeySource, ModelSpec, Provider, QuietSettings, Routing,
    Settings, Tier, UiMode, UiSettings,
};

/// The commented default file, also printed by `kintsu default-config`.
pub const DEFAULT_CONFIG: &str = include_str!("../../../config/default.toml");

/// The file `kintsu setup` writes: the models, the routing, the ui, in
/// the documented shape, with a short header. Parses back to the same
/// settings.
pub fn render_settings(settings: &Settings) -> String {
    let mut out = String::from(
        "# Kintsu configuration, written by `kintsu setup`.\n\
         # `kintsu default-config` prints every option with its comment.\n\n",
    );
    for m in &settings.models {
        out.push_str(&format!("[models.{}]\n", m.name));
        match m.provider {
            Provider::CliAgent => {
                out.push_str("provider = \"cli_agent\"\n");
                if crate::use_cases::setup::AGENT_PRESETS.contains(&m.model.as_str()) {
                    out.push_str(&format!("command  = {}\n", toml_string(&m.model)));
                } else {
                    out.push_str(&format!("template = {}\n", toml_string(&m.model)));
                }
            }
            provider => {
                let name = match provider {
                    Provider::Ollama => "ollama",
                    Provider::Anthropic => "anthropic",
                    _ => "openai_compatible",
                };
                out.push_str(&format!("provider = \"{name}\"\n"));
                out.push_str(&format!("model    = {}\n", toml_string(&m.model)));
                if let Some(url) = &m.base_url {
                    out.push_str(&format!("base_url = {}\n", toml_string(url)));
                }
                match &m.key {
                    KeySource::None => {}
                    KeySource::Env(var) => {
                        out.push_str(&format!("key      = {{ env = {} }}\n", toml_string(var)))
                    }
                    KeySource::Command(cmd) => out.push_str(&format!(
                        "key      = {{ command = {} }}\n",
                        toml_string(cmd)
                    )),
                    KeySource::Keychain(_) => out.push_str("key      = { keychain = true }\n"),
                    KeySource::Literal(lit) => out.push_str(&format!(
                        "key      = {{ literal = {} }}\n",
                        toml_string(lit)
                    )),
                }
            }
        }
        let tier = match m.tier {
            Tier::Tiny => "tiny",
            Tier::Small => "small",
            Tier::Large => "large",
            Tier::Agent => "agent",
        };
        out.push_str(&format!("tier     = \"{tier}\"\n\n"));
    }
    out.push_str("[routing]\n");
    for (task, names) in [
        ("quick_fix", &settings.routing.quick_fix),
        ("explain", &settings.routing.explain),
        ("investigate", &settings.routing.investigate),
    ] {
        let list: Vec<String> = names.iter().map(|n| toml_string(n)).collect();
        out.push_str(&format!("{task:<11} = [{}]\n", list.join(", ")));
    }
    out.push_str("\n[routing.constraints]\n");
    out.push_str(&format!(
        "sensitive_output = \"{}\"\n\n",
        if settings.sensitive_local_only {
            "local_only"
        } else {
            "allow"
        }
    ));
    out.push_str("[ui]\n");
    out.push_str(&format!(
        "mode      = \"{}\"\n",
        match settings.ui.mode {
            UiMode::Toast => "toast",
            UiMode::Hint => "hint",
            UiMode::Silent => "silent",
        }
    ));
    out.push_str(&format!("ascii     = {}\n", settings.ui.ascii));
    out.push_str(&format!(
        "eager_fix = {}\n",
        match settings.ui.eager_fix {
            EagerFix::Auto => "\"auto\"".to_string(),
            EagerFix::On => "true".to_string(),
            EagerFix::Off => "false".to_string(),
        }
    ));
    out
}

fn toml_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SettingsError {
    #[error("cannot read {0}")]
    Read(String),
    #[error("config: {0}")]
    Parse(String),
    #[error("config: {0}")]
    Invalid(String),
}

/// Reads the file; a missing file means the defaults.
pub fn load_settings(path: &Path, home: Option<&str>) -> Result<Settings, SettingsError> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_settings(&text, home),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(SettingsError::Read(format!("{}: {e}", path.display()))),
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct FileDto {
    models: toml::Table,
    routing: RoutingDto,
    quiet: QuietDto,
    ui: UiDto,
    capture: CaptureDto,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CaptureDto {
    sources: Option<Vec<String>>,
    max_lines: Option<usize>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ModelDto {
    provider: Option<String>,
    model: Option<String>,
    command: Option<String>,
    template: Option<String>,
    base_url: Option<String>,
    key: Option<KeyDto>,
    tier: Option<String>,
    timeout: Option<String>,
    max_output_tokens: Option<u32>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct KeyDto {
    env: Option<String>,
    command: Option<String>,
    literal: Option<String>,
    keychain: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RoutingDto {
    quick_fix: Vec<String>,
    explain: Vec<String>,
    investigate: Vec<String>,
    constraints: ConstraintsDto,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ConstraintsDto {
    sensitive_output: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct QuietDto {
    never_triage: Option<Vec<String>>,
    ok_statuses: Option<Vec<i32>>,
    ok_commands: Option<BTreeMap<String, Vec<i32>>>,
    same_failure: Option<String>,
    off_in: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct UiDto {
    mode: Option<String>,
    ascii: Option<bool>,
    eager_fix: Option<EagerDto>,
}

/// `eager_fix = true`, `false`, or `"auto"`.
#[derive(Deserialize)]
#[serde(untagged)]
enum EagerDto {
    Flag(bool),
    Word(String),
}

/// The pseudo-model the documentation allows in routing lists.
const RULES_ONLY: &str = "rules-only";

/// Parses the file text. `home` expands `~` in directories.
pub fn parse_settings(text: &str, home: Option<&str>) -> Result<Settings, SettingsError> {
    let file: FileDto =
        toml::from_str(text).map_err(|e| SettingsError::Parse(e.message().to_string()))?;
    let mut models = Vec::new();
    for (name, value) in file.models {
        let dto: ModelDto = value.try_into().map_err(|e: toml::de::Error| {
            SettingsError::Invalid(format!("models.{name}: {}", e.message()))
        })?;
        models.push(model_spec(&name, dto)?);
    }
    let strip = |names: Vec<String>| -> Result<Vec<String>, SettingsError> {
        let names: Vec<String> = names.into_iter().filter(|n| n != RULES_ONLY).collect();
        for n in &names {
            if !models.iter().any(|m: &ModelSpec| &m.name == n) {
                return Err(SettingsError::Invalid(format!(
                    "routing names `{n}`, which is not under [models]"
                )));
            }
        }
        Ok(names)
    };
    let routing = Routing {
        quick_fix: strip(file.routing.quick_fix)?,
        explain: strip(file.routing.explain)?,
        investigate: strip(file.routing.investigate)?,
    };
    let sensitive_local_only = match file.routing.constraints.sensitive_output.as_deref() {
        None | Some("local_only") => true,
        Some("allow") => false,
        Some(other) => {
            return Err(SettingsError::Invalid(format!(
                "routing.constraints.sensitive_output: `{other}` (local_only or allow)"
            )));
        }
    };
    let defaults = QuietSettings::default();
    let quiet = QuietSettings {
        never_triage: file.quiet.never_triage.unwrap_or(defaults.never_triage),
        ok_statuses: file.quiet.ok_statuses.unwrap_or(defaults.ok_statuses),
        ok_commands: file
            .quiet
            .ok_commands
            .map(|m| m.into_iter().collect())
            .unwrap_or(defaults.ok_commands),
        same_failure_once: match file.quiet.same_failure.as_deref() {
            None | Some("once") => true,
            Some("always") => false,
            Some(other) => {
                return Err(SettingsError::Invalid(format!(
                    "quiet.same_failure: `{other}` (once or always)"
                )));
            }
        },
        off_in: file
            .quiet
            .off_in
            .unwrap_or_default()
            .iter()
            .map(|d| expand_dir(d, home))
            .collect(),
    };
    let ui = UiSettings {
        mode: match file.ui.mode.as_deref() {
            None | Some("toast") | Some("panel") => UiMode::Toast,
            Some("hint") => UiMode::Hint,
            Some("silent") => UiMode::Silent,
            Some(other) => {
                return Err(SettingsError::Invalid(format!(
                    "ui.mode: `{other}` (toast, hint or silent)"
                )));
            }
        },
        ascii: file.ui.ascii.unwrap_or(false),
        eager_fix: match file.ui.eager_fix {
            None => EagerFix::Auto,
            Some(EagerDto::Flag(true)) => EagerFix::On,
            Some(EagerDto::Flag(false)) => EagerFix::Off,
            Some(EagerDto::Word(w)) if w == "auto" => EagerFix::Auto,
            Some(EagerDto::Word(other)) => {
                return Err(SettingsError::Invalid(format!(
                    "ui.eager_fix: `{other}` (auto, true or false)"
                )));
            }
        },
    };
    let defaults = CaptureSettings::default();
    let capture = CaptureSettings {
        sources: match file.capture.sources {
            None => defaults.sources,
            Some(list) => {
                if let Some(bad) = list.iter().find(|s| !defaults.sources.contains(s)) {
                    return Err(SettingsError::Invalid(format!(
                        "capture.sources: `{bad}` (herdr, tmux, wezterm, kitty, iterm2)"
                    )));
                }
                list
            }
        },
        max_lines: file.capture.max_lines.unwrap_or(defaults.max_lines),
    };
    Ok(Settings {
        models,
        routing,
        quiet,
        ui,
        capture,
        sensitive_local_only,
    })
}

fn model_spec(name: &str, dto: ModelDto) -> Result<ModelSpec, SettingsError> {
    let invalid = |msg: String| SettingsError::Invalid(format!("models.{name}: {msg}"));
    let provider_name = dto
        .provider
        .as_deref()
        .ok_or_else(|| invalid("provider is required".into()))?;
    let (provider, default_url, default_key) = match provider_name {
        "ollama" => (
            Provider::Ollama,
            Some("http://127.0.0.1:11434"),
            KeySource::None,
        ),
        "openai_compatible" | "openai" => (
            Provider::OpenAiCompatible,
            Some("https://api.openai.com/v1"),
            KeySource::Env("OPENAI_API_KEY".into()),
        ),
        "gemini" => (
            Provider::OpenAiCompatible,
            Some("https://generativelanguage.googleapis.com/v1beta/openai"),
            KeySource::Env("GEMINI_API_KEY".into()),
        ),
        "anthropic" => (
            Provider::Anthropic,
            Some("https://api.anthropic.com"),
            KeySource::Env("ANTHROPIC_API_KEY".into()),
        ),
        "cli_agent" | "cli" => (Provider::CliAgent, None, KeySource::None),
        other => {
            return Err(invalid(format!(
                "unknown provider `{other}` (ollama, openai_compatible, anthropic, gemini, cli_agent)"
            )));
        }
    };
    let model = if provider == Provider::CliAgent {
        match (dto.template, dto.command) {
            (Some(template), _) => template,
            (None, Some(command)) => agent_preset(&command),
            (None, None) => {
                return Err(invalid(
                    "a cli_agent needs command = \"claude\" or a template".into(),
                ));
            }
        }
    } else {
        dto.model
            .ok_or_else(|| invalid("model is required".into()))?
    };
    let key =
        match dto.key {
            None => default_key,
            Some(KeyDto { env: Some(var), .. }) => KeySource::Env(var),
            Some(KeyDto {
                command: Some(cmd), ..
            }) => KeySource::Command(cmd),
            Some(KeyDto {
                literal: Some(lit), ..
            }) => KeySource::Literal(lit),
            Some(KeyDto {
                keychain: Some(true),
                ..
            }) => KeySource::Keychain(name.to_string()),
            Some(_) => return Err(invalid(
                "key must be { env = … }, { command = … }, { keychain = true } or { literal = … }"
                    .into(),
            )),
        };
    let tier = match dto.tier.as_deref() {
        None => {
            if provider == Provider::CliAgent {
                Tier::Agent
            } else {
                Tier::Small
            }
        }
        Some("tiny") => Tier::Tiny,
        Some("small") => Tier::Small,
        Some("large") => Tier::Large,
        Some("agent") => Tier::Agent,
        Some(other) => {
            return Err(invalid(format!(
                "unknown tier `{other}` (tiny, small, large, agent)"
            )));
        }
    };
    let timeout = match dto.timeout {
        None => None,
        Some(text) => Some(
            parse_timeout(&text)
                .ok_or_else(|| invalid(format!("timeout `{text}` (try 8s, 500ms, 2m)")))?,
        ),
    };
    Ok(ModelSpec {
        name: name.to_string(),
        provider,
        model,
        base_url: dto
            .base_url
            .or_else(|| default_url.map(String::from))
            .map(|u| u.trim_end_matches('/').to_string()),
        key,
        tier,
        timeout,
        max_output_tokens: dto.max_output_tokens,
    })
}

/// How each known CLI takes an initial prompt; `{brief}` is a file path.
/// Checked against each CLI's own reference; `shell_agents` launches
/// every one of them in its tests.
pub(crate) fn agent_preset(command: &str) -> String {
    match command {
        "claude" => "claude \"$(cat {brief})\"".into(),
        "codex" => "codex \"$(cat {brief})\"".into(),
        "opencode" => "opencode --prompt \"$(cat {brief})\"".into(),
        "aider" => "aider --message-file {brief}".into(),
        "gemini" => "gemini -i \"$(cat {brief})\"".into(),
        "copilot" => "copilot -i \"$(cat {brief})\"".into(),
        other => format!("{other} \"$(cat {{brief}})\""),
    }
}

fn parse_timeout(text: &str) -> Option<Duration> {
    let text = text.trim();
    let (number, unit) = text.split_at(text.find(|c: char| !c.is_ascii_digit())?);
    let n: u64 = number.parse().ok()?;
    Some(match unit {
        "ms" => Duration::from_millis(n),
        "s" => Duration::from_secs(n),
        "m" => Duration::from_mins(n),
        _ => return None,
    })
}

fn expand_dir(dir: &str, home: Option<&str>) -> String {
    let dir = dir.trim_end_matches("/**").trim_end_matches('/');
    match (dir.strip_prefix("~"), home) {
        (Some(rest), Some(home)) => format!("{home}{rest}"),
        _ => dir.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_setup_renders_parses_back_to_the_same_settings() {
        use crate::use_cases::setup::{CloudChoice, CloudKind, SetupAnswers, compose};
        let answers = SetupAnswers {
            local_model: Some("qwen2.5-coder:7b".into()),
            cloud: Some(CloudChoice {
                kind: CloudKind::Anthropic,
                model: "claude-haiku-4-5-20251001".into(),
                key: KeySource::Keychain("claude".into()),
            }),
            agent: Some("codex".into()),
        };
        let settings = compose(&answers);
        let text = render_settings(&settings);
        let parsed = parse_settings(&text, None).unwrap();
        let mut expected = settings.clone();
        expected
            .models
            .iter_mut()
            .find(|m| m.name == "codex-cli")
            .unwrap()
            .model = "codex \"$(cat {brief})\"".into();
        assert_eq!(parsed, expected, "{text}");
        assert!(
            text.contains("[models.local]\nprovider = \"ollama\"\nmodel    = \"qwen2.5-coder:7b\""),
            "{text}"
        );
        assert!(text.contains("key      = { keychain = true }"));
        assert!(text.contains("command  = \"codex\""));
        assert!(text.contains("explain     = [\"claude\", \"local\"]"));
        let bare = render_settings(&Settings::default());
        assert_eq!(
            parse_settings(&bare, None).unwrap(),
            Settings::default(),
            "{bare}"
        );
    }

    #[test]
    fn the_default_file_routes_the_local_model_and_keeps_the_other_defaults() {
        let s = parse_settings(DEFAULT_CONFIG, Some("/home/me")).unwrap();
        let local = s.model("local").unwrap();
        assert_eq!(
            (local.provider, local.model.as_str(), local.tier),
            (Provider::Ollama, "qwen2.5-coder:7b", Tier::Large)
        );
        assert_eq!(local.base_url.as_deref(), Some("http://127.0.0.1:11434"));
        assert_eq!(s.models.len(), 1);
        assert_eq!(s.routing.quick_fix, vec!["local"]);
        assert_eq!(s.routing.explain, vec!["local"]);
        assert!(s.routing.investigate.is_empty());
        let defaults = Settings::default();
        assert_eq!(
            (s.quiet, s.ui, s.sensitive_local_only),
            (defaults.quiet, defaults.ui, defaults.sensitive_local_only)
        );
    }

    #[test]
    fn models_get_their_provider_defaults_and_presets() {
        let text = r#"
[models.local]
provider = "ollama"
model = "qwen2.5-coder:7b"
tier = "tiny"

[models.haiku]
provider = "anthropic"
model = "claude-haiku-4-5-20251001"
key = { keychain = true }
timeout = "8s"
max_output_tokens = 800

[models.router]
provider = "openai_compatible"
base_url = "https://openrouter.ai/api/v1/"
model = "x"
key = { command = "op read op://dev/key" }
tier = "large"

[models.g]
provider = "gemini"
model = "gemini-2.5-flash"

[models.claude-code]
provider = "cli_agent"
command = "claude"

[models.mine]
provider = "cli_agent"
template = "myagent --task-file {brief}"
tier = "agent"

[routing]
classify = ["rules-only"]
quick_fix = ["local", "rules-only"]
explain = ["haiku", "local"]
investigate = ["claude-code"]
"#;
        let s = parse_settings(text, None).unwrap();
        let m = |n: &str| s.model(n).unwrap();
        assert_eq!(
            m("local").base_url.as_deref(),
            Some("http://127.0.0.1:11434")
        );
        assert_eq!(
            (m("local").tier, &m("local").key),
            (Tier::Tiny, &KeySource::None)
        );
        assert_eq!(m("haiku").key, KeySource::Keychain("haiku".into()));
        assert_eq!(m("haiku").timeout, Some(Duration::from_secs(8)));
        assert_eq!(m("haiku").max_output_tokens, Some(800));
        assert_eq!(
            m("haiku").base_url.as_deref(),
            Some("https://api.anthropic.com")
        );
        assert_eq!(
            m("router").base_url.as_deref(),
            Some("https://openrouter.ai/api/v1")
        );
        assert_eq!(
            m("router").key,
            KeySource::Command("op read op://dev/key".into())
        );
        assert_eq!(m("g").provider, Provider::OpenAiCompatible);
        assert_eq!(m("g").key, KeySource::Env("GEMINI_API_KEY".into()));
        assert_eq!(m("claude-code").model, "claude \"$(cat {brief})\"");
        assert_eq!(m("claude-code").tier, Tier::Agent);
        assert_eq!(m("mine").model, "myagent --task-file {brief}");
        assert_eq!(s.routing.quick_fix, vec!["local"]);
        assert_eq!(s.routing.explain, vec!["haiku", "local"]);
        assert!(s.sensitive_local_only);
    }

    #[test]
    fn every_agent_preset_is_the_flag_its_cli_documents() {
        let expected = [
            ("claude", "claude \"$(cat {brief})\""),
            ("codex", "codex \"$(cat {brief})\""),
            ("opencode", "opencode --prompt \"$(cat {brief})\""),
            ("aider", "aider --message-file {brief}"),
            ("gemini", "gemini -i \"$(cat {brief})\""),
            ("copilot", "copilot -i \"$(cat {brief})\""),
            ("my-agent", "my-agent \"$(cat {brief})\""),
        ];
        for (command, line) in expected {
            assert_eq!(agent_preset(command), line, "{command}");
            let text = format!("[models.a]\nprovider = \"cli_agent\"\ncommand = \"{command}\"");
            assert_eq!(
                parse_settings(&text, None)
                    .unwrap()
                    .model("a")
                    .unwrap()
                    .model,
                line,
                "{command} through the file"
            );
        }
    }

    #[test]
    fn quiet_ui_and_constraints_are_read() {
        let text = r#"
[routing.constraints]
sensitive_output = "allow"
[quiet]
never_triage = ["make watch"]
ok_statuses = [1]
ok_commands = { npm = [1] }
same_failure = "always"
off_in = ["~/scratch/**", "/tmp/x/"]
[ui]
mode = "hint"
ascii = true
eager_fix = true
"#;
        let s = parse_settings(text, Some("/home/me")).unwrap();
        assert!(!s.sensitive_local_only);
        assert_eq!(s.quiet.never_triage, vec!["make watch"]);
        assert_eq!(s.quiet.ok_statuses, vec![1]);
        assert_eq!(s.quiet.ok_commands, vec![("npm".to_string(), vec![1])]);
        assert!(!s.quiet.same_failure_once);
        assert_eq!(s.quiet.off_in, vec!["/home/me/scratch", "/tmp/x"]);
        assert_eq!(
            s.ui,
            UiSettings {
                mode: UiMode::Hint,
                ascii: true,
                eager_fix: EagerFix::On
            }
        );
    }

    #[test]
    fn mistakes_are_named_precisely() {
        let err = |text: &str| parse_settings(text, None).unwrap_err().to_string();
        assert!(err("[models.x]\nmodel = \"m\"").contains("models.x: provider is required"));
        assert!(
            err("[models.x]\nprovider = \"cohere\"\nmodel = \"m\"")
                .contains("unknown provider `cohere`")
        );
        assert!(err("[models.x]\nprovider = \"ollama\"").contains("models.x: model is required"));
        assert!(err("[models.x]\nprovider = \"cli_agent\"").contains("needs command"));
        assert!(
            err("[models.x]\nprovider = \"ollama\"\nmodel = \"m\"\ntier = \"huge\"")
                .contains("unknown tier `huge`")
        );
        assert!(
            err("[models.x]\nprovider = \"ollama\"\nmodel = \"m\"\nkey = { vault = 1 }")
                .contains("key must be")
        );
        assert!(err("[routing]\nexplain = [\"ghost\"]").contains("routing names `ghost`"));
        assert!(err("[quiet]\nsame_failure = \"never\"").contains("quiet.same_failure"));
        assert!(err("[ui]\nmode = \"loud\"").contains("ui.mode"));
        assert!(err("[capture]\nsources = [\"screen\"]").contains("capture.sources"));
        let c = parse_settings(
            "[capture]\nsources = [\"tmux\"]\nmax_lines = 80\nstderr_tee = true",
            None,
        )
        .unwrap()
        .capture;
        assert_eq!((c.sources, c.max_lines), (vec!["tmux".to_string()], 80));
        assert!(err("[ui]\neager_fix = \"sometimes\"").contains("ui.eager_fix"));
        assert_eq!(
            parse_settings("[ui]\neager_fix = \"auto\"", None)
                .unwrap()
                .ui
                .eager_fix,
            EagerFix::Auto
        );
        assert_eq!(
            parse_settings("[ui]\neager_fix = false", None)
                .unwrap()
                .ui
                .eager_fix,
            EagerFix::Off
        );
        assert!(err("this is = not toml =").starts_with("config: "));
        assert!(
            err("[models.x]\nprovider = \"ollama\"\nmodel = \"m\"\ntimeout = \"soon\"")
                .contains("timeout `soon`")
        );
    }

    #[test]
    fn a_missing_file_means_the_defaults_and_unknown_keys_are_tolerated() {
        assert_eq!(
            load_settings(Path::new("/definitely/not/here.toml"), None).unwrap(),
            Settings::default()
        );
        let s = parse_settings("[daemon]\nsync_budget = \"40ms\"\n[ui]\nhotkey = \"ctrl-k\"\n[models.x]\nprovider = \"ollama\"\nmodel = \"m\"\nopen_in = \"pane\"", None).unwrap();
        assert_eq!(s.models.len(), 1);
    }
}
