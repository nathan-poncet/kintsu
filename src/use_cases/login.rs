//! `kintsu login <model>`: the key goes to the OS keychain, under the
//! model's name, and the configuration is told to read it from there.

use thiserror::Error;

use crate::entities::{KeySource, ModelSpec, Provider, SecretKey, Settings};
use crate::use_cases::ports::{SecretStore, SecretStoreError};

/// Why the key was not stored.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LoginError {
    #[error("`{0}` is not a configured model")]
    UnknownModel(String),
    #[error("`{0}` needs no key: {1}")]
    NoKeyNeeded(String, &'static str),
    #[error(transparent)]
    Store(#[from] SecretStoreError),
}

/// What happened, for the notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggedIn {
    /// The model, which is also the keychain account.
    pub model: String,
    /// Whether the configuration already reads this model's key from the keychain.
    pub configured: bool,
}

pub struct Login<'a> {
    pub settings: &'a Settings,
    pub store: &'a dyn SecretStore,
}

impl Login<'_> {
    /// Whether `model` is one a key can be stored for, before asking for
    /// the key.
    pub fn check(&self, model: &str) -> Result<&ModelSpec, LoginError> {
        let spec = self
            .settings
            .model(model)
            .ok_or_else(|| LoginError::UnknownModel(model.to_string()))?;
        match spec.provider {
            Provider::CliAgent => Err(LoginError::NoKeyNeeded(
                model.to_string(),
                "a CLI agent uses its own login",
            )),
            Provider::Ollama => Err(LoginError::NoKeyNeeded(
                model.to_string(),
                "Ollama takes none",
            )),
            Provider::OpenAiCompatible | Provider::Anthropic => Ok(spec),
        }
    }

    pub fn run(&self, model: &str, key: &SecretKey) -> Result<LoggedIn, LoginError> {
        let spec = self.check(model)?;
        self.store.store(&spec.name, key)?;
        Ok(LoggedIn {
            model: spec.name.clone(),
            configured: matches!(&spec.key, KeySource::Keychain(account) if account == &spec.name),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::Tier;
    use crate::use_cases::ports::Secrets;
    use crate::use_cases::testing::*;

    fn settings() -> Settings {
        let mut vault = spec("vault", Provider::Anthropic, Tier::Large);
        vault.key = KeySource::Keychain("vault".into());
        let mut cloud = spec("cloud", Provider::Anthropic, Tier::Large);
        cloud.key = KeySource::Env("ANTHROPIC_API_KEY".into());
        Settings {
            models: vec![
                vault,
                cloud,
                spec("local", Provider::Ollama, Tier::Small),
                spec("claude", Provider::CliAgent, Tier::Agent),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn the_key_lands_under_the_models_name_and_the_notice_knows_the_config() {
        let settings = settings();
        let store = MemorySecretStore::default();
        let uc = Login {
            settings: &settings,
            store: &store,
        };
        let key = SecretKey::new("sk-1").unwrap();
        assert_eq!(
            uc.run("vault", &key),
            Ok(LoggedIn {
                model: "vault".into(),
                configured: true,
            })
        );
        assert_eq!(
            uc.run("cloud", &key),
            Ok(LoggedIn {
                model: "cloud".into(),
                configured: false,
            }),
            "stored, but the file still says env"
        );
        assert_eq!(
            store
                .lookup(&KeySource::Keychain("cloud".into()))
                .as_deref(),
            Some("sk-1")
        );
    }

    #[test]
    fn models_that_take_no_key_and_unknown_names_are_refused_before_storing() {
        let settings = settings();
        let store = MemorySecretStore::default();
        let uc = Login {
            settings: &settings,
            store: &store,
        };
        let key = SecretKey::new("sk-1").unwrap();
        assert!(matches!(
            uc.run("claude", &key),
            Err(LoginError::NoKeyNeeded(_, _))
        ));
        assert!(matches!(
            uc.run("local", &key),
            Err(LoginError::NoKeyNeeded(_, _))
        ));
        assert_eq!(
            uc.run("nope", &key),
            Err(LoginError::UnknownModel("nope".into()))
        );
        assert!(store.entries.borrow().is_empty());
    }

    #[test]
    fn a_keychain_that_refuses_is_reported_as_such() {
        let settings = settings();
        let store = MemorySecretStore {
            failure: Some(SecretStoreError::Unavailable("no secret-tool".into())),
            ..Default::default()
        };
        let uc = Login {
            settings: &settings,
            store: &store,
        };
        assert_eq!(
            uc.run("vault", &SecretKey::new("sk-1").unwrap()),
            Err(LoginError::Store(SecretStoreError::Unavailable(
                "no secret-tool".into()
            )))
        );
    }
}
