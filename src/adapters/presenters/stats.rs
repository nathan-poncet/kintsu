//! `kintsu stats`: the last thirty days as text or as JSON.

use serde_json::json;

use crate::entities::{FixSource, SourceScore};
use crate::use_cases::StatsReport;

use super::Style;

pub fn stats_report(report: &StatsReport, style: &Style) -> String {
    let card = &report.card;
    let mut out = vec![format!(
        "Last 30 days, to {} (UTC): {} looked at, {} offered, {} taken",
        report.day,
        style.bold(&plural(card.failures, "failure")),
        plural(card.offered, "fix"),
        style.bold(&card.taken.to_string())
    )];
    let busy: Vec<String> = card
        .days
        .iter()
        .filter(|d| d.failures > 0)
        .map(|d| format!("  {}  {:>3}", d.day, d.failures))
        .collect();
    if busy.is_empty() {
        out.push(style.dim("  no failure looked at"));
    } else {
        out.extend(busy);
    }
    out.push(String::new());
    out.push("Fixes by source:".to_string());
    if card.sources.is_empty() {
        out.push(style.dim("  no fix offered"));
    } else {
        let width = card
            .sources
            .iter()
            .map(|s| s.source.to_string().chars().count())
            .max()
            .unwrap_or(0);
        out.push(format!("  {:<width$}  offered  taken   rate", ""));
        out.extend(card.sources.iter().map(|s| row(s, width)));
    }
    out.push(String::new());
    out.push(format!(
        "Explanations asked of a model: {}",
        report.explanations
    ));
    out.join("\n")
}

fn row(score: &SourceScore, width: usize) -> String {
    let rate = match score.rate() {
        Some(rate) => format!("{:>3} %", (rate * 100.0).round() as u64),
        None => "    -".to_string(),
    };
    format!(
        "  {:<width$}  {:>7}  {:>5}  {rate}",
        score.source.to_string(),
        score.offered,
        score.taken
    )
}

fn plural(count: u64, noun: &str) -> String {
    match (count, noun) {
        (1, _) => format!("1 {noun}"),
        (_, "fix") => format!("{count} fixes"),
        _ => format!("{count} {noun}s"),
    }
}

pub fn stats_json(report: &StatsReport) -> String {
    let card = &report.card;
    json!({
        "day": report.day.to_string(),
        "days": card.days.iter().map(|d| json!({"day": d.day.to_string(), "failures": d.failures})).collect::<Vec<_>>(),
        "totals": {"failures": card.failures, "offered": card.offered, "taken": card.taken},
        "sources": card.sources.iter().map(|s| {
            let (kind, name) = match &s.source {
                FixSource::Rule(name) => ("rule", name),
                FixSource::Model(name) => ("model", name),
            };
            json!({"kind": kind, "name": name, "offered": s.offered, "taken": s.taken, "rate": s.rate()})
        }).collect::<Vec<_>>(),
        "explanations": report.explanations,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::score::DayCount;
    use crate::entities::{Day, Scorecard};

    fn report() -> StatsReport {
        let today = Day::from_index(20_724);
        StatsReport {
            day: today,
            card: Scorecard {
                days: vec![
                    DayCount {
                        day: today.minus(1),
                        failures: 0,
                    },
                    DayCount {
                        day: today,
                        failures: 3,
                    },
                ],
                sources: vec![
                    SourceScore {
                        source: FixSource::Rule("command typo".into()),
                        offered: 2,
                        taken: 2,
                    },
                    SourceScore {
                        source: FixSource::Model("local".into()),
                        offered: 1,
                        taken: 0,
                    },
                ],
                failures: 3,
                offered: 3,
                taken: 2,
            },
            explanations: 1,
        }
    }

    #[test]
    fn the_text_shows_busy_days_and_one_row_per_source_with_its_rate() {
        let text = stats_report(&report(), &Style::PLAIN);
        assert!(
            text.starts_with(
                "Last 30 days, to 2026-09-28 (UTC): 3 failures looked at, 3 fixes offered, 2 taken"
            ),
            "{text}"
        );
        assert!(text.contains("  2026-09-28    3"), "{text}");
        assert!(!text.contains("2026-09-27"), "quiet days are left out");
        assert!(
            text.contains("rule · command typo        2      2  100 %"),
            "{text}"
        );
        assert!(
            text.contains("model · local              1      0    0 %"),
            "{text}"
        );
        assert!(text.ends_with("Explanations asked of a model: 1"), "{text}");
    }

    #[test]
    fn an_empty_month_says_so() {
        let mut empty = report();
        empty.card = Scorecard {
            days: vec![],
            sources: vec![],
            failures: 0,
            offered: 0,
            taken: 0,
        };
        let text = stats_report(&empty, &Style::PLAIN);
        assert!(text.contains("0 failures looked at, 0 fixes offered, 0 taken"));
        assert!(text.contains("no failure looked at"));
        assert!(text.contains("no fix offered"));
    }

    #[test]
    fn the_json_carries_every_day_and_every_source() {
        let v: serde_json::Value = serde_json::from_str(&stats_json(&report())).unwrap();
        assert_eq!(v["day"], "2026-09-28");
        assert_eq!(v["days"].as_array().unwrap().len(), 2);
        assert_eq!(v["days"][1]["failures"], 3);
        assert_eq!(v["totals"]["taken"], 2);
        assert_eq!(v["sources"][0]["kind"], "rule");
        assert_eq!(v["sources"][0]["name"], "command typo");
        assert_eq!(v["sources"][0]["rate"], 1.0);
        assert_eq!(v["sources"][1]["rate"], 0.0);
        assert_eq!(v["explanations"], 1);
    }
}
