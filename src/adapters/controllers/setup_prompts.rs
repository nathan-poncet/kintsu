//! The three questions of `kintsu setup`, asked on a terminal and read
//! back as typed answers. `--yes` takes every default without asking.

use std::io::{BufRead, Write};

use crate::entities::KeySource;
use crate::use_cases::setup::{CloudChoice, CloudKind, Detected, SetupAnswers};

/// The local model proposed when Ollama is there.
pub const DEFAULT_LOCAL_MODEL: &str = "qwen2.5-coder:7b";

pub struct Prompter<'a> {
    pub input: &'a mut dyn BufRead,
    pub output: &'a mut dyn Write,
    /// Every default, no question.
    pub assume_yes: bool,
}

impl Prompter<'_> {
    fn ask(&mut self, question: &str, default: &str) -> String {
        if self.assume_yes {
            return default.to_string();
        }
        let shown = if default.is_empty() {
            String::new()
        } else {
            format!(" [{default}]")
        };
        let _ = write!(self.output, "▎ {question}{shown} ");
        let _ = self.output.flush();
        let mut line = String::new();
        if self.input.read_line(&mut line).unwrap_or(0) == 0 {
            return default.to_string();
        }
        let answer = line.trim();
        if answer.is_empty() {
            default.to_string()
        } else {
            answer.to_string()
        }
    }

    fn yes(&mut self, question: &str, default_yes: bool) -> bool {
        let answer = self.ask(question, if default_yes { "Y/n" } else { "y/N" });
        match answer.to_ascii_lowercase().as_str() {
            "y" | "yes" => true,
            "n" | "no" => false,
            _ => default_yes,
        }
    }

    /// The three questions, shaped by what was detected.
    pub fn run(&mut self, detected: &Detected) -> SetupAnswers {
        let local_model = if detected.ollama_installed {
            let note = if detected.ollama_running {
                ""
            } else {
                " (the server is not running; start it with `ollama serve`)"
            };
            if self.yes(&format!("Use a local model with Ollama{note}?"), true) {
                Some(self.ask("Which model?", DEFAULT_LOCAL_MODEL))
            } else {
                None
            }
        } else {
            let _ = writeln!(
                self.output,
                "▎ Ollama is not installed; a local model keeps fixes and explanations on this machine (brew install ollama, or https://ollama.com)."
            );
            None
        };

        let cloud = match self
            .ask(
                "A cloud model too? (a)nthropic, (o)penai, (g)emini, or (n)one",
                "n",
            )
            .to_ascii_lowercase()
            .as_str()
        {
            "a" | "anthropic" => Some(CloudKind::Anthropic),
            "o" | "openai" => Some(CloudKind::OpenAi),
            "g" | "gemini" => Some(CloudKind::Gemini),
            _ => None,
        }
        .map(|kind| {
            let model = self.ask("Which model?", kind.default_model());
            let where_ = self.ask(
                "Where is the key? (e)nvironment variable or (k)eychain",
                "e",
            );
            let key = if where_.to_ascii_lowercase().starts_with('k') {
                let _ = writeln!(
                    self.output,
                    "▎ Store it once with:  security add-generic-password -s kintsu -a {} -w",
                    kind.name()
                );
                KeySource::Keychain(kind.name().to_string())
            } else {
                KeySource::Env(self.ask("Variable name?", kind.default_key_var()))
            };
            CloudChoice { kind, model, key }
        });

        let agent = if detected.agents.is_empty() {
            let _ = writeln!(
                self.output,
                "▎ No CLI agent found on the PATH (claude, codex, opencode, aider, gemini, copilot); `kintsu agent` will need one."
            );
            None
        } else {
            let list = detected.agents.join(", ");
            let answer = self.ask(
                &format!("Which agent for `kintsu agent`? ({list}, or none)"),
                &detected.agents[0],
            );
            detected.agents.iter().find(|a| **a == answer).cloned()
        };

        SetupAnswers {
            local_model,
            cloud,
            agent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn detected() -> Detected {
        Detected {
            ollama_installed: true,
            ollama_running: true,
            agents: vec!["claude".into(), "codex".into()],
        }
    }

    fn run(answers: &str, detected: &Detected, yes: bool) -> (SetupAnswers, String) {
        let mut input = Cursor::new(answers.to_string());
        let mut output = Vec::new();
        let result = Prompter {
            input: &mut input,
            output: &mut output,
            assume_yes: yes,
        }
        .run(detected);
        (result, String::from_utf8(output).unwrap())
    }

    #[test]
    fn empty_answers_take_every_default() {
        let (a, out) = run("\n\n\n\n", &detected(), false);
        assert_eq!(
            a,
            SetupAnswers {
                local_model: Some(DEFAULT_LOCAL_MODEL.into()),
                cloud: None,
                agent: Some("claude".into())
            }
        );
        assert!(out.contains("Use a local model with Ollama? [Y/n]"));
        assert!(out.contains("Which agent for `kintsu agent`? (claude, codex, or none) [claude]"));
    }

    #[test]
    fn typed_answers_are_read_and_the_keychain_is_explained() {
        let (a, out) = run("y\nllama3\na\n\nk\ncodex\n", &detected(), false);
        assert_eq!(a.local_model.as_deref(), Some("llama3"));
        let cloud = a.cloud.unwrap();
        assert_eq!(
            (cloud.kind, cloud.model.as_str(), cloud.key),
            (
                CloudKind::Anthropic,
                "claude-haiku-4-5-20251001",
                KeySource::Keychain("claude".into())
            )
        );
        assert!(out.contains("security add-generic-password -s kintsu -a claude -w"));
        assert_eq!(a.agent.as_deref(), Some("codex"));
    }

    #[test]
    fn no_ollama_and_no_agent_are_said_and_skipped_and_yes_asks_nothing() {
        let bare = Detected::default();
        let (a, out) = run("g\ngemini-2.5-pro\ne\nMY_KEY\n", &bare, false);
        assert_eq!(a.local_model, None);
        assert!(out.contains("Ollama is not installed"));
        let cloud = a.cloud.unwrap();
        assert_eq!(
            (cloud.kind, cloud.model.as_str(), cloud.key),
            (
                CloudKind::Gemini,
                "gemini-2.5-pro",
                KeySource::Env("MY_KEY".into())
            )
        );
        assert_eq!(a.agent, None);
        assert!(out.contains("No CLI agent found"));
        let (a, out) = run("", &detected(), true);
        assert_eq!(
            a,
            SetupAnswers {
                local_model: Some(DEFAULT_LOCAL_MODEL.into()),
                cloud: None,
                agent: Some("claude".into())
            }
        );
        assert!(out.is_empty(), "--yes asks nothing");
    }
}
