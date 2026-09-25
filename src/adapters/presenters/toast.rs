//! The bubble under a failed command: one sentence, one line of actions.

use crate::adapters::controllers::act_url;
use crate::entities::{Action, CaseId, Danger, Fix, Message, MessageBody, TriageDecision, UiMode};

use super::Style;

/// Width at which the command echo is cut.
const ECHO_WIDTH: usize = 60;

/// What to print under the failure, or nothing.
/// `ghost` says the shell will pre-type a safe fix on the next prompt.
pub fn toast(decision: &TriageDecision, style: &Style, ghost: bool) -> Option<String> {
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
            match outcome.failed_stage() {
                Some(_) => format!(
                    "{} exited {}{after} in {}.",
                    style.bold(outcome.program()),
                    outcome.status(),
                    echo
                ),
                None => format!("{} exited {}{after}.", style.bold(&echo), outcome.status()),
            }
        }
    };
    let dot = style.dot();
    let pretyped = ghost && fix.as_ref().is_some_and(Fix::is_ghostable);
    let word = |action: Action| word_for(case.id(), action, style);
    let (why, agent, ignore) = (word(Action::Why), word(Action::Agent), word(Action::Ignore));
    let actions = match fix {
        Some(_) if pretyped => format!("Tab to fix{dot}{why}{dot}{agent}{dot}{ignore}{dot}^K more"),
        Some(_) => format!("{why}{dot}{agent}{dot}{ignore}{dot}^K more"),
        None => format!(
            "{}{dot}{why}{dot}{agent}{dot}{ignore}{dot}^K more",
            word(Action::Fix)
        ),
    };
    Some(match style.mode {
        UiMode::Hint => {
            let key = match fix {
                Some(_) if pretyped => format!("{dot}Tab"),
                Some(_) => format!("{dot}^K"),
                None => String::new(),
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

/// A message that arrives later: a model's fix or explanation for a case.
/// A late one, landing once the shell looks at something else, names its
/// command and offers no keys: `^K` and the words belong to the current
/// failure.
pub fn message_toast(message: &Message, style: &Style) -> String {
    let dot = style.dot();
    let label = message
        .command()
        .filter(|_| message.is_late())
        .map(|command| format!("{}: ", style.bold(&style.abbreviate(command.as_str(), 40))))
        .unwrap_or_default();
    match message.body() {
        MessageBody::Fix(fix) => {
            let who = match fix.source() {
                crate::entities::FixSource::Model(name) => format!("{name}{dot}not verified"),
                crate::entities::FixSource::Rule(name) => format!("rule{dot}{name}"),
            };
            if message.is_late() {
                return style.line(&format!(
                    "{label}try {}?{} {}",
                    style.bold(fix.command().as_str()),
                    danger_note(fix, style),
                    style.dim(&format!("({who})"))
                ));
            }
            let sentence = format!(
                "Try {}?{} {}",
                style.bold(fix.command().as_str()),
                danger_note(fix, style),
                style.dim(&format!("({who})"))
            );
            let actions = format!(
                "{}{dot}{}{dot}^K more",
                word_for(message.case(), Action::Why, style),
                word_for(message.case(), Action::Agent, style)
            );
            format!(
                "{}\n{}",
                style.line(&sentence),
                style.line(&style.dim(&actions))
            )
        }
        MessageBody::Explanation { model, text } => {
            let mut out = style.lines(&format!("{label}{}", text.trim()));
            out.push('\n');
            out.push_str(&style.line(&style.dim(&format!("— {model}"))));
            out
        }
        MessageBody::Note(text) => style.line(&format!("{label}{}", style.dim(text))),
    }
}

/// The dim line under the bubble while a model is being asked; the answer
/// replaces it.
pub fn pending_line(model: &str, style: &Style) -> String {
    style.line(&style.dim(&format!("asking {model}{}", style.ellipsis())))
}

/// `kintsu why`, as a word the terminal may make clickable.
fn word_for(case: &CaseId, action: Action, style: &Style) -> String {
    style.link(&act_url(case, action), &format!("kintsu {action}"))
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
                &Style::PLAIN,
                false
            ),
            None
        );
        let silent = Style {
            mode: UiMode::Silent,
            ..Style::PLAIN
        };
        assert_eq!(toast(&offer("make", 2, None, None), &silent, false), None);
    }

    #[test]
    fn a_safe_fix_in_a_shell_that_pre_types_says_tab() {
        let text = toast(
            &offer("gti status", 127, None, Some("git status")),
            &Style::PLAIN,
            true,
        )
        .unwrap();
        assert_eq!(
            text,
            "| Did you mean git status?\n| Tab to fix - kintsu why - kintsu agent - kintsu ignore - ^K more"
        );
        let rough = toast(
            &offer("rm -rf buidl", 1, None, Some("rm -rf build")),
            &Style::PLAIN,
            true,
        )
        .unwrap();
        assert!(
            !rough.contains("Tab") && rough.ends_with("kintsu ignore - ^K more"),
            "destructive: never pre-typed, so no Tab: {rough}"
        );
        let hint = Style {
            mode: UiMode::Hint,
            ..Style::PLAIN
        };
        assert_eq!(
            toast(
                &offer("gti status", 127, None, Some("git status")),
                &hint,
                true
            )
            .unwrap(),
            "| Did you mean git status? - Tab"
        );
    }

    #[test]
    fn a_fix_is_a_question_and_a_key() {
        let text = toast(
            &offer("gti status", 127, None, Some("git status")),
            &Style::PLAIN,
            false,
        )
        .unwrap();
        assert_eq!(
            text,
            "| Did you mean git status?\n| kintsu why - kintsu agent - kintsu ignore - ^K more"
        );
    }

    #[test]
    fn without_a_fix_the_failure_is_stated_with_its_duration() {
        let text = toast(
            &offer("npm run build", 1, Some(12_000), None),
            &Style::PLAIN,
            false,
        )
        .unwrap();
        assert_eq!(
            text,
            "| npm run build exited 1 after 12 s.\n| kintsu fix - kintsu why - kintsu agent - kintsu ignore - ^K more"
        );
        let quick = toast(&offer("make", 2, Some(300), None), &Style::PLAIN, false).unwrap();
        assert!(
            quick.starts_with("| make exited 2.\n"),
            "sub-second durations are noise: {quick}"
        );
    }

    #[test]
    fn a_pipelines_failing_stage_is_named() {
        use crate::entities::{
            CaseId, CommandLine, CommandOutcome, ExitStatus, FailureCase, Timestamp,
        };
        let outcome = CommandOutcome::new(
            CommandLine::new("cat log | grep x").unwrap(),
            ExitStatus::new(1),
        )
        .in_pipeline(vec![ExitStatus::new(0), ExitStatus::new(2)]);
        let case = FailureCase::new(CaseId::new("c"), Timestamp::from_millis(0), outcome, None);
        let decision = TriageDecision::Offer {
            case: Box::new(case),
            fix: None,
        };
        let text = toast(&decision, &Style::PLAIN, false).unwrap();
        assert!(
            text.starts_with("| grep exited 2 in cat log | grep x.\n"),
            "{text}"
        );
    }

    #[test]
    fn a_destructive_fix_carries_a_red_warning_and_hint_mode_is_one_line() {
        let colour = Style {
            color: true,
            ascii: false,
            mode: UiMode::Toast,
            links: false,
        };
        let text = toast(
            &offer("rm -rf buidl", 1, None, Some("rm -rf build")),
            &colour,
            false,
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
        let text = toast(
            &offer("gti status", 127, None, Some("git status")),
            &hint,
            false,
        )
        .unwrap();
        assert_eq!(text, "| Did you mean git status? - ^K");
        assert_eq!(
            toast(&offer("make", 2, None, None), &hint, false).unwrap(),
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
            "| Try nvm use 22? (local - not verified)\n| kintsu why - kintsu agent - ^K more"
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
    fn a_late_message_names_its_command_and_offers_no_keys() {
        let about = CommandLine::new("git status").unwrap();
        let note = Message::new(
            CaseId::new("c"),
            Timestamp::from_millis(0),
            MessageBody::Note("local had no fix for this one.".into()),
        )
        .about(about.clone())
        .late();
        assert_eq!(
            message_toast(&note, &Style::PLAIN),
            "| git status: local had no fix for this one."
        );
        let fix = Message::new(
            CaseId::new("c"),
            Timestamp::from_millis(0),
            MessageBody::Fix(Fix::new(
                CommandLine::new("git init").unwrap(),
                Confidence::new(0.6),
                FixSource::Model("local".into()),
                "",
            )),
        )
        .about(about.clone())
        .late();
        assert_eq!(
            message_toast(&fix, &Style::PLAIN),
            "| git status: try git init? (local - not verified)"
        );
        let on_time = Message::new(
            CaseId::new("c"),
            Timestamp::from_millis(0),
            MessageBody::Note("local had no fix for this one.".into()),
        )
        .about(about);
        assert_eq!(
            message_toast(&on_time, &Style::PLAIN),
            "| local had no fix for this one.",
            "known command, but not late: no label"
        );
    }

    #[test]
    fn on_a_terminal_that_wants_them_the_words_are_links_to_the_case() {
        let linked = Style {
            color: true,
            ascii: false,
            mode: UiMode::Toast,
            links: true,
        };
        let text = toast(&offer("make", 2, None, None), &linked, false).unwrap();
        assert!(
            text.contains("\x1b]8;;kintsu://act?case=c&do=why\x1b\\kintsu why\x1b]8;;\x1b\\"),
            "{text:?}"
        );
        assert!(text.contains("kintsu://act?case=c&do=fix"));
        let late = Message::new(
            CaseId::new("c"),
            Timestamp::from_millis(0),
            MessageBody::Fix(Fix::new(
                CommandLine::new("nvm use 22").unwrap(),
                Confidence::new(0.6),
                FixSource::Model("local".into()),
                "",
            )),
        );
        assert!(message_toast(&late, &linked).contains("kintsu://act?case=c&do=agent"));
        let plain = toast(&offer("make", 2, None, None), &Style::PLAIN, false).unwrap();
        assert!(!plain.contains("kintsu://"), "no links without a terminal");
    }

    #[test]
    fn long_commands_are_abbreviated_in_the_echo() {
        let long = "cargo run --release -- --input some/very/long/path/to/a/file.json --output out.json --verbose";
        let text = toast(&offer(long, 1, None, None), &Style::PLAIN, false).unwrap();
        assert!(
            text.starts_with(
                "| cargo run --release -- --input some/very/long/path/to/a/fil... exited 1."
            ),
            "{text}"
        );
    }
}
