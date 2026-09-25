//! What arrives later, above the prompt: a model's answer about a case.
//! When the shell has moved on to another command by then, the message is
//! *late*: it names the command it is about, so it never reads as being
//! about the current one.

use crate::entities::{CaseId, CommandLine, Fix, Timestamp};

/// The content of a message.
#[derive(Debug, Clone, PartialEq)]
pub enum MessageBody {
    /// A model proposed a command line.
    Fix(Fix),
    /// A model explained the failure.
    Explanation { model: String, text: String },
    /// One line of news about the case: a model that had nothing, or failed.
    Note(String),
}

/// A message about a case, sent to the shell it happened in.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    case: CaseId,
    /// When the answer came; kept for the panel's timeline.
    #[allow(dead_code)]
    at: Timestamp,
    body: MessageBody,
    about: Option<CommandLine>,
    late: bool,
}

impl Message {
    pub fn new(case: CaseId, at: Timestamp, body: MessageBody) -> Self {
        Self {
            case,
            at,
            body,
            about: None,
            late: false,
        }
    }

    /// The same message knowing the command line it is about.
    pub fn about(mut self, command: CommandLine) -> Self {
        self.about = Some(command);
        self
    }

    /// The same message, arriving once the shell looks at something else.
    pub fn late(mut self) -> Self {
        self.late = true;
        self
    }

    pub fn case(&self) -> &CaseId {
        &self.case
    }

    pub fn body(&self) -> &MessageBody {
        &self.body
    }

    /// The command line the message is about, when known.
    pub fn command(&self) -> Option<&CommandLine> {
        self.about.as_ref()
    }

    /// Whether the shell had moved on when the message came.
    pub fn is_late(&self) -> bool {
        self.late
    }
}
