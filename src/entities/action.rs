//! The words of the bubble: what a user can do about a case.

use std::fmt;

/// One action on a case, as the bubble's words name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Why,
    Fix,
    Agent,
    Ignore,
    Privacy,
}

impl Action {
    pub const ALL: [Action; 5] = [
        Action::Why,
        Action::Fix,
        Action::Agent,
        Action::Ignore,
        Action::Privacy,
    ];

    /// The word, as typed after `kintsu` and as written in a link.
    pub fn name(self) -> &'static str {
        match self {
            Action::Why => "why",
            Action::Fix => "fix",
            Action::Agent => "agent",
            Action::Ignore => "ignore",
            Action::Privacy => "privacy",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.name() == name)
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_unknown_words_are_refused() {
        for action in Action::ALL {
            assert_eq!(Action::from_name(action.name()), Some(action));
        }
        assert_eq!(Action::from_name("dance"), None);
        assert_eq!(Action::Why.to_string(), "why");
    }
}
