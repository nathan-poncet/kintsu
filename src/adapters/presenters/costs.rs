//! `kintsu costs`: today and the last thirty days, per model, as text or
//! as JSON.

use serde_json::json;

use crate::entities::{ModelSpend, Money};
use crate::use_cases::CostReport;

use super::Style;

pub fn costs_report(report: &CostReport, style: &Style) -> String {
    let mut out = Vec::new();
    let cap = match report.cap {
        Some(cap) if report.today_total >= cap => format!(
            " of {cap}{}",
            style.warn(" (spent: remote models wait for midnight UTC)")
        ),
        Some(cap) => format!(" of {cap}"),
        None => String::new(),
    };
    out.push(format!(
        "Today ({}, UTC): {}{cap}",
        report.day,
        style.bold(&report.today_total.to_string())
    ));
    out.extend(rows(&report.today, style));
    out.push(String::new());
    out.push(format!(
        "Last 30 days: {}",
        style.bold(&report.month_total.to_string())
    ));
    out.extend(rows(&report.month, style));
    if report.cap.is_none() {
        out.push(String::new());
        out.push(style.dim(
            "No daily cap; set routing.constraints.max_daily_cost = \"1.00 USD\" to add one.",
        ));
    }
    out.join("\n")
}

fn rows(spends: &[ModelSpend], style: &Style) -> Vec<String> {
    if spends.is_empty() {
        return vec![style.dim("  no model call")];
    }
    let width = spends
        .iter()
        .map(|s| s.model.chars().count())
        .max()
        .unwrap_or(0);
    spends
        .iter()
        .map(|s| {
            let calls = if s.calls == 1 { "call" } else { "calls" };
            let cost = if s.unpriced == s.calls {
                "price unknown".to_string()
            } else if s.unpriced > 0 {
                format!("{} + {} unpriced", s.cost, s.unpriced)
            } else if s.cost == Money::ZERO {
                "free".to_string()
            } else {
                s.cost.to_string()
            };
            format!(
                "  {:<width$}  {:>3} {calls:<5}  {:>8} in  {:>8} out  {cost}",
                s.model, s.calls, s.tokens.input, s.tokens.output
            )
        })
        .collect()
}

pub fn costs_json(report: &CostReport) -> String {
    let spends = |spends: &[ModelSpend]| -> Vec<serde_json::Value> {
        spends
            .iter()
            .map(|s| {
                json!({
                    "model": s.model,
                    "calls": s.calls,
                    "input_tokens": s.tokens.input,
                    "output_tokens": s.tokens.output,
                    "cost_usd": micro_to_usd(s.cost),
                    "unpriced_calls": s.unpriced,
                })
            })
            .collect()
    };
    json!({
        "day": report.day.to_string(),
        "currency": "USD",
        "today": {"total_usd": micro_to_usd(report.today_total), "models": spends(&report.today)},
        "last_30_days": {"total_usd": micro_to_usd(report.month_total), "models": spends(&report.month)},
        "max_daily_cost_usd": report.cap.map(micro_to_usd),
    })
    .to_string()
}

/// Dollars with six decimals, as a JSON number.
fn micro_to_usd(money: Money) -> f64 {
    money.micro_usd() as f64 / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{Day, Tokens};

    fn report() -> CostReport {
        CostReport {
            day: Day::from_index(20_724),
            today: vec![
                ModelSpend {
                    model: "haiku".into(),
                    calls: 2,
                    tokens: Tokens::new(2_400, 300),
                    cost: Money::from_micro_usd(3_900),
                    unpriced: 0,
                },
                ModelSpend {
                    model: "local".into(),
                    calls: 1,
                    tokens: Tokens::new(900, 40),
                    cost: Money::ZERO,
                    unpriced: 0,
                },
            ],
            today_total: Money::from_micro_usd(3_900),
            month: vec![ModelSpend {
                model: "mystery".into(),
                calls: 3,
                tokens: Tokens::new(10, 10),
                cost: Money::ZERO,
                unpriced: 3,
            }],
            month_total: Money::from_micro_usd(3_900),
            cap: Some(Money::from_micro_usd(1_000_000)),
        }
    }

    #[test]
    fn the_text_names_the_day_the_cap_and_each_model() {
        let text = costs_report(&report(), &Style::PLAIN);
        assert!(
            text.starts_with("Today (2026-09-28, UTC): $0.0039 of $1.00\n"),
            "{text}"
        );
        assert!(
            text.contains("  haiku    2 calls      2400 in       300 out  $0.0039"),
            "{text}"
        );
        assert!(
            text.contains("  local    1 call        900 in        40 out  free"),
            "{text}"
        );
        assert!(
            text.contains("  mystery    3 calls        10 in        10 out  price unknown"),
            "{text}"
        );
        let spent = CostReport {
            today_total: Money::from_micro_usd(2_000_000),
            ..report()
        };
        assert!(
            costs_report(&spent, &Style::PLAIN)
                .contains("(spent: remote models wait for midnight UTC)")
        );
        let uncapped = CostReport {
            cap: None,
            today: vec![],
            ..report()
        };
        let text = costs_report(&uncapped, &Style::PLAIN);
        assert!(text.contains("  no model call"), "{text}");
        assert!(text.contains("No daily cap"), "{text}");
    }

    #[test]
    fn the_json_carries_dollars_as_numbers_and_the_cap() {
        let v: serde_json::Value = serde_json::from_str(&costs_json(&report())).unwrap();
        assert_eq!(v["day"], "2026-09-28");
        assert_eq!(v["today"]["total_usd"], 0.0039);
        assert_eq!(v["today"]["models"][0]["model"], "haiku");
        assert_eq!(v["today"]["models"][0]["input_tokens"], 2400);
        assert_eq!(v["last_30_days"]["models"][0]["unpriced_calls"], 3);
        assert_eq!(v["max_daily_cost_usd"], 1.0);
        let uncapped = CostReport {
            cap: None,
            ..report()
        };
        let v: serde_json::Value = serde_json::from_str(&costs_json(&uncapped)).unwrap();
        assert!(v["max_daily_cost_usd"].is_null());
    }
}
