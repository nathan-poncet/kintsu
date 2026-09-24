//! One question to one model, one answer back.

use thiserror::Error;

use crate::entities::ModelSpec;

/// What a model is asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// The role and the rules.
    pub system: String,
    /// The case, fenced.
    pub user: String,
    /// How long the answer may be.
    pub max_tokens: u32,
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

/// Speaks to models.
pub trait ModelGateway {
    /// Asks and waits for the whole answer.
    fn complete(
        &self,
        spec: &ModelSpec,
        key: Option<&str>,
        prompt: &Prompt,
    ) -> Result<String, ModelError>;

    /// Whether the model can be reached right now, without asking it
    /// anything: a local server that is not running is not. Remote
    /// endpoints are assumed reachable; only a request tells.
    fn is_reachable(&self, spec: &ModelSpec) -> bool {
        let _ = spec;
        true
    }
}
