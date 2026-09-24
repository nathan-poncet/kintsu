//! A suggested command, how sure we are, and whether it bites.

use std::fmt;

use crate::entities::{CommandLine, Danger};

/// How confident the source of a fix is, from 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Confidence(f32);

impl Confidence {
    /// Clamps into `0.0..=1.0`.
    pub fn new(value: f32) -> Self {
        Self(value.clamp(0.0, 1.0))
    }

    /// The value.
    pub fn value(self) -> f32 {
        self.0
    }

    /// Sure enough to be typed on the next prompt as ghost text.
    pub fn is_high(self) -> bool {
        self.0 >= 0.8
    }
}

/// Where a fix came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixSource {
    /// A built-in rule, by name.
    Rule(String),
    /// A model, by its configured name.
    Model(String),
}

/// One corrected command line.
#[derive(Debug, Clone, PartialEq)]
pub struct Fix {
    command: CommandLine,
    confidence: Confidence,
    danger: Danger,
    source: FixSource,
    rationale: String,
}

impl Fix {
    /// A fix; its danger is classified from the command itself.
    pub fn new(
        command: CommandLine,
        confidence: Confidence,
        source: FixSource,
        rationale: impl Into<String>,
    ) -> Self {
        let danger = crate::entities::classify_danger(command.as_str());
        Self {
            command,
            confidence,
            danger,
            source,
            rationale: rationale.into(),
        }
    }

    /// The corrected command line.
    pub fn command(&self) -> &CommandLine {
        &self.command
    }

    /// How sure the source is.
    pub fn confidence(&self) -> Confidence {
        self.confidence
    }

    /// Whether running it needs a warning.
    pub fn danger(&self) -> &Danger {
        &self.danger
    }

    /// Who proposed it.
    pub fn source(&self) -> &FixSource {
        &self.source
    }

    /// Why, in one sentence, for the bubble.
    pub fn rationale(&self) -> &str {
        &self.rationale
    }
}

impl fmt::Display for FixSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FixSource::Rule(name) => write!(f, "rule · {name}"),
            FixSource::Model(name) => write!(f, "model · {name}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_is_clamped_and_high_from_point_eight() {
        assert!(Confidence::new(1.7).is_high());
        assert!(!Confidence::new(-1.0).is_high());
        assert!(Confidence::new(0.8).is_high());
        assert!(!Confidence::new(0.79).is_high());
    }

    #[test]
    fn a_fix_classifies_its_own_danger() {
        let safe = Fix::new(
            CommandLine::new("git status").unwrap(),
            Confidence::new(0.95),
            FixSource::Rule("typo".into()),
            "gti is not on PATH",
        );
        let rough = Fix::new(
            CommandLine::new("rm -rf ./build").unwrap(),
            Confidence::new(0.95),
            FixSource::Rule("typo".into()),
            "",
        );
        assert_eq!(safe.danger(), &Danger::None);
        assert!(matches!(rough.danger(), Danger::Destructive(_)));
        assert_eq!(format!("{}", safe.source()), "rule · typo");
    }
}
