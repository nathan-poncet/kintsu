//! `kintsu models`, `kintsu models test` and `kintsu login <model>`: the
//! configured models against the machine, and a key into the keychain.

use std::io::{BufRead, Write};
use std::process::ExitCode;

use crate::adapters::gateways::{
    EnvSecrets, FsEnvironment, HttpModels, OsKeychain, SystemClock, unix, with_keychain_key,
};
use crate::adapters::presenters::{
    Style, login_notice, models_json, models_table, probes_json, probes_report,
};
use crate::entities::{SecretKey, Settings};
use crate::use_cases::ports::ModelError;
use crate::use_cases::{ListModels, Login, ProbeModels};

use super::{Runtime, failure};

pub(super) fn list(
    settings: &Settings,
    environment: &FsEnvironment,
    json: bool,
    style: &Style,
    out: &mut dyn Write,
) -> ExitCode {
    let rows = ListModels {
        settings,
        secrets: &EnvSecrets,
        environment,
        models: &HttpModels,
    }
    .run();
    let text = if json {
        models_json(&rows)
    } else {
        models_table(&rows, style)
    };
    let _ = writeln!(out, "{text}");
    ExitCode::SUCCESS
}

/// Exit 1 when a model that should have answered did not; agents, which
/// are not asked, do not count.
pub(super) fn test(
    settings: &Settings,
    json: bool,
    style: &Style,
    out: &mut dyn Write,
) -> ExitCode {
    let probes = ProbeModels {
        settings,
        secrets: &EnvSecrets,
        models: &HttpModels,
        clock: &SystemClock,
    }
    .run();
    let text = if json {
        probes_json(&probes)
    } else {
        probes_report(&probes, style)
    };
    let _ = writeln!(out, "{text}");
    let failed = probes
        .iter()
        .any(|p| matches!(&p.outcome, Err(e) if !matches!(e, ModelError::Unsupported(_))));
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

pub(super) fn login(
    rt: &Runtime,
    settings: &Settings,
    model: &str,
    write_config: bool,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    let keychain = OsKeychain::new(rt.path_var.clone());
    let login = Login {
        settings,
        store: &keychain,
    };
    if let Err(e) = login.check(model) {
        return failure(err, &e.to_string(), style, false);
    }
    let typed = match read_key(model, err) {
        Ok(typed) => typed,
        Err(e) => return failure(err, &format!("cannot read the key: {e}"), style, false),
    };
    let key = match SecretKey::new(typed) {
        Ok(key) => key,
        Err(e) => return failure(err, &e.to_string(), style, false),
    };
    let done = match login.run(model, &key) {
        Ok(done) => done,
        Err(e) => return failure(err, &e.to_string(), style, false),
    };
    let wrote = write_config && !done.configured;
    if wrote {
        let edited = std::fs::read_to_string(&rt.config_path)
            .ok()
            .and_then(|text| with_keychain_key(&text, &done.model));
        match edited {
            Some(text) => {
                if let Err(e) = std::fs::write(&rt.config_path, text) {
                    return failure(
                        err,
                        &format!("stored, but cannot write {}: {e}", rt.config_path.display()),
                        style,
                        false,
                    );
                }
            }
            None => {
                return failure(
                    err,
                    &format!(
                        "stored, but no [models.{}] table in {} to point at the keychain",
                        done.model,
                        rt.config_path.display()
                    ),
                    style,
                    false,
                );
            }
        }
    }
    let _ = writeln!(out, "{}", login_notice(&done, wrote, style));
    ExitCode::SUCCESS
}

/// The key, typed on the terminal with echo off, or piped in.
fn read_key(model: &str, err: &mut dyn Write) -> std::io::Result<String> {
    let stdin = std::io::stdin();
    let mut line = String::new();
    if unix::is_tty(&stdin) {
        let _ = write!(err, "▎ Key for {model} (not shown): ");
        let _ = err.flush();
        let read = unix::with_echo_off(&stdin, || stdin.lock().read_line(&mut line));
        let _ = writeln!(err);
        read?;
    } else {
        stdin.lock().read_line(&mut line)?;
    }
    Ok(line)
}
