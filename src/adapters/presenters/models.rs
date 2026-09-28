//! `kintsu models`, `kintsu models test` and `kintsu login`: a table, a
//! probe report, a notice; plain or JSON. No key ever appears here.

use serde_json::{Value, json};

use crate::entities::KeySource;
use crate::use_cases::ports::ModelError;
use crate::use_cases::{KeyStatus, LoggedIn, ModelRow, Probe, Reach};

use super::Style;

fn key_words(status: &KeyStatus) -> String {
    match status {
        KeyStatus::NotNeeded => "no key needed".into(),
        KeyStatus::WrittenInConfig => "key written in the config file".into(),
        KeyStatus::Found(KeySource::Env(var)) => format!("key found in ${var}"),
        KeyStatus::Found(KeySource::Command(cmd)) => format!("key from `{cmd}`"),
        KeyStatus::Found(KeySource::Keychain(account)) => {
            format!("key in the keychain ({account})")
        }
        KeyStatus::Found(_) => "key found".into(),
        KeyStatus::Missing(KeySource::None) => "a remote model needs a key".into(),
        KeyStatus::Missing(source) => crate::use_cases::models::describe(source),
    }
}

fn key_source_name(status: &KeyStatus) -> Value {
    let source = match status {
        KeyStatus::NotNeeded => return Value::Null,
        KeyStatus::WrittenInConfig => "config".to_string(),
        KeyStatus::Found(source) | KeyStatus::Missing(source) => match source {
            KeySource::None => return Value::Null,
            KeySource::Env(var) => format!("env:{var}"),
            KeySource::Command(cmd) => format!("command:{cmd}"),
            KeySource::Keychain(account) => format!("keychain:{account}"),
            KeySource::Literal(_) => "config".to_string(),
        },
    };
    Value::String(source)
}

fn reach_words(reach: Reach) -> &'static str {
    match reach {
        Reach::Running => "server running",
        Reach::NotRunning => "server not running",
        Reach::Remote => "remote",
        Reach::OnPath => "on the PATH",
        Reach::NotOnPath => "not on the PATH",
    }
}

fn reach_name(reach: Reach) -> &'static str {
    match reach {
        Reach::Running => "running",
        Reach::NotRunning => "not_running",
        Reach::Remote => "remote",
        Reach::OnPath => "on_path",
        Reach::NotOnPath => "not_on_path",
    }
}

fn mark(row: &ModelRow, style: &Style) -> String {
    let key_ok = matches!(row.key, KeyStatus::Found(_) | KeyStatus::NotNeeded);
    let reach_ok = matches!(row.reach, Reach::Running | Reach::Remote | Reach::OnPath);
    match (key_ok, reach_ok, &row.key, row.reach) {
        (_, _, KeyStatus::Missing(_), _) | (_, _, _, Reach::NotOnPath) => {
            style.warn(if style.ascii { "x " } else { "✗ " })
        }
        (true, true, _, _) => (if style.ascii { "ok " } else { "✓ " }).to_string(),
        _ => style.warn("! "),
    }
}

/// One line per model: name, provider and model id, tier, key, reach.
pub fn models_table(rows: &[ModelRow], style: &Style) -> String {
    if rows.is_empty() {
        return style.line("No model configured: rules only. `kintsu setup` adds one.");
    }
    let dot = style.dot();
    let cells: Vec<[String; 5]> = rows
        .iter()
        .map(|row| {
            [
                row.name.clone(),
                format!("{}{dot}{}", row.provider.name(), row.model),
                row.tier.name().to_string(),
                key_words(&row.key),
                reach_words(row.reach).to_string(),
            ]
        })
        .collect();
    let width = |column: usize| {
        cells
            .iter()
            .map(|c| c[column].chars().count())
            .max()
            .unwrap_or(0)
    };
    let (w0, w1, w2, w3) = (width(0), width(1), width(2), width(3));
    rows.iter()
        .zip(&cells)
        .map(|(row, c)| {
            format!(
                "{}{:<w0$}  {:<w1$}  {:<w2$}  {:<w3$}  {}",
                mark(row, style),
                c[0],
                c[1],
                c[2],
                c[3],
                c[4]
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn models_json(rows: &[ModelRow]) -> String {
    let list: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({
                "name": row.name,
                "provider": row.provider.name(),
                "model": row.model,
                "tier": row.tier.name(),
                "key": {
                    "needed": !matches!(row.key, KeyStatus::NotNeeded),
                    "found": matches!(row.key, KeyStatus::Found(_) | KeyStatus::WrittenInConfig),
                    "source": key_source_name(&row.key),
                },
                "reach": reach_name(row.reach),
            })
        })
        .collect();
    Value::Array(list).to_string()
}

/// One line per model: how long the answer took, or why there was none.
pub fn probes_report(probes: &[Probe], style: &Style) -> String {
    let width = probes
        .iter()
        .map(|p| p.name.chars().count())
        .max()
        .unwrap_or(0);
    probes
        .iter()
        .map(|p| match &p.outcome {
            Ok(latency) => format!(
                "{}{:<width$}  answered in {latency}",
                if style.ascii { "ok " } else { "✓ " },
                p.name
            ),
            Err(ModelError::Unsupported(why)) => style.dim(&format!("- {:<width$}  {why}", p.name)),
            Err(e) => format!(
                "{}{:<width$}  {e}",
                style.warn(if style.ascii { "x " } else { "✗ " }),
                p.name
            ),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn probes_json(probes: &[Probe]) -> String {
    let list: Vec<Value> = probes
        .iter()
        .map(|p| match &p.outcome {
            Ok(latency) => json!({"name": p.name, "ok": true, "latency_ms": latency.as_millis(), "error": Value::Null}),
            Err(e) => json!({"name": p.name, "ok": false, "latency_ms": Value::Null, "error": e.to_string()}),
        })
        .collect();
    Value::Array(list).to_string()
}

/// What `kintsu login` says once the key is in the keychain.
pub fn login_notice(done: &LoggedIn, wrote_config: bool, style: &Style) -> String {
    let mut out = vec![style.line(&format!(
        "key for {} stored in the keychain (service kintsu, account {}).",
        style.bold(&done.model),
        done.model
    ))];
    if wrote_config {
        out.push(style.line(
            &style.dim("the configuration now reads it from there: key = { keychain = true }"),
        ));
    } else if done.configured {
        out.push(style.line(&style.dim("the configuration already reads it from there.")));
    } else {
        out.push(style.line(&style.dim(&format!(
            "set key = {{ keychain = true }} under [models.{}], or run kintsu login {} --write-config.",
            done.model, done.model
        ))));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{Duration, Provider, Tier};

    fn row(name: &str, key: KeyStatus, reach: Reach) -> ModelRow {
        ModelRow {
            name: name.into(),
            provider: Provider::Anthropic,
            model: "claude-haiku-4-5".into(),
            tier: Tier::Large,
            key,
            reach,
        }
    }

    #[test]
    fn the_table_marks_each_row_and_aligns_the_columns_without_any_key() {
        let rows = vec![
            row(
                "haiku",
                KeyStatus::Found(KeySource::Env("ANTHROPIC_API_KEY".into())),
                Reach::Remote,
            ),
            row(
                "vault",
                KeyStatus::Missing(KeySource::Keychain("vault".into())),
                Reach::Remote,
            ),
            ModelRow {
                name: "local".into(),
                provider: Provider::Ollama,
                model: "qwen2.5-coder:7b".into(),
                tier: Tier::Small,
                key: KeyStatus::NotNeeded,
                reach: Reach::NotRunning,
            },
            row("literal", KeyStatus::WrittenInConfig, Reach::Remote),
        ];
        let text = models_table(&rows, &Style::PLAIN);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            "ok haiku    anthropic - claude-haiku-4-5  large  key found in $ANTHROPIC_API_KEY  remote"
        );
        assert!(lines[1].starts_with("x vault"), "{}", lines[1]);
        assert!(lines[1].contains("no keychain entry kintsu/vault"));
        assert!(lines[2].starts_with("! local"), "{}", lines[2]);
        assert!(lines[2].contains("no key needed") && lines[2].ends_with("server not running"));
        assert!(lines[3].starts_with("! literal") && lines[3].contains("written in the config"));
        assert!(
            models_table(&[], &Style::PLAIN).contains("No model configured"),
            "an empty table says what to do"
        );
    }

    #[test]
    fn the_json_says_whether_a_key_is_needed_and_found_and_where_never_its_value() {
        let rows = vec![
            row("literal", KeyStatus::WrittenInConfig, Reach::Remote),
            row(
                "vault",
                KeyStatus::Found(KeySource::Keychain("vault".into())),
                Reach::Remote,
            ),
        ];
        let parsed: Value = serde_json::from_str(&models_json(&rows)).unwrap();
        assert_eq!(parsed[0]["key"]["needed"], true);
        assert_eq!(parsed[0]["key"]["found"], true);
        assert_eq!(parsed[0]["key"]["source"], "config");
        assert_eq!(parsed[1]["key"]["source"], "keychain:vault");
        assert_eq!(parsed[1]["reach"], "remote");
        assert_eq!(parsed[1]["provider"], "anthropic");
        assert_eq!(parsed[1]["tier"], "large");
    }

    #[test]
    fn probes_show_latency_errors_and_the_agents_left_out() {
        let probes = vec![
            Probe {
                name: "local".into(),
                outcome: Ok(Duration::from_millis(812)),
            },
            Probe {
                name: "vault".into(),
                outcome: Err(ModelError::MissingKey(
                    "no keychain entry kintsu/vault".into(),
                )),
            },
            Probe {
                name: "claude".into(),
                outcome: Err(ModelError::Unsupported("a CLI agent".into())),
            },
        ];
        let text = probes_report(&probes, &Style::PLAIN);
        assert_eq!(
            text,
            "ok local   answered in 0.81 s\nx vault   no key: no keychain entry kintsu/vault\n- claude  a CLI agent"
        );
        let parsed: Value = serde_json::from_str(&probes_json(&probes)).unwrap();
        assert_eq!(parsed[0]["latency_ms"], 812);
        assert_eq!(parsed[1]["ok"], false);
        assert!(parsed[1]["error"].as_str().unwrap().contains("no key"));
    }

    #[test]
    fn the_login_notice_says_what_to_set_unless_the_config_already_does() {
        let done = LoggedIn {
            model: "haiku".into(),
            configured: false,
        };
        let text = login_notice(&done, false, &Style::PLAIN);
        assert!(text.starts_with(
            "| key for haiku stored in the keychain (service kintsu, account haiku)."
        ));
        assert!(text.contains("set key = { keychain = true } under [models.haiku]"));
        let configured = LoggedIn {
            configured: true,
            ..done.clone()
        };
        assert!(login_notice(&configured, false, &Style::PLAIN).contains("already reads it"));
        assert!(login_notice(&done, true, &Style::PLAIN).contains("now reads it"));
    }
}
