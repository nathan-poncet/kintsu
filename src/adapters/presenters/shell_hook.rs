//! The integration script `kintsu init` prints for a shell.

use crate::entities::Shell;

/// The script to `eval` or `source`, from `shell/` at the root of the
/// repository, with the state directory filled in so the hook can read
/// the marker `kintsu why` leaves.
pub fn shell_hook(shell: Shell, state_dir: &str) -> String {
    let template = match shell {
        Shell::Zsh => include_str!("../../../shell/kintsu.zsh"),
        Shell::Bash => include_str!("../../../shell/kintsu.bash"),
        Shell::Fish => include_str!("../../../shell/kintsu.fish"),
    };
    template.replace("__KINTSU_STATE_DIR__", state_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_hook_reports_through_triage_and_knows_the_state_directory() {
        for shell in Shell::ALL {
            let hook = shell_hook(shell, "/home/me/.local/state/kintsu");
            assert!(hook.contains("kintsu triage --status"), "{shell}");
            assert!(!hook.contains("__KINTSU_STATE_DIR__"), "{shell}");
        }
        assert!(shell_hook(Shell::Zsh, "/s").contains("/s/sessions/"));
        assert!(shell_hook(Shell::Fish, "/s").contains("/s/sessions/"));
    }

    #[test]
    fn each_hook_uses_its_own_shells_mechanism() {
        assert!(shell_hook(Shell::Zsh, "/s").contains("add-zsh-hook"));
        assert!(shell_hook(Shell::Bash, "/s").contains("PROMPT_COMMAND"));
        assert!(shell_hook(Shell::Fish, "/s").contains("fish_postexec"));
    }
}
