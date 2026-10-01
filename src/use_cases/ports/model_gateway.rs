//! One question to one model, one answer back.

use thiserror::Error;

use crate::entities::{ModelSpec, Tokens};

/// What a model is asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// The role and the rules.
    pub system: String,
    /// The case, fenced.
    pub user: String,
    /// How long the answer may be.
    pub max_tokens: u32,
    /// Prose, or one fix as a JSON object the gateway asks for in the
    /// provider's own way.
    pub shape: AnswerShape,
}

/// The shape of the answer the caller wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnswerShape {
    /// Free text.
    #[default]
    Prose,
    /// One corrected command line, or none, as the JSON object
    /// `{"command", "confidence", "rationale"}`: a forced tool call where
    /// the provider has tools, a JSON schema elsewhere. The text answer
    /// stays a valid fallback.
    Fix,
}

/// Why a model did not answer.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelError {
    /// The key source yielded nothing.
    #[error("no key: {0}")]
    MissingKey(String),
    /// The gateway cannot speak to this kind of model.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// The endpoint did not answer.
    #[error("unreachable: {0}")]
    Unreachable(String),
    /// The endpoint answered with an error.
    #[error("refused: {0}")]
    Refused(String),
    /// The answer could not be read.
    #[error("malformed answer: {0}")]
    Malformed(String),
}

/// A whole answer and what the provider counted for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub text: String,
    /// Zero when the provider reported nothing.
    pub tokens: Tokens,
    /// The fix the model proposed in the shape it was asked for, when it
    /// did; the gateway reads the provider's structure, nobody else.
    pub fix: Option<ProposedFix>,
}

/// `{"command", "confidence", "rationale"}` as a model wrote it: a null
/// command is a model saying it has none.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProposedFix {
    pub command: Option<String>,
    pub confidence: Option<f32>,
    pub rationale: Option<String>,
}

/// Speaks to models.
pub trait ModelGateway {
    /// Asks and waits for the whole answer.
    fn complete(
        &self,
        spec: &ModelSpec,
        key: Option<&str>,
        prompt: &Prompt,
    ) -> Result<String, ModelError>;

    /// Asks and waits for the whole answer, with the tokens it cost; a
    /// gateway that cannot count says zero.
    fn answer(
        &self,
        spec: &ModelSpec,
        key: Option<&str>,
        prompt: &Prompt,
    ) -> Result<Answer, ModelError> {
        self.complete(spec, key, prompt).map(|text| Answer {
            text,
            tokens: Tokens::default(),
            fix: None,
        })
    }

    /// Asks and hands the answer over as it comes, then returns it whole.
    /// A gateway that cannot stream hands it over in one piece.
    fn stream(
        &self,
        spec: &ModelSpec,
        key: Option<&str>,
        prompt: &Prompt,
        on_chunk: &mut dyn FnMut(&str),
    ) -> Result<String, ModelError> {
        let answer = self.complete(spec, key, prompt)?;
        on_chunk(&answer);
        Ok(answer)
    }

    /// Whether the model can be reached right now, without asking it
    /// anything: a local server that is not running is not. Remote
    /// endpoints are assumed reachable; only a request tells.
    fn is_reachable(&self, spec: &ModelSpec) -> bool {
        let _ = spec;
        true
    }
}
