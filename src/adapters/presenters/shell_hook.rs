//! The integration script `kintsu init` prints for a shell.

use crate::entities::{Hotkey, Shell};

/// The script to `eval` or `source`, from `shell/` at the root of the
/// repository, with the state directory filled in so the hook can read
/// the markers the binary leaves, and the hotkey in the shell's own
/// notation for the bind line that opens the panel.
pub fn shell_hook(shell: Shell, state_dir: &str, hotkey: Hotkey) -> String {
    let (template, key) = match shell {
        Shell::Zsh => (include_str!("../../../shell/kintsu.zsh"), hotkey.zsh()),
        Shell::Bash => (include_str!("../../../shell/kintsu.bash"), hotkey.bash()),
        Shell::Fish => (include_str!("../../../shell/kintsu.fish"), hotkey.fish()),
    };
    template
        .replace("__KINTSU_STATE_DIR__", state_dir)
        .replace("__KINTSU_HOTKEY__", &key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook(shell: Shell, state_dir: &str) -> String {
        shell_hook(shell, state_dir, Hotkey::DEFAULT)
    }

    #[test]
    fn every_hook_reports_through_triage_and_knows_the_state_directory() {
        for shell in Shell::ALL {
            let text = hook(shell, "/home/me/.local/state/kintsu");
            assert!(text.contains("kintsu triage --status"), "{shell}");
            assert!(!text.contains("__KINTSU_"), "{shell}");
        }
        assert!(hook(Shell::Zsh, "/s").contains("/s/sessions/"));
        assert!(hook(Shell::Fish, "/s").contains("/s/sessions/"));
    }

    #[test]
    fn each_hook_uses_its_own_shells_mechanism() {
        assert!(hook(Shell::Zsh, "/s").contains("add-zsh-hook"));
        assert!(hook(Shell::Bash, "/s").contains("PROMPT_COMMAND"));
        assert!(hook(Shell::Fish, "/s").contains("fish_postexec"));
    }

    #[test]
    fn the_panel_is_bound_to_the_configured_key_in_each_shells_notation() {
        assert!(hook(Shell::Zsh, "/s").contains("bindkey '^K' __kintsu_panel_widget"));
        assert!(hook(Shell::Fish, "/s").contains("bind \\ck __kintsu_panel"));
        assert!(hook(Shell::Bash, "/s").contains(r#"bind -x '"\C-k": __kintsu_panel'"#));
        let ctrl_o = Hotkey::parse("^O").unwrap();
        assert!(
            shell_hook(Shell::Zsh, "/s", ctrl_o).contains("bindkey '^O' __kintsu_panel_widget")
        );
        let fish = shell_hook(Shell::Fish, "/s", ctrl_o);
        assert!(fish.contains("bind \\co __kintsu_panel"));
        assert!(
            fish.contains("bind -M insert \\co __kintsu_panel"),
            "vi mode too"
        );
        assert!(
            shell_hook(Shell::Bash, "/s", ctrl_o).contains(r#"bind -x '"\C-o": __kintsu_panel'"#)
        );
    }
}
