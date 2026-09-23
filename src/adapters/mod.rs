//! The interface adapters. Controllers turn what comes in (argv, daemon
//! frames, URL-scheme calls) into use case inputs; presenters turn what
//! comes out (decisions, answers, streams) into what a surface shows
//! (toast, panel, plain text, JSON); gateways implement the ports over the
//! outside world (models, agents, terminals, storage, secrets).

pub mod controllers;
pub mod gateways;
pub mod presenters;
