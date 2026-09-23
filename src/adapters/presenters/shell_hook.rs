//! The integration script `kintsu init` prints for a shell.

use crate::entities::Shell;

/// The script to `eval` or `source`, verbatim from `shell/` at the root of
/// the repository.
pub fn shell_hook(shell: Shell) -> &'static str {
    match shell {
        Shell::Zsh => include_str!("../../../shell/kintsu.zsh"),
        Shell::Bash => include_str!("../../../shell/kintsu.bash"),
        Shell::Fish => include_str!("../../../shell/kintsu.fish"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_hook_reports_through_triage() {
        for shell in Shell::ALL {
            assert!(
                shell_hook(shell).contains("kintsu triage --status"),
                "{shell}"
            );
        }
    }

    #[test]
    fn each_hook_uses_its_own_shells_mechanism() {
        assert!(shell_hook(Shell::Zsh).contains("add-zsh-hook"));
        assert!(shell_hook(Shell::Bash).contains("PROMPT_COMMAND"));
        assert!(shell_hook(Shell::Fish).contains("fish_postexec"));
    }
}
