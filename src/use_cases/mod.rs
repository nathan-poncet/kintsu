//! The application rules: one interactor per thing a user or a hook can
//! ask for, each driving the ports it needs. As pure as the entities: no
//! I/O, no executor, no terminal. Async, when it comes, arrives as
//! `impl Future` on the ports; the runtime stays in the outer rings.

pub mod ports;
pub mod triage_outcome;

pub use triage_outcome::triage_outcome;
