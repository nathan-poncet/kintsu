//! `kintsu doctor`: is the hook active, are the models reachable in
//! principle, are the keys where the configuration says.

use crate::entities::{KeySource, Provider, SessionId, Settings};
use crate::use_cases::ports::{Environment, Secrets};

/// How a check went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Ok,
    Warning,
    Problem,
}

/// One line of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub subject: String,
    pub health: Health,
    pub detail: String,
}

/// Inspects the setup without calling anything.
pub struct Diagnose<'a> {
    pub settings: &'a Settings,
    pub secrets: &'a dyn Secrets,
    pub environment: &'a dyn Environment,
}

impl Diagnose<'_> {
    pub fn run(&self, session: Option<&SessionId>) -> Vec<Check> {
        let mut checks = vec![match session {
            Some(id) => check(
                "shell hook",
                Health::Ok,
                format!("active in this shell (session {})", id.as_str()),
            ),
            None => check(
                "shell hook",
                Health::Problem,
                "not active here: run `kintsu init <shell>` and restart the shell",
            ),
        }];
        if self.settings.models.is_empty() {
            checks.push(check(
                "models",
                Health::Warning,
                "none configured: rules only; add one in the config file",
            ));
        }
        let executables = self.environment.executables();
        for m in &self.settings.models {
            let subject = format!("model {}", m.name);
            checks.push(match m.provider {
                Provider::CliAgent => {
                    let program = m.model.split_whitespace().next().unwrap_or_default();
                    if executables.iter().any(|e| e == program) {
                        check(subject, Health::Ok, format!("`{program}` is installed"))
                    } else {
                        check(
                            subject,
                            Health::Problem,
                            format!("`{program}` is not on the PATH"),
                        )
                    }
                }
                _ => match (&m.key, m.is_local()) {
                    (KeySource::Literal(_), _) => check(
                        subject,
                        Health::Warning,
                        "key written in the config file; prefer env, command or keychain",
                    ),
                    (KeySource::Env(var), _) if self.secrets.lookup(&m.key).is_some() => {
                        check(subject, Health::Ok, format!("key found in ${var}"))
                    }
                    (KeySource::Env(var), _) => {
                        check(subject, Health::Problem, format!("${var} is not set"))
                    }
                    (KeySource::Command(cmd), _) if self.secrets.lookup(&m.key).is_some() => {
                        check(subject, Health::Ok, format!("key from `{cmd}`"))
                    }
                    (KeySource::Command(cmd), _) => {
                        check(subject, Health::Problem, format!("`{cmd}` gave no key"))
                    }
                    (KeySource::Keychain(account), _) if self.secrets.lookup(&m.key).is_some() => {
                        check(
                            subject,
                            Health::Ok,
                            format!("key in the keychain ({account})"),
                        )
                    }
                    (KeySource::Keychain(account), _) => check(
                        subject,
                        Health::Problem,
                        format!("no keychain entry kintsu/{account}"),
                    ),
                    (KeySource::None, true) => check(subject, Health::Ok, "local, no key needed"),
                    (KeySource::None, false) => {
                        check(subject, Health::Problem, "a remote model needs a key")
                    }
                },
            });
        }
        let routing = &self.settings.routing;
        for (task, names) in [
            ("explain", &routing.explain),
            ("quick_fix", &routing.quick_fix),
            ("investigate", &routing.investigate),
        ] {
            for name in names {
                if self.settings.model(name).is_none() {
                    checks.push(check(
                        format!("routing.{task}"),
                        Health::Problem,
                        format!("`{name}` is not a configured model"),
                    ));
                }
            }
        }
        checks
    }
}

fn check(subject: impl Into<String>, health: Health, detail: impl Into<String>) -> Check {
    Check {
        subject: subject.into(),
        health,
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{Routing, Tier};
    use crate::use_cases::testing::*;

    #[test]
    fn every_kind_of_setup_gets_a_verdict() {
        let mut cloud = spec("cloud", Provider::Anthropic, Tier::Small);
        cloud.key = KeySource::Env("ANTHROPIC_API_KEY".into());
        let mut bare = spec("bare", Provider::OpenAiCompatible, Tier::Small);
        bare.base_url = Some("https://api.example.com/v1".into());
        let mut literal = spec("literal", Provider::Anthropic, Tier::Small);
        literal.key = KeySource::Literal("sk-x".into());
        let mut claude = spec("claude", Provider::CliAgent, Tier::Agent);
        claude.model = "claude --permission-mode plan".into();
        let settings = Settings {
            models: vec![
                cloud,
                spec("local", Provider::Ollama, Tier::Small),
                bare,
                literal,
                claude,
                spec("codex", Provider::CliAgent, Tier::Agent),
            ],
            routing: Routing {
                explain: vec!["cloud".into(), "ghost".into()],
                ..Default::default()
            },
            ..Default::default()
        };
        let uc = Diagnose {
            settings: &settings,
            secrets: &MapSecrets::with(&[("ANTHROPIC_API_KEY", "k")]),
            environment: &FakeEnvironment::with_executables(&["claude"]),
        };
        let report = uc.run(Some(&SessionId::new("42")));
        let health = |subject: &str| {
            report
                .iter()
                .find(|c| c.subject == subject)
                .map(|c| c.health)
                .unwrap()
        };
        assert_eq!(health("shell hook"), Health::Ok);
        assert_eq!(health("model cloud"), Health::Ok);
        assert_eq!(health("model local"), Health::Ok);
        assert_eq!(health("model bare"), Health::Problem);
        assert_eq!(health("model literal"), Health::Warning);
        assert_eq!(health("model claude"), Health::Ok);
        assert_eq!(health("model codex"), Health::Problem);
        assert_eq!(health("routing.explain"), Health::Problem);
        assert!(report.iter().all(|c| c.subject != "models"));
    }

    #[test]
    fn no_hook_and_no_model_are_said_plainly() {
        let settings = Settings::default();
        let uc = Diagnose {
            settings: &settings,
            secrets: &MapSecrets::with(&[]),
            environment: &FakeEnvironment::with_executables(&[]),
        };
        let report = uc.run(None);
        assert_eq!(report[0].health, Health::Problem);
        assert_eq!(report[1].subject, "models");
        assert_eq!(report[1].health, Health::Warning);
    }
}
