//! What the use cases need from the world, as role nouns. One trait per
//! file, each owning its error type. Gateways implement them; tests use
//! the in-memory fakes in `use_cases::testing`.

pub mod agent_launcher;
pub mod case_store;
pub mod clock;
pub mod environment;
pub mod ids;
pub mod ignore_store;
pub mod model_gateway;
pub mod notifier;
pub mod output_source;
pub mod secrets;
pub mod session_registry;

pub use agent_launcher::{AgentError, AgentLauncher};
pub use case_store::{CaseStore, CaseStoreError};
pub use clock::Clock;
pub use environment::Environment;
pub use ids::IdGenerator;
pub use ignore_store::{IgnoreStore, IgnoreStoreError};
pub use model_gateway::{ModelError, ModelGateway, Prompt};
pub use notifier::{Notifier, NotifyError};
pub use output_source::OutputSource;
pub use secrets::Secrets;
pub use session_registry::{RegistryError, SessionRegistry};
