//! What the use cases need from the world, as role nouns. One trait per
//! file, each owning its error type. Gateways implement them; tests use
//! the in-memory fakes in `use_cases::testing`.

pub mod agent_launcher;
pub mod case_store;
pub mod clock;
pub mod cost_ledger;
pub mod environment;
pub mod ids;
pub mod ignore_store;
pub mod learned_fixes;
pub mod model_gateway;
pub mod notifier;
pub mod output_source;
pub mod scoreboard;
pub mod secret_store;
pub mod secrets;
pub mod session_registry;

pub use agent_launcher::{AgentError, AgentLauncher};
pub use case_store::{CaseStore, CaseStoreError};
pub use clock::Clock;
pub use cost_ledger::{CostLedger, LedgerError};
pub use environment::Environment;
pub use ids::IdGenerator;
pub use ignore_store::{IgnoreStore, IgnoreStoreError};
pub use learned_fixes::{LearnedFixes, LearnedFixesError};
pub use model_gateway::{Answer, AnswerShape, ModelError, ModelGateway, Prompt, ProposedFix};
pub use notifier::{Notifier, NotifyError};
pub use output_source::OutputSource;
pub use scoreboard::{Scoreboard, ScoreboardError};
pub use secret_store::{SecretStore, SecretStoreError};
pub use secrets::Secrets;
pub use session_registry::{RegistryError, SessionRegistry};
