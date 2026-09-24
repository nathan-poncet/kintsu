//! The daemon protocol, daemon to client: one JSON object per line.

use serde_json::json;

use crate::entities::{CaseId, QuietReason, TriageDecision};

const V: u64 = 1;

pub fn welcome(version: &str) -> String {
    json!({"v": V, "type": "welcome", "version": version}).to_string()
}

pub fn outdated(version: &str) -> String {
    json!({"v": V, "type": "outdated", "version": version}).to_string()
}

/// The answer to `command_finished`: the decision, the toast already
/// rendered for the client's terminal, and the bubbles it has not seen.
pub fn decision(
    decision: &TriageDecision,
    toast: Option<&str>,
    bubbles: &[String],
    pending: Option<&str>,
) -> String {
    match decision {
        TriageDecision::Quiet(reason) => {
            json!({"v": V, "type": "decision", "quiet": quiet_name(reason), "bubbles": bubbles}).to_string()
        }
        TriageDecision::Offer { case, fix } => json!({
            "v": V,
            "type": "decision",
            "offer": {"case": case.id().as_str(), "fix": fix.as_ref().map(|f| f.command().as_str())},
            "toast": toast,
            "bubbles": bubbles,
            "pending": pending,
        })
        .to_string(),
    }
}

fn quiet_name(reason: &QuietReason) -> String {
    match reason {
        QuietReason::Succeeded => "succeeded".into(),
        QuietReason::Interrupted => "interrupted".into(),
        QuietReason::AcceptedStatus => "accepted_status".into(),
        QuietReason::NeverTriaged(p) => format!("never_triaged:{p}"),
        QuietReason::Ignored => "ignored".into(),
        QuietReason::Duplicate => "duplicate".into(),
    }
}

/// A message, rendered, for the shell it belongs to.
pub fn bubble(case: &CaseId, text: &str) -> String {
    json!({"v": V, "type": "bubble", "case": case.as_str(), "text": text}).to_string()
}

pub fn ping() -> String {
    json!({"v": V, "type": "ping"}).to_string()
}

pub fn done() -> String {
    json!({"v": V, "type": "done"}).to_string()
}

pub fn ack() -> String {
    json!({"v": V, "type": "ack"}).to_string()
}

/// `ack` with the name of the model being asked in the background.
pub fn asked(model: &str) -> String {
    json!({"v": V, "type": "ack", "pending": model}).to_string()
}

pub fn bye() -> String {
    json!({"v": V, "type": "bye"}).to_string()
}

pub fn error(message: &str) -> String {
    json!({"v": V, "type": "error", "message": message}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{
        CaseId, CommandLine, CommandOutcome, ExitStatus, FailureCase, Timestamp,
    };
    use serde_json::Value;

    fn parse(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn decisions_carry_the_reason_or_the_offer_and_the_bubbles() {
        let quiet = parse(&decision(
            &TriageDecision::Quiet(QuietReason::NeverTriaged("vim".into())),
            None,
            &[],
            None,
        ));
        assert_eq!(quiet["type"], "decision");
        assert_eq!(quiet["quiet"], "never_triaged:vim");
        let case = FailureCase::new(
            CaseId::new("c1"),
            Timestamp::from_millis(0),
            CommandOutcome::new(CommandLine::new("make").unwrap(), ExitStatus::new(2)),
            None,
        );
        let offer = TriageDecision::Offer {
            case: Box::new(case),
            fix: None,
        };
        let v = parse(&decision(
            &offer,
            Some("▎ make exited 2."),
            &["▎ old".into()],
            Some("haiku"),
        ));
        assert_eq!(v["offer"]["case"], "c1");
        assert_eq!(v["pending"], "haiku");
        assert_eq!(v["offer"]["fix"], Value::Null);
        assert_eq!(v["toast"], "▎ make exited 2.");
        assert_eq!(v["bubbles"][0], "▎ old");
        assert_eq!(v["v"], 1);
    }

    #[test]
    fn small_frames_have_their_type() {
        for (frame, kind) in [
            (welcome("0.1.0"), "welcome"),
            (outdated("0.2.0"), "outdated"),
            (ping(), "ping"),
            (done(), "done"),
            (ack(), "ack"),
            (asked("haiku"), "ack"),
            (bye(), "bye"),
            (error("x"), "error"),
        ] {
            assert_eq!(parse(&frame)["type"], kind);
        }
        let b = parse(&bubble(&CaseId::new("c1"), "▎ t"));
        assert_eq!(
            (b["type"].as_str(), b["case"].as_str(), b["text"].as_str()),
            (Some("bubble"), Some("c1"), Some("▎ t"))
        );
    }
}
