//! The shells Kintsu knows how to hook.

use std::fmt;

/// A shell with a `kintsu init` integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shell {
    /// zsh: `preexec` and `precmd` hooks.
    Zsh,
    /// bash: `PROMPT_COMMAND` and the history.
    Bash,
    /// fish: the `fish_postexec` event.
    Fish,
}

impl Shell {
    /// Whether the hook can pre-type a fix on the next prompt: zsh and fish
    /// have a line editor the hook can draw in; bash does not.
    pub fn supports_ghost_text(self) -> bool {
        matches!(self, Shell::Zsh | Shell::Fish)
    }

    /// Every supported shell, in the order the documentation lists them.
    pub const ALL: [Self; 3] = [Self::Zsh, Self::Bash, Self::Fish];

    /// Resolves the name a user types after `kintsu init`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "zsh" => Some(Self::Zsh),
            "bash" => Some(Self::Bash),
            "fish" => Some(Self::Fish),
            _ => None,
        }
    }

    /// The name the shell calls itself.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Zsh => "zsh",
            Self::Bash => "bash",
            Self::Fish => "fish",
        }
    }
}

impl fmt::Display for Shell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_shell_round_trips_through_its_name() {
        for shell in Shell::ALL {
            assert_eq!(Shell::from_name(shell.name()), Some(shell));
        }
    }

    #[test]
    fn an_unknown_shell_has_no_variant() {
        assert_eq!(Shell::from_name("powershell"), None);
    }
}
