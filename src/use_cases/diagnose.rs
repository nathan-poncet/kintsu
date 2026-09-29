//! `kintsu doctor`: is the hook active, are the models reachable in
//! principle, are the keys where the configuration says.

use crate::entities::{KeySource, Provider, SessionId, Settings, Shell};
use crate::use_cases::ports::{Clock, CostLedger, Environment, ModelGateway, Secrets};
use crate::use_cases::routing::spent_today;

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
    pub models: &'a dyn ModelGateway,
    /// Whether the daemon is installed as a launchd agent or a systemd
    /// unit, so it runs without the shell's environment.
    pub service_installed: bool,
    /// The shell of the session the report is for, when it is known.
    pub shell: Option<Shell>,
    pub ledger: &'a dyn CostLedger,
    pub clock: &'a dyn Clock,
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
                    (KeySource::None, true) if !self.models.is_reachable(m) => check(

                        subject,

                        Health::Warning,

                        format!(

                            "local, but nothing answers at {}; start the server (ollama serve, or brew services start ollama)",

                            m.base_url.as_deref().unwrap_or("its address")

                        ),

                    ),

                    (KeySource::None, true) => check(subject, Health::Ok, "local, no key needed, server running"),
                    (KeySource::None, false) => {
                        check(subject, Health::Problem, "a remote model needs a key")
                    }
                },
            });
        }
        let from_env: Vec<String> = self
            .settings
            .models
            .iter()
            .filter_map(|m| match &m.key {
                KeySource::Env(var) => Some(format!("${var}")),
                _ => None,
            })
            .collect();
        if self.service_installed && !from_env.is_empty() {
            checks.push(check(
                "daemon",
                Health::Ok,
                format!(
                    "runs as a service, without your shell's environment: {} are read in the shell and forwarded with each command",
                    from_env.join(", ")
                ),
            ));
        }
        if self.settings.capture.stderr_tee {
            match self.shell {
                Some(shell) if shell.supports_stderr_tee() => checks.push(check(
                    "capture.stderr_tee",
                    Health::Ok,
                    format!("{shell} copies each command's stderr for the capture"),
                )),
                Some(shell) => checks.push(check(
                    "capture.stderr_tee",
                    Health::Warning,
                    format!("on, but {shell} cannot copy its own stderr: zsh and bash only"),
                )),
                None => {}
            }
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
        let spent = spent_today(self.ledger, self.clock);
        match self.settings.max_daily_cost {
            Some(cap) if spent >= cap => checks.push(check(
                "costs",
                Health::Warning,
                format!(
                    "{spent} today, cap {cap}: remote models wait for midnight UTC; `kintsu costs` details it"
                ),
            )),
            Some(cap) => checks.push(check(
                "costs",
                Health::Ok,
                format!("{spent} today of {cap}; `kintsu costs` details it"),
            )),
            None if spent > crate::entities::Money::ZERO => checks.push(check(
                "costs",
                Health::Ok,
                format!(
                    "{spent} today, no daily cap (routing.constraints.max_daily_cost); `kintsu costs` details it"
                ),
            )),
            None => {}
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
    use crate::entities::{Duration, LedgerEntry, Money, Task, Tokens};
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
            models: &ScriptedModels::default(),
            service_installed: false,
            shell: Some(Shell::Zsh),
            ledger: &MemoryLedger::default(),
            clock: &FakeClock::at(0),
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
        let mut stopped_models = ScriptedModels::default();
        stopped_models.down.push("local".into());
        let stopped = Diagnose {
            models: &stopped_models,
            ..uc
        };
        let report = stopped.run(Some(&SessionId::new("42")));
        let local = report.iter().find(|c| c.subject == "model local").unwrap();
        assert_eq!(local.health, Health::Warning);
        assert!(
            local.detail.contains("start the server"),
            "{}",
            local.detail
        );
        assert_eq!(health("model bare"), Health::Problem);
        assert_eq!(health("model literal"), Health::Warning);
        assert_eq!(health("model claude"), Health::Ok);
        assert_eq!(health("model codex"), Health::Problem);
        assert_eq!(health("routing.explain"), Health::Problem);
        assert!(report.iter().all(|c| c.subject != "models"));
    }

    #[test]
    fn a_service_daemon_says_the_shells_keys_are_forwarded() {
        let mut cloud = spec("cloud", Provider::Anthropic, Tier::Small);
        cloud.key = KeySource::Env("ANTHROPIC_API_KEY".into());
        let settings = Settings {
            models: vec![cloud, spec("local", Provider::Ollama, Tier::Small)],
            ..Default::default()
        };
        let uc = Diagnose {
            settings: &settings,
            secrets: &MapSecrets::with(&[("ANTHROPIC_API_KEY", "k")]),
            environment: &FakeEnvironment::with_executables(&[]),
            models: &ScriptedModels::default(),
            service_installed: true,
            shell: None,
            ledger: &MemoryLedger::default(),
            clock: &FakeClock::at(0),
        };
        let report = uc.run(Some(&SessionId::new("42")));
        let daemon = report.iter().find(|c| c.subject == "daemon").unwrap();
        assert_eq!(daemon.health, Health::Ok);
        assert!(
            daemon.detail.contains("$ANTHROPIC_API_KEY") && daemon.detail.contains("forwarded"),
            "{}",
            daemon.detail
        );
        let on_demand = Diagnose {
            service_installed: false,
            ..uc
        };
        assert!(
            on_demand.run(None).iter().all(|c| c.subject != "daemon"),
            "a daemon the shell starts inherits its environment"
        );
    }

    #[test]
    fn no_hook_and_no_model_are_said_plainly() {
        let settings = Settings::default();
        let uc = Diagnose {
            settings: &settings,
            secrets: &MapSecrets::with(&[]),
            environment: &FakeEnvironment::with_executables(&[]),
            models: &ScriptedModels::default(),
            service_installed: false,
            shell: None,
            ledger: &MemoryLedger::default(),
            clock: &FakeClock::at(0),
        };
        let report = uc.run(None);
        assert_eq!(report[0].health, Health::Problem);
        assert_eq!(report[1].subject, "models");
        assert_eq!(report[1].health, Health::Warning);
        assert!(
            report.iter().all(|c| c.subject != "costs"),
            "nothing spent, no cap: nothing to say"
        );
    }

    #[test]
    fn the_days_spend_is_reported_against_the_cap() {
        let ledger = MemoryLedger::default();
        let clock = FakeClock::at(1_790_637_207_000);
        ledger
            .record(&LedgerEntry {
                at: clock.now(),
                model: "cloud".into(),
                task: Task::Explain,
                tokens: Tokens::new(10, 10),
                cost: Some(Money::from_micro_usd(120_000)),
                latency: Duration::from_millis(1),
            })
            .unwrap();
        let capped = |micro: Option<u64>| Settings {
            max_daily_cost: micro.map(Money::from_micro_usd),
            ..Settings::default()
        };
        let costs = |settings: &Settings| {
            Diagnose {
                settings,
                secrets: &MapSecrets::with(&[]),
                environment: &FakeEnvironment::with_executables(&[]),
                models: &ScriptedModels::default(),
                ledger: &ledger,
                clock: &clock,
                service_installed: false,
                shell: None,
            }
            .run(None)
            .into_iter()
            .find(|c| c.subject == "costs")
            .unwrap()
        };
        let uncapped = costs(&capped(None));
        assert_eq!(uncapped.health, Health::Ok);
        assert!(
            uncapped.detail.contains("$0.12 today, no daily cap"),
            "{}",
            uncapped.detail
        );
        let within = costs(&capped(Some(1_000_000)));
        assert_eq!(within.health, Health::Ok);
        assert!(
            within.detail.contains("$0.12 today of $1.00"),
            "{}",
            within.detail
        );
        let reached = costs(&capped(Some(100_000)));
        assert_eq!(reached.health, Health::Warning);
        assert!(
            reached.detail.contains("midnight UTC"),
            "{}",
            reached.detail
        );
    }

    #[test]
    fn the_stderr_tee_is_for_zsh_and_bash_and_fish_is_told_so() {
        let mut settings = Settings::default();
        settings.capture.stderr_tee = true;
        let report_for = |settings: &Settings, shell: Option<Shell>| {
            Diagnose {
                settings,
                secrets: &MapSecrets::with(&[]),
                environment: &FakeEnvironment::with_executables(&[]),
                models: &ScriptedModels::default(),
                shell,
                service_installed: false,
                ledger: &MemoryLedger::default(),
                clock: &FakeClock::at(0),
            }
            .run(Some(&SessionId::new("42")))
            .into_iter()
            .find(|c| c.subject == "capture.stderr_tee")
        };
        assert_eq!(
            report_for(&settings, Some(Shell::Zsh)).unwrap().health,
            Health::Ok
        );
        assert_eq!(
            report_for(&settings, Some(Shell::Bash)).unwrap().health,
            Health::Ok
        );
        let fish = report_for(&settings, Some(Shell::Fish)).unwrap();
        assert_eq!(fish.health, Health::Warning);
        assert!(fish.detail.contains("zsh and bash only"), "{}", fish.detail);
        assert_eq!(report_for(&settings, None), None, "no shell to speak of");
        settings.capture.stderr_tee = false;
        assert_eq!(
            report_for(&settings, Some(Shell::Fish)),
            None,
            "off: nothing to say"
        );
    }
}
