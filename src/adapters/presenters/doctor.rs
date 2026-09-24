//! `kintsu doctor`: one line per check, then where things are.

use crate::use_cases::{Check, Health};

use super::Style;

/// Where the files are, for the footer.
pub struct Places<'a> {
    pub config: &'a str,
    pub config_exists: bool,
    pub state: &'a str,
}

pub fn doctor_report(checks: &[Check], places: &Places<'_>, style: &Style) -> String {
    let width = checks
        .iter()
        .map(|c| c.subject.chars().count())
        .max()
        .unwrap_or(0);
    let mut out: Vec<String> = checks
        .iter()
        .map(|c| {
            let mark = match (c.health, style.ascii) {
                (Health::Ok, true) => "ok ".to_string(),
                (Health::Ok, false) => "✓ ".to_string(),
                (Health::Warning, _) => style.warn("! "),
                (Health::Problem, true) => style.warn("x "),
                (Health::Problem, false) => style.warn("✗ "),
            };
            format!("{mark}{:<width$}  {}", c.subject, c.detail)
        })
        .collect();
    out.push(String::new());
    let config = if places.config_exists {
        places.config.to_string()
    } else {
        format!(
            "{} (missing: kintsu default-config > that path)",
            places.config
        )
    };
    out.push(format!("config  {config}"));
    out.push(format!("state   {}", places.state));
    let problems = checks
        .iter()
        .filter(|c| c.health == Health::Problem)
        .count();
    out.push(String::new());
    out.push(match problems {
        0 => "Everything Kintsu needs is here.".to_string(),
        1 => "1 problem to fix.".to_string(),
        n => format!("{n} problems to fix."),
    });
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_aligns_subjects_and_counts_problems() {
        let checks = vec![
            Check {
                subject: "shell hook".into(),
                health: Health::Ok,
                detail: "active".into(),
            },
            Check {
                subject: "model claude".into(),
                health: Health::Problem,
                detail: "not on the PATH".into(),
            },
            Check {
                subject: "model x".into(),
                health: Health::Warning,
                detail: "literal key".into(),
            },
        ];
        let places = Places {
            config: "/c/config.toml",
            config_exists: false,
            state: "/s",
        };
        let text = doctor_report(&checks, &places, &Style::PLAIN);
        assert!(text.starts_with("ok shell hook    active\nx model claude  not on the PATH\n! model x       literal key\n"));
        assert!(
            text.contains("config  /c/config.toml (missing: kintsu default-config > that path)")
        );
        assert!(text.ends_with("1 problem to fix."));
    }
}
