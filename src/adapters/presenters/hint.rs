//! The one dim line printed after a failure, until the real bubble exists.

use crate::entities::TriageDecision;

/// Longest command echoed back before it is abbreviated.
pub const COMMAND_WIDTH: usize = 60;

/// The line to print, or nothing when triage stayed quiet.
pub fn hint(decision: &TriageDecision) -> Option<String> {
    match decision {
        TriageDecision::Quiet(_) => None,
        TriageDecision::Offer(outcome) => Some(format!(
            "kintsu · `{}` exited {} · agent hand-off not wired yet, see docs/VISION.md",
            abbreviate(outcome.command().as_str(), COMMAND_WIDTH),
            outcome.status()
        )),
    }
}

/// One line, inner whitespace collapsed, cut with an ellipsis past `width`
/// characters.
fn abbreviate(text: &str, width: usize) -> String {
    let single_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.chars().count() <= width {
        return single_line;
    }
    let cut: String = single_line.chars().take(width - 1).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use crate::entities::{CommandLine, CommandOutcome, ExitStatus, QuietReason};

    use super::*;

    fn offer(command: &str, code: i32) -> TriageDecision {
        TriageDecision::Offer(CommandOutcome::new(
            CommandLine::new(command).unwrap(),
            ExitStatus::new(code),
        ))
    }

    #[test]
    fn a_quiet_decision_prints_nothing() {
        assert_eq!(hint(&TriageDecision::Quiet(QuietReason::Succeeded)), None);
    }

    #[test]
    fn an_offer_names_the_command_and_its_status() {
        let line = hint(&offer("gti status", 127)).unwrap();
        assert!(line.contains("`gti status`"), "{line}");
        assert!(line.contains("127"), "{line}");
    }

    #[test]
    fn a_long_or_multiline_command_is_squeezed_onto_one_short_line() {
        let command = format!("echo one \\\n  two {}", "x".repeat(100));
        let line = hint(&offer(&command, 1)).unwrap();
        assert!(!line.contains('\n'));
        assert!(line.contains("echo one \\ two xxx"), "{line}");
        assert!(line.contains('…'), "{line}");
    }
}
