//! What arrives later, above the prompt: a model's answer about a case
//! the shell already moved past.

use crate::entities::{CaseId, Fix, Timestamp};

/// The content of a message.
#[derive(Debug, Clone, PartialEq)]
pub enum MessageBody {
    /// A model proposed a command line.
    Fix(Fix),
    /// A model explained the failure.
    Explanation { model: String, text: String },
}

/// A message about a case, sent to the shell it happened in.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    case: CaseId,
    /// When the answer came; kept for the panel's timeline.
    #[allow(dead_code)]
    at: Timestamp,
    body: MessageBody,
}

impl Message {
    pub fn new(case: CaseId, at: Timestamp, body: MessageBody) -> Self {
        Self { case, at, body }
    }

    pub fn case(&self) -> &CaseId {
        &self.case
    }

    pub fn body(&self) -> &MessageBody {
        &self.body
    }
}
