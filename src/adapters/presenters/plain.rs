//! What the commands print: fix, why, agent, ignore, privacy, errors.

use crate::entities::{
    CaseDocument, Danger, FixSource, IgnoreEntry, IgnoreScope, IgnoreTarget, LearnedFix,
};
use crate::use_cases::{Explained, FixProposal, HandOffPlan};

use super::Style;

/// `kintsu fix`: the command, why, and a warning when it bites.
pub fn fix_report(proposal: &FixProposal, style: &Style) -> String {
    let Some(fix) = &proposal.fix else {
        let echo = style.abbreviate(proposal.case.outcome().command().as_str(), 60);
        let mut out = vec![style.line(&format!("No fix known for {}.", style.bold(&echo)))];
        if proposal.budget_reached {
            out.push(style.line(
                &style.dim("Today's model budget is spent: remote models wait for midnight UTC."),
            ));
        } else if proposal.failures.is_empty() {
            out.push(style.line(&style.dim(&format!(
                "kintsu why explains it{}kintsu agent hands it over.",
                style.dot()
            ))));
        } else {
            for (name, error) in &proposal.failures {
                out.push(style.line(&style.dim(&format!("{name}: {error}"))));
            }
        }
        return out.join("\n");
    };
    let mut out = vec![style.line(&style.bold(fix.command().as_str()))];
    let dot = style.dot();
    let source = match fix.source() {
        FixSource::Rule(name) if fix.confidence().is_high() => format!("rule{dot}{name}"),
        FixSource::Rule(name) => format!("rule{dot}{name}{dot}a guess"),
        FixSource::Model(name) => format!("model {name}{dot}not verified"),
    };
    out.push(style.line(&style.dim(&format!("{} ({source})", fix.rationale()))));
    match fix.danger() {
        Danger::None => {}
        Danger::NeedsPrivilege => {
            out.push(style.line(&style.dim("Runs as root: read it before Enter.")))
        }
        Danger::Destructive(why) => {
            out.push(style.line(&style.warn(&format!(
                "{} {why}: read it before Enter.",
                style.warning_sign()
            ))));
        }
    }
    out.push(style.line(&style.dim(&format!(
        "{} inserts it in your prompt; nothing runs until you press Enter.",
        style.hotkey
    ))));
    out.join("\n")
}

/// `kintsu fix --raw`: the command alone, for the shell binding.
pub fn raw_fix(proposal: &FixProposal) -> Option<String> {
    proposal
        .fix
        .as_ref()
        .map(|f| f.command().as_str().to_string())
}

/// `kintsu why`: the answer, then who said it.
pub fn explanation(e: &Explained, style: &Style) -> String {
    let mut out = style.lines(&e.text);
    let redacted = match e.redactions {
        0 => String::new(),
        1 => format!("{}1 secret redacted", style.dot()),
        n => format!("{}{n} secrets redacted", style.dot()),
    };
    out.push('\n');
    out.push_str(&style.line(&style.dim(&format!("— {}{redacted}", e.model))));
    out
}

/// `kintsu agent`: what is about to be sent, before the agent takes over.
pub fn hand_off_notice(plan: &HandOffPlan, style: &Style) -> String {
    let redacted = match plan.redactions {
        0 => String::new(),
        1 => " (redacted: 1 secret)".to_string(),
        n => format!(" (redacted: {n} secrets)"),
    };
    let mut parts = vec!["command", "status", "directory"];
    if plan.brief.contains("## Output") {
        parts.push("output");
    }
    if plan.brief.contains("## Earlier commands in this shell") {
        parts.push("recent commands");
    }
    let sent = parts.join(", ");
    let how = style.dim(&format!(
        "kintsu privacy shows the brief{}the agent uses its own login.",
        style.dot()
    ));
    format!(
        "{}\n{}",
        style.line(&format!(
            "Handing this to {}: {sent}{redacted}.",
            style.bold(&plan.agent.name)
        )),
        style.line(&how)
    )
}

/// `kintsu ignore` and `kintsu mute`: what is now quiet.
pub fn ignored(entry: &IgnoreEntry, style: &Style) -> String {
    let what = match entry.target() {
        IgnoreTarget::Everything => "Everything".to_string(),
        IgnoreTarget::Program(p) => style.bold(p),
        IgnoreTarget::Command(c) => style.bold(c),
    };
    let where_ = match entry.scope() {
        IgnoreScope::Everywhere => "everywhere.".to_string(),
        IgnoreScope::Directory(d) => format!("in {d} and below."),
        IgnoreScope::Session(_) => "in this shell.".to_string(),
        IgnoreScope::Until(_) => "until the mute ends.".to_string(),
    };
    let undo = style.dim(&format!(
        "Edit or delete the ignore list to undo{}kintsu config path shows where.",
        style.dot()
    ));
    format!(
        "{}\n{}",
        style.line(&format!("{what} stays quiet {where_}")),
        style.line(&undo)
    )
}

/// `kintsu privacy`: the document, verbatim, under a one-line header.
pub fn privacy_report(doc: &CaseDocument, style: &Style) -> String {
    let redacted = match doc.redactions {
        0 => "nothing was redacted".to_string(),
        1 => "1 secret redacted".to_string(),
        n => format!("{n} secrets redacted"),
    };
    format!(
        "{}\n\n{}",
        style.line(&format!(
            "This is what a model or an agent would receive ({redacted}):"
        )),
        doc.text.trim_end()
    )
}

/// An error, in the same voice.
pub fn error_line(message: &str, style: &Style) -> String {
    style.line(&format!("kintsu: {message}"))
}

/// `kintsu learned`: the fixes taken twice, most recently taken first.
pub fn learned_report(entries: &[LearnedFix], style: &Style) -> String {
    if entries.is_empty() {
        return style.line(&style.dim(
            "Nothing learned yet: a fix you take twice for the same failure becomes a rule.",
        ));
    }
    let mut lines: Vec<String> = entries
        .iter()
        .map(|entry| {
            let taken = match entry.acceptances() {
                1 => "taken once, a rule from twice".to_string(),
                n => format!("taken {n} times"),
            };
            style.line(&format!(
                "{} {}: {}{}{}",
                style.bold(entry.shape().line()),
                style.dim(&format!("exited {}", entry.shape().status().code())),
                entry.command(),
                style.dot(),
                style.dim(&taken)
            ))
        })
        .collect();
    lines.push(style.line(&style.dim("kintsu learned forget <program>, or --all, to unlearn.")));
    lines.join("\n")
}

/// `kintsu learned --json`: the same, for programs.
pub fn learned_json(entries: &[LearnedFix]) -> String {
    let items: Vec<serde_json::Value> = entries
        .iter()
        .map(|entry| {
            serde_json::json!({
                "command": entry.shape().line(),
                "status": entry.shape().status().code(),
                "fix": entry.command().as_str(),
                "acceptances": entry.acceptances(),
                "last_accepted_ms": entry.last_accepted().as_millis(),
                "rule": entry.as_rule().is_some(),
            })
        })
        .collect();
    serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".into())
}

/// `kintsu learned forget`: what went.
pub fn forgotten(count: usize, program: Option<&str>, style: &Style) -> String {
    let scope = program.map_or(String::new(), |p| format!(" for {}", style.bold(p)));
    let text = match count {
        0 => format!("Nothing learned{scope}."),
        1 => format!("Forgot 1 learned fix{scope}."),
        n => format!("Forgot {n} learned fixes{scope}."),
    };
    style.line(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{
        CaseId, CommandLine, CommandOutcome, Confidence, ExitStatus, FailureCase, Fix, KeySource,
        ModelSpec, Provider, Tier, Timestamp,
    };
    use crate::use_cases::ports::ModelError;

    fn case(text: &str) -> FailureCase {
        FailureCase::new(
            CaseId::new("c"),
            Timestamp::from_millis(0),
            CommandOutcome::new(CommandLine::new(text).unwrap(), ExitStatus::new(1)),
            None,
        )
    }

    fn fix(text: &str, source: FixSource) -> Fix {
        Fix::new(
            CommandLine::new(text).unwrap(),
            Confidence::new(0.9),
            source,
            "because",
        )
    }

    #[test]
    fn a_fix_report_names_the_command_the_source_and_the_danger() {
        let p = FixProposal {
            case: case("gti status"),
            fix: Some(fix("git status", FixSource::Rule("typo".into()))),
            failures: vec![],
            budget_reached: false,
        };
        let text = fix_report(&p, &Style::PLAIN);
        assert_eq!(
            text,
            "| git status\n| because (rule - typo)\n| ^K inserts it in your prompt; nothing runs until you press Enter."
        );
        assert_eq!(raw_fix(&p).as_deref(), Some("git status"));
        let rough = FixProposal {
            case: case("x"),
            fix: Some(fix("rm -rf build", FixSource::Model("local".into()))),
            failures: vec![],
            budget_reached: false,
        };
        let text = fix_report(&rough, &Style::PLAIN);
        assert!(text.contains("| because (model local - not verified)"));
        assert!(text.contains("| ! deletes recursively: read it before Enter."));
    }

    #[test]
    fn no_fix_says_so_and_lists_what_was_tried() {
        let none = FixProposal {
            case: case("make test"),
            fix: None,
            failures: vec![],
            budget_reached: false,
        };
        let capped = FixProposal {
            budget_reached: true,
            ..none.clone()
        };
        assert!(
            fix_report(&capped, &Style::PLAIN).contains("| Today's model budget is spent"),
            "{}",
            fix_report(&capped, &Style::PLAIN)
        );
        assert_eq!(
            fix_report(&none, &Style::PLAIN),
            "| No fix known for make test.\n| kintsu why explains it - kintsu agent hands it over."
        );
        assert_eq!(raw_fix(&none), None);
        let tried = FixProposal {
            failures: vec![(
                "local".into(),
                ModelError::Unreachable("connection refused".into()),
            )],
            ..none
        };
        assert!(
            fix_report(&tried, &Style::PLAIN).ends_with("| local: unreachable: connection refused")
        );
    }

    #[test]
    fn an_explanation_is_seamed_and_signed() {
        let e = Explained {
            case: case("x"),
            model: "haiku".into(),
            text: "Line one.\nLine two.".into(),
            redactions: 2,
        };
        assert_eq!(
            explanation(&e, &Style::PLAIN),
            "| Line one.\n| Line two.\n| — haiku - 2 secrets redacted"
        );
    }

    #[test]
    fn the_hand_off_notice_says_what_is_sent() {
        let agent = ModelSpec {
            name: "codex".into(),
            provider: Provider::CliAgent,
            model: "codex".into(),
            base_url: None,
            key: KeySource::None,
            tier: Tier::Agent,
            timeout: None,
            max_output_tokens: None,
        };
        let plan = HandOffPlan {
            case: CaseId::new("c"),
            agent,
            brief: "## Earlier commands in this shell".into(),
            redactions: 1,
        };
        let text = hand_off_notice(&plan, &Style::PLAIN);
        assert!(text.starts_with("| Handing this to codex: command, status, directory, recent commands (redacted: 1 secret)."));
    }

    #[test]
    fn learned_fixes_read_as_a_list_a_json_array_and_a_farewell() {
        use crate::entities::{CommandLine, ExitStatus, FailureShape, LearnedFix, Timestamp};
        assert!(learned_report(&[], &Style::PLAIN).contains("Nothing learned yet"));
        let once = LearnedFix::first(
            FailureShape::new("make test", ExitStatus::new(2)),
            CommandLine::new("make -j4 test").unwrap(),
            Timestamp::from_millis(1),
        );
        let twice = once.clone().accepted(Timestamp::from_millis(2));
        let report = learned_report(&[twice.clone(), once], &Style::PLAIN);
        assert!(
            report.starts_with("| make test exited 2: make -j4 test - taken 2 times\n"),
            "{report}"
        );
        assert!(report.contains("taken once, a rule from twice"));
        assert!(report.ends_with("to unlearn."));
        let json: serde_json::Value = serde_json::from_str(&learned_json(&[twice])).unwrap();
        assert_eq!(json[0]["fix"], "make -j4 test");
        assert_eq!(json[0]["acceptances"], 2);
        assert_eq!(json[0]["rule"], true);
        assert_eq!(forgotten(0, None, &Style::PLAIN), "| Nothing learned.");
        assert_eq!(
            forgotten(1, Some("make"), &Style::PLAIN),
            "| Forgot 1 learned fix for make."
        );
        assert_eq!(
            forgotten(3, None, &Style::PLAIN),
            "| Forgot 3 learned fixes."
        );
    }

    #[test]
    fn ignores_and_privacy_read_as_sentences() {
        let e = IgnoreEntry::new(
            IgnoreTarget::Program("make".into()),
            IgnoreScope::Directory("/w".into()),
        );
        assert!(ignored(&e, &Style::PLAIN).starts_with("| make stays quiet in /w and below."));
        let m = IgnoreEntry::mute_until(Timestamp::from_millis(5));
        assert!(
            ignored(&m, &Style::PLAIN).starts_with("| Everything stays quiet until the mute ends.")
        );
        let doc = CaseDocument {
            text: "## Command\n".into(),
            redactions: 0,
        };
        assert_eq!(
            privacy_report(&doc, &Style::PLAIN),
            "| This is what a model or an agent would receive (nothing was redacted):\n\n## Command"
        );
        assert_eq!(error_line("boom", &Style::PLAIN), "| kintsu: boom");
    }
}
