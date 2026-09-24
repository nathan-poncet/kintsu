//! The bubble under a failed command: one sentence, one line of actions.

use crate::entities::{Danger, Fix, Message, MessageBody, TriageDecision, UiMode};

use super::Style;

/// Width at which the command echo is cut.
const ECHO_WIDTH: usize = 60;

/// What to print under the failure, or nothing.
pub fn toast(decision: &TriageDecision, style: &Style) -> Option<String> {
    let TriageDecision::Offer { case, fix } = decision else {
        return None;
    };
    if style.mode == UiMode::Silent {
        return None;
    }
    let outcome = case.outcome();
    let echo = style.abbreviate(outcome.command().as_str(), ECHO_WIDTH);
    let sentence = match fix {
        Some(fix) => format!(
            "Did you mean {}?{}",
            style.bold(fix.command().as_str()),
            danger_note(fix, style)
        ),
        None => {
            let after = outcome
                .duration()
                .filter(|d| d.as_millis() >= 1_000)
                .map(|d| format!(" after {d}"))
                .unwrap_or_default();
            format!("{} exited {}{after}.", style.bold(&echo), outcome.status())
        }
    };
    let dot = style.dot();
    let actions = match fix {
        Some(_) => format!("^K to insert{dot}kintsu why{dot}kintsu agent{dot}kintsu ignore"),
        None => format!("kintsu fix{dot}kintsu why{dot}kintsu agent{dot}kintsu ignore"),
    };
    Some(match style.mode {
        UiMode::Hint => {
            let key = if fix.is_some() {
                format!("{dot}^K")
            } else {
                String::new()
            };
            style.line(&style.dim(&format!("{sentence}{key}")))
        }
        _ => format!(
            "{}\n{}",
            style.line(&sentence),
            style.line(&style.dim(&actions))
        ),
    })
}

/// A message that arrives later: a model's fix or explanation for a case
/// the shell already moved past.
pub fn message_toast(message: &Message, style: &Style) -> String {
    let dot = style.dot();
    match message.body() {
        MessageBody::Fix(fix) => {
            let who = match fix.source() {
                crate::entities::FixSource::Model(name) => format!("{name}{dot}not verified"),
                crate::entities::FixSource::Rule(name) => format!("rule{dot}{name}"),
            };
            let sentence = format!(
                "Try {}?{} {}",
                style.bold(fix.command().as_str()),
                danger_note(fix, style),
                style.dim(&format!("({who})"))
            );
            let actions = format!("^K to insert{dot}kintsu why{dot}kintsu agent");
            format!(
                "{}\n{}",
                style.line(&sentence),
                style.line(&style.dim(&actions))
            )
        }
        MessageBody::Explanation { model, text } => {
            let mut out = style.lines(text.trim());
            out.push('\n');
            out.push_str(&style.line(&style.dim(&format!("— {model}"))));
            out
        }
        MessageBody::Note(text) => style.line(&style.dim(text)),
    }
}

/// The dim line under the bubble while a model is being asked; the answer
/// replaces it.
pub fn pending_line(model: &str, style: &Style) -> String {
    style.line(&style.dim(&format!("asking {model}{}", style.ellipsis())))
}

fn danger_note(fix: &Fix, style: &Style) -> String {
    match fix.danger() {
        Danger::None => String::new(),
        Danger::NeedsPrivilege => format!(" {}", style.dim("(runs as root)")),
        Danger::Destructive(why) => format!(
            " {}",
            style.warn(&format!("{} {why}", style.warning_sign()))
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{
        CaseId, CommandLine, CommandOutcome, Confidence, Duration, ExitStatus, FailureCase,
        FixSource, QuietReason, Timestamp,
    };

    fn offer(text: &str, code: i32, duration_ms: Option<u64>, fix: Option<&str>) -> TriageDecision {
        let mut outcome =
            CommandOutcome::new(CommandLine::new(text).unwrap(), ExitStatus::new(code));
        if let Some(ms) = duration_ms {
            outcome = outcome.lasting(Duration::from_millis(ms));
        }
        let case = FailureCase::new(CaseId::new("c"), Timestamp::from_millis(0), outcome, None);
        let fix = fix.map(|f| {
            Fix::new(
                CommandLine::new(f).unwrap(),
                Confidence::new(0.9),
                FixSource::Rule("typo".into()),
                "because",
            )
        });
        TriageDecision::Offer {
            case: Box::new(case),
            fix,
        }
    }

    #[test]
    fn quiet_decisions_print_nothing() {
        assert_eq!(
            toast(
                &TriageDecision::Quiet(QuietReason::Succeeded),
                &Style::PLAIN
            ),
            None
        );
        let silent = Style {
            mode: UiMode::Silent,
            ..Style::PLAIN
        };
        assert_eq!(toast(&offer("make", 2, None, None), &silent), None);
    }

    #[test]
    fn a_fix_is_a_question_and_a_key() {
        let text = toast(
            &offer("gti status", 127, None, Some("git status")),
            &Style::PLAIN,
        )
        .unwrap();
        assert_eq!(
            text,
            "| Did you mean git status?\n| ^K to insert - kintsu why - kintsu agent - kintsu ignore"
        );
    }

    #[test]
    fn without_a_fix_the_failure_is_stated_with_its_duration() {
        let text = toast(
            &offer("npm run build", 1, Some(12_000), None),
            &Style::PLAIN,
        )
        .unwrap();
        assert_eq!(
            text,
            "| npm run build exited 1 after 12 s.\n| kintsu fix - kintsu why - kintsu agent - kintsu ignore"
        );
        let quick = toast(&offer("make", 2, Some(300), None), &Style::PLAIN).unwrap();
        assert!(
            quick.starts_with("| make exited 2.\n"),
            "sub-second durations are noise: {quick}"
        );
    }

    #[test]
    fn a_destructive_fix_carries_a_red_warning_and_hint_mode_is_one_line() {
        let colour = Style {
            color: true,
            ascii: false,
            mode: UiMode::Toast,
        };
        let text = toast(
            &offer("rm -rf buidl", 1, None, Some("rm -rf build")),
            &colour,
        )
        .unwrap();
        assert!(
            text.contains("\x1b[31m⚠ deletes recursively\x1b[0m"),
            "{text}"
        );
        let hint = Style {
            mode: UiMode::Hint,
            ..Style::PLAIN
        };
        let text = toast(&offer("gti status", 127, None, Some("git status")), &hint).unwrap();
        assert_eq!(text, "| Did you mean git status? - ^K");
        assert_eq!(
            toast(&offer("make", 2, None, None), &hint).unwrap(),
            "| make exited 2."
        );
    }

    #[test]
    fn a_message_reads_as_a_late_suggestion_or_an_explanation() {
        let fix = Fix::new(
            CommandLine::new("nvm use 22").unwrap(),
            Confidence::new(0.6),
            FixSource::Model("local".into()),
            "",
        );
        let m = Message::new(
            CaseId::new("c"),
            Timestamp::from_millis(0),
            MessageBody::Fix(fix),
        );
        assert_eq!(
            message_toast(&m, &Style::PLAIN),
            "| Try nvm use 22? (local - not verified)\n| ^K to insert - kintsu why - kintsu agent"
        );
        let e = Message::new(
            CaseId::new("c"),
            Timestamp::from_millis(0),
            MessageBody::Explanation {
                model: "haiku".into(),
                text: "Node is too old.\n".into(),
            },
        );
        assert_eq!(
            message_toast(&e, &Style::PLAIN),
            "| Node is too old.\n| — haiku"
        );
        let n = Message::new(
            CaseId::new("c"),
            Timestamp::from_millis(0),
            MessageBody::Note("haiku had no fix for this one.".into()),
        );
        assert_eq!(
            message_toast(&n, &Style::PLAIN),
            "| haiku had no fix for this one."
        );
        assert_eq!(pending_line("haiku", &Style::PLAIN), "| asking haiku...");
    }

    #[test]
    fn long_commands_are_abbreviated_in_the_echo() {
        let long = "cargo run --release -- --input some/very/long/path/to/a/file.json --output out.json --verbose";
        let text = toast(&offer(long, 1, None, None), &Style::PLAIN).unwrap();
        assert!(
            text.starts_with(
                "| cargo run --release -- --input some/very/long/path/to/a/fil... exited 1."
            ),
            "{text}"
        );
    }
}
