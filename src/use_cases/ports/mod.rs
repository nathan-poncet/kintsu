//! Ports: the traits the use cases define and the adapters implement
//! (dependency inversion). Role nouns, one file per port, each owning its
//! error type. None yet: the first ones land with the daemon
//! (`SessionRegistry`, `CaseStore`, `Clock`, `Notifier`) and with the model
//! layer (`ModelGateway`, `AgentLauncher`, `OutputSource`, `SecretStore`).
//! The map is in `docs/ARCHITECTURE.md`.
