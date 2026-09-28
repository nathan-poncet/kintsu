//! The keys a shell forwarded for its session, then the daemon's own
//! environment. Under launchd or systemd the daemon has none of the
//! variables the configuration names; the hook sends their values with
//! each command and they live here, in memory, for that session.

use std::collections::BTreeMap;

use crate::adapters::gateways::EnvSecrets;
use crate::entities::KeySource;
use crate::use_cases::ports::Secrets;

pub struct SessionSecrets {
    forwarded: BTreeMap<String, String>,
}

impl SessionSecrets {
    /// Over what one shell forwarded; empty means the daemon's environment
    /// alone.
    pub fn new(forwarded: BTreeMap<String, String>) -> Self {
        Self { forwarded }
    }
}

impl Secrets for SessionSecrets {
    fn lookup(&self, source: &KeySource) -> Option<String> {
        if let KeySource::Env(var) = source
            && let Some(value) = self.forwarded.get(var).map(|v| v.trim())
            && !value.is_empty()
        {
            return Some(value.to_string());
        }
        EnvSecrets.lookup(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forwarded(pairs: &[(&str, &str)]) -> SessionSecrets {
        SessionSecrets::new(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    #[test]
    fn a_forwarded_value_wins_over_the_daemons_own_environment() {
        // PATH is set in every process: the shell's value must still win.
        let secrets = forwarded(&[("PATH", " /their/bin ")]);
        assert_eq!(
            secrets.lookup(&KeySource::Env("PATH".into())).as_deref(),
            Some("/their/bin")
        );
    }

    #[test]
    fn what_was_not_forwarded_falls_back_to_the_environment_and_the_other_sources() {
        let secrets = forwarded(&[("BLANK", "  ")]);
        assert_eq!(
            secrets.lookup(&KeySource::Env("KINTSU_TEST_SURELY_UNSET_VAR".into())),
            None
        );
        assert_eq!(
            secrets.lookup(&KeySource::Env("BLANK".into())),
            None,
            "a blank value is no value"
        );
        assert!(
            secrets.lookup(&KeySource::Env("PATH".into())).is_some(),
            "the daemon's own environment still counts"
        );
        assert_eq!(
            secrets.lookup(&KeySource::Literal("k".into())).as_deref(),
            Some("k")
        );
        assert_eq!(
            secrets
                .lookup(&KeySource::Command("printf from-cmd".into()))
                .as_deref(),
            Some("from-cmd")
        );
        assert_eq!(secrets.lookup(&KeySource::None), None);
    }
}
