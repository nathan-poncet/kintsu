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
}
