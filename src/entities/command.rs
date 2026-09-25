//! The command line as the user typed it.

use std::fmt;

use thiserror::Error;

/// A command line the shell actually ran, kept exactly as typed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CommandLine(String);

/// Why a piece of text is not a command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CommandLineError {
    /// Nothing but whitespace: the shell ran nothing, there is nothing to triage.
    #[error("a command line cannot be blank")]
    Blank,
}

impl CommandLine {
    /// Keeps `text` as typed, trailing newline included; refuses blank text.
    pub fn new(text: impl Into<String>) -> Result<Self, CommandLineError> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(CommandLineError::Blank);
        }
        Ok(Self(text))
    }

    /// The text as typed.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The words, split on whitespace. Quoting is not interpreted: rules
    /// only need the program and the first arguments.
    pub fn words(&self) -> Vec<&str> {
        self.0.split_whitespace().collect()
    }

    /// The first word: the program the shell resolved, or tried to.
    pub fn program(&self) -> &str {
        self.words().first().copied().unwrap_or_default()
    }

    /// Everything after the program.
    pub fn arguments(&self) -> Vec<&str> {
        self.words().into_iter().skip(1).collect()
    }

    /// The pipeline's stages, split at the `|` between them: `||` is not
    /// a pipe, `|&` is one, and a `|` inside quotes belongs to its word.
    pub fn stages(&self) -> Vec<&str> {
        let text = &self.0;
        let bytes = text.as_bytes();
        let mut stages = Vec::new();
        let (mut start, mut i) = (0, 0);
        let (mut single, mut double) = (false, false);
        while i < bytes.len() {
            match bytes[i] {
                b'\\' if !single => i += 1,
                b'\'' if !double => single = !single,
                b'"' if !single => double = !double,
                b'|' if !single && !double => {
                    if bytes.get(i + 1) == Some(&b'|') {
                        i += 1;
                    } else {
                        stages.push(text[start..i].trim());
                        if bytes.get(i + 1) == Some(&b'&') {
                            i += 1;
                        }
                        start = i + 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        stages.push(text[start.min(text.len())..].trim());
        stages
    }

    /// The same line with its first word replaced.
    pub fn with_program(&self, program: &str) -> Self {
        let rest: Vec<&str> = self.arguments();
        if rest.is_empty() {
            Self(program.to_string())
        } else {
            Self(format!("{program} {}", rest.join(" ")))
        }
    }
}

impl fmt::Display for CommandLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_line_is_not_a_command() {
        for blank in ["", "   ", "\n\t "] {
            assert_eq!(CommandLine::new(blank), Err(CommandLineError::Blank));
        }
    }

    #[test]
    fn the_text_is_kept_exactly_as_typed() {
        let line = CommandLine::new("  git   status \n").unwrap();
        assert_eq!(line.as_str(), "  git   status \n");
    }

    #[test]
    fn the_program_is_the_first_word_and_the_rest_are_arguments() {
        let line = CommandLine::new("  gti status --short").unwrap();
        assert_eq!(line.program(), "gti");
        assert_eq!(line.arguments(), vec!["status", "--short"]);
    }

    #[test]
    fn a_pipeline_splits_into_stages_but_or_and_quoted_bars_do_not() {
        let stages = |t: &str| {
            CommandLine::new(t)
                .unwrap()
                .stages()
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        };
        assert_eq!(stages("gti status | head -1"), ["gti status", "head -1"]);
        assert_eq!(
            stages("make || echo 'a | b' | wc -l"),
            ["make || echo 'a | b'", "wc -l"]
        );
        assert_eq!(stages("cmd 2>&1 |& tee log"), ["cmd 2>&1", "tee log"]);
        assert_eq!(
            stages(r#"echo "x|y" | tr \| _"#),
            [r#"echo "x|y""#, r"tr \| _"]
        );
        assert_eq!(stages("ls"), ["ls"]);
    }

    #[test]
    fn replacing_the_program_keeps_the_arguments() {
        let line = CommandLine::new("gti status --short").unwrap();
        assert_eq!(line.with_program("git").as_str(), "git status --short");
        assert_eq!(
            CommandLine::new("gti")
                .unwrap()
                .with_program("git")
                .as_str(),
            "git"
        );
    }
}
