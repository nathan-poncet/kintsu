//! Which models may see a case, and asking them in order until one answers.

use crate::entities::{FailureCase, ModelSpec, Provider, Settings};
use crate::use_cases::ports::{ModelError, ModelGateway, Prompt, Secrets};

/// The models named for a task, in order, that are allowed to see this
/// case: never a CLI agent, and only local ones when the case holds a
/// secret and the user asked for that.
pub fn model_candidates<'a>(
    settings: &'a Settings,
    names: &[String],
    case: &FailureCase,
) -> Vec<&'a ModelSpec> {
    let local_only = settings.sensitive_local_only && case.is_sensitive();
    settings
        .candidates(names)
        .into_iter()
        .filter(|m| m.provider != Provider::CliAgent)
        .filter(|m| !local_only || m.is_local())
        .collect()
}

/// Whether some named model was dropped only because the case is sensitive.
pub fn excluded_for_sensitivity(settings: &Settings, names: &[String], case: &FailureCase) -> bool {
    settings.sensitive_local_only
        && case.is_sensitive()
        && settings
            .candidates(names)
            .iter()
            .any(|m| m.provider != Provider::CliAgent && !m.is_local())
}

/// The first model that answers, with its name; otherwise every failure.
#[allow(clippy::type_complexity)]
pub fn ask_first(
    models: &dyn ModelGateway,
    secrets: &dyn Secrets,
    candidates: &[&ModelSpec],
    prompt: &Prompt,
) -> Result<(String, String), Vec<(String, ModelError)>> {
    let mut failures = Vec::new();
    for spec in candidates {
        let key = secrets.lookup(&spec.key);
        match models.complete(spec, key.as_deref(), prompt) {
            Ok(answer) if !answer.trim().is_empty() => return Ok((spec.name.clone(), answer)),
            Ok(_) => failures.push((
                spec.name.clone(),
                ModelError::Malformed("empty answer".into()),
            )),
            Err(e) => failures.push((spec.name.clone(), e)),
        }
    }
    Err(failures)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{KeySource, Tier};
    use crate::use_cases::testing::{MapSecrets, ScriptedModels, case, spec};

    fn settings() -> Settings {
        let mut cloud = spec("cloud", Provider::Anthropic, Tier::Small);
        cloud.key = KeySource::Env("ANTHROPIC_API_KEY".into());
        Settings {
            models: vec![
                cloud,
                spec("local", Provider::Ollama, Tier::Small),
                spec("claude", Provider::CliAgent, Tier::Agent),
            ],
            ..Default::default()
        }
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_sensitive_case_only_reaches_local_models_and_agents_are_never_models() {
        let s = settings();
        let plain = case("gti status", 127, None);
        let secret = case(
            "curl -H 'Authorization: Bearer sk-live-abcdefghijklmnop' https://x",
            22,
            None,
        );
        let order = names(&["cloud", "local", "claude"]);
        let pick = |c: &FailureCase| {
            model_candidates(&s, &order, c)
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(pick(&plain), vec!["cloud", "local"]);
        assert_eq!(pick(&secret), vec!["local"]);
        assert!(excluded_for_sensitivity(&s, &order, &secret));
        assert!(!excluded_for_sensitivity(&s, &order, &plain));
        let mut relaxed = settings();
        relaxed.sensitive_local_only = false;
        assert_eq!(model_candidates(&relaxed, &order, &secret).len(), 2);
    }

    #[test]
    fn models_are_asked_in_order_until_one_answers_with_their_key() {
        let s = settings();
        let models = ScriptedModels::answering(&[
            ("cloud", Err(ModelError::Unreachable("timeout".into()))),
            ("local", Ok("because")),
        ]);
        let secrets = MapSecrets::with(&[("ANTHROPIC_API_KEY", "k-123")]);
        let candidates: Vec<&ModelSpec> =
            vec![s.model("cloud").unwrap(), s.model("local").unwrap()];
        let prompt = Prompt {
            system: "s".into(),
            user: "u".into(),
            max_tokens: 10,
        };
        let (name, answer) = ask_first(&models, &secrets, &candidates, &prompt).unwrap();
        assert_eq!((name.as_str(), answer.as_str()), ("local", "because"));
        assert_eq!(models.asked(), vec!["cloud", "local"]);
        assert_eq!(models.calls.borrow()[0].1.as_deref(), Some("k-123"));
        assert_eq!(models.calls.borrow()[1].1, None);
    }

    #[test]
    fn when_nobody_answers_every_failure_is_reported() {
        let s = settings();
        let models = ScriptedModels::answering(&[
            ("cloud", Ok("  ")),
            ("local", Err(ModelError::Refused("429".into()))),
        ]);
        let candidates: Vec<&ModelSpec> =
            vec![s.model("cloud").unwrap(), s.model("local").unwrap()];
        let prompt = Prompt {
            system: "s".into(),
            user: "u".into(),
            max_tokens: 10,
        };
        let failures =
            ask_first(&models, &MapSecrets::with(&[]), &candidates, &prompt).unwrap_err();
        assert_eq!(failures.len(), 2);
        assert_eq!(failures[0].1, ModelError::Malformed("empty answer".into()));
        assert_eq!(failures[1].1, ModelError::Refused("429".into()));
    }
}
