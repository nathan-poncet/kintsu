//! A failure worth attention, with everything a helper needs to know.

use crate::entities::{CommandLine, CommandOutcome, Fix, Redacted, SessionId, Timestamp, redact};

/// The unguessable identity of a case; clickable links and `act` frames
/// carry it, so it is never sequential.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CaseId(String);

impl CaseId {
    /// Wraps a token handed out by the id port.
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    /// The token.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A failed command and its context: where it ran, what ran before, what
/// it printed when that was captured.
#[derive(Debug, Clone, PartialEq)]
pub struct FailureCase {
    id: CaseId,
    at: Timestamp,
    outcome: CommandOutcome,
    cwd: Option<String>,
    session: Option<SessionId>,
    recent: Vec<CommandLine>,
    output: Option<String>,
    proposal: Option<Fix>,
}

impl FailureCase {
    /// A case from a failed outcome, at a moment, in a directory.
    pub fn new(id: CaseId, at: Timestamp, outcome: CommandOutcome, cwd: Option<String>) -> Self {
        Self {
            id,
            at,
            outcome,
            cwd,
            session: None,
            recent: Vec::new(),
            output: None,
            proposal: None,
        }
    }

    /// The same case with a fix someone already proposed for it.
    pub fn with_proposal(mut self, proposal: Option<Fix>) -> Self {
        self.proposal = proposal;
        self
    }

    /// The same case knowing which shell session it happened in.
    pub fn with_session(mut self, session: Option<SessionId>) -> Self {
        self.session = session;
        self
    }

    /// The same case knowing the command lines that ran before it.
    pub fn with_recent(mut self, recent: Vec<CommandLine>) -> Self {
        self.recent = recent;
        self
    }

    /// The same case with the output the command printed.
    pub fn with_output(mut self, output: String) -> Self {
        self.output = Some(output);
        self
    }

    /// The identity.
    pub fn id(&self) -> &CaseId {
        &self.id
    }

    /// When the command finished.
    pub fn at(&self) -> Timestamp {
        self.at
    }

    /// The failed command line and its status.
    pub fn outcome(&self) -> &CommandOutcome {
        &self.outcome
    }

    /// The working directory, when the hook sent it.
    pub fn cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    /// The shell session, when the hook said which.
    pub fn session(&self) -> Option<&SessionId> {
        self.session.as_ref()
    }

    /// What ran before, oldest first.
    pub fn recent(&self) -> &[CommandLine] {
        &self.recent
    }

    /// What the command printed, when captured.
    pub fn output(&self) -> Option<&str> {
        self.output.as_deref()
    }

    /// The fix a rule or a model already proposed, if any.
    pub fn proposal(&self) -> Option<&Fix> {
        self.proposal.as_ref()
    }

    /// Everything that could leave the machine, with secrets redacted:
    /// the command line, the output, the recent history.
    pub fn redacted(&self) -> Redacted {
        let mut text = String::new();
        text.push_str(self.outcome.command().as_str());
        if let Some(out) = &self.output {
            text.push('\n');
            text.push_str(out);
        }
        for line in &self.recent {
            text.push('\n');
            text.push_str(line.as_str());
        }
        redact(&text)
    }

    /// Whether anything sensitive appeared in the command, its output or
    /// the recent history: then no cloud model may see the case.
    pub fn is_sensitive(&self) -> bool {
        !self.redacted().findings().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::ExitStatus;

    fn case(text: &str, code: i32) -> FailureCase {
        FailureCase::new(
            CaseId::new("k1"),
            Timestamp::from_millis(0),
            CommandOutcome::new(CommandLine::new(text).unwrap(), ExitStatus::new(code)),
            Some("/home/me/dev".into()),
        )
    }

    #[test]
    fn a_token_in_the_command_makes_the_case_sensitive() {
        assert!(
            case(
                "curl -H 'Authorization: Bearer sk-live-abcdefghijklmnop' https://x",
                22
            )
            .is_sensitive()
        );
        assert!(!case("gti status", 127).is_sensitive());
    }
}
