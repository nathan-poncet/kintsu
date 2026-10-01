//! What a model call costs: tokens as the providers count them, money in
//! micro-dollars, the list prices of the models we know, and what a day
//! of calls adds up to. Days are UTC: the budget resets at midnight UTC.

use std::fmt;

use thiserror::Error;

use crate::entities::{Duration, Timestamp};

/// Tokens a provider counted for one call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
}

impl Tokens {
    pub const fn new(input: u64, output: u64) -> Self {
        Self { input, output }
    }

    pub const fn plus(self, other: Tokens) -> Self {
        Self {
            input: self.input.saturating_add(other.input),
            output: self.output.saturating_add(other.output),
        }
    }
}

/// An amount of US dollars, in millionths, so a call worth a few
/// hundredths of a cent still counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Money(u64);

/// Why a text is not an amount.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MoneyError {
    #[error("`{0}` is not an amount; write it like \"1.00 USD\"")]
    Malformed(String),
    #[error("`{0}`: only USD is supported")]
    Currency(String),
}

impl Money {
    pub const ZERO: Money = Money(0);

    pub const fn from_micro_usd(micro: u64) -> Self {
        Self(micro)
    }

    pub const fn micro_usd(self) -> u64 {
        self.0
    }

    pub const fn plus(self, other: Money) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    /// `"1.00 USD"`, `"0.5 USD"`, `"$2"`, `"1"`: dollars, up to six decimals.
    pub fn parse(text: &str) -> Result<Self, MoneyError> {
        let text = text.trim();
        let malformed = || MoneyError::Malformed(text.to_string());
        let mut parts = text.split_whitespace();
        let amount = parts.next().ok_or_else(malformed)?;
        if let Some(currency) = parts.next()
            && !currency.eq_ignore_ascii_case("usd")
        {
            return Err(MoneyError::Currency(text.to_string()));
        }
        if parts.next().is_some() {
            return Err(malformed());
        }
        let amount = amount.strip_prefix('$').unwrap_or(amount);
        let (whole, fraction) = amount.split_once('.').unwrap_or((amount, ""));
        let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
        if !digits(whole) || !(fraction.is_empty() || digits(fraction)) || fraction.len() > 6 {
            return Err(malformed());
        }
        let whole: u64 = whole.parse().map_err(|_| malformed())?;
        let fraction: u64 = format!("{fraction:0<6}").parse().map_err(|_| malformed())?;
        Ok(Self(
            whole.saturating_mul(1_000_000).saturating_add(fraction),
        ))
    }
}

impl fmt::Display for Money {
    /// Cents, or ten-thousandths under a cent: `$1.20`, `$0.0007`, `$0.00`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let micro = self.0;
        if micro > 0 && micro < 10_000 {
            write!(f, "$0.{:04}", micro / 100)
        } else {
            let cents = (micro + 5_000) / 10_000;
            write!(f, "${}.{:02}", cents / 100, cents % 100)
        }
    }
}

/// A model's list price, in cents per million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Price {
    pub input_cents_per_mtok: u64,
    pub output_cents_per_mtok: u64,
}

impl Price {
    /// A cent per million tokens is a hundredth of a micro-dollar per token.
    pub const fn cost(self, tokens: Tokens) -> Money {
        Money(
            tokens
                .input
                .saturating_mul(self.input_cents_per_mtok)
                .saturating_add(tokens.output.saturating_mul(self.output_cents_per_mtok))
                / 100,
        )
    }
}

/// List prices as published by the providers, read on 2026-09-29 from
/// anthropic.com/pricing, openai.com/api/pricing and ai.google.dev/pricing;
/// standard tier, no caching, no batch. Cents per million tokens, input
/// then output. A model id matches the longest prefix; an OpenRouter-style
/// `vendor/model` id is matched after its slash.
const PRICES: &[(&str, u64, u64)] = &[
    ("claude-3-5-haiku", 80, 400),
    ("claude-3-5-sonnet", 300, 1500),
    ("claude-3-7-sonnet", 300, 1500),
    ("claude-haiku-4-5", 100, 500),
    ("claude-opus-4", 1500, 7500),
    ("claude-opus-4-1", 1500, 7500),
    ("claude-sonnet-4", 300, 1500),
    ("claude-sonnet-4-5", 300, 1500),
    ("gemini-2.0-flash", 10, 40),
    ("gemini-2.5-flash", 30, 250),
    ("gemini-2.5-flash-lite", 10, 40),
    ("gemini-2.5-pro", 125, 1000),
    ("gpt-4.1", 200, 800),
    ("gpt-4.1-mini", 40, 160),
    ("gpt-4.1-nano", 10, 40),
    ("gpt-4o", 250, 1000),
    ("gpt-4o-mini", 15, 60),
    ("gpt-5", 125, 1000),
    ("gpt-5-mini", 25, 200),
    ("gpt-5-nano", 5, 40),
    ("o3", 200, 800),
    ("o4-mini", 110, 440),
];

/// The list price of a model, when we know it.
pub fn list_price(model_id: &str) -> Option<Price> {
    let id = model_id.rsplit('/').next().unwrap_or(model_id);
    PRICES
        .iter()
        .filter(|(prefix, _, _)| id.starts_with(prefix))
        .max_by_key(|(prefix, _, _)| prefix.len())
        .map(|(_, input, output)| Price {
            input_cents_per_mtok: *input,
            output_cents_per_mtok: *output,
        })
}

/// What a model was asked to do, for the ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    QuickFix,
    Explain,
}

impl Task {
    pub const fn name(self) -> &'static str {
        match self {
            Task::QuickFix => "quick_fix",
            Task::Explain => "explain",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "quick_fix" => Some(Task::QuickFix),
            "explain" => Some(Task::Explain),
            _ => None,
        }
    }
}

/// One model call, as the ledger keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    pub at: Timestamp,
    pub model: String,
    pub task: Task,
    pub tokens: Tokens,
    /// `None` when the model's price is not known.
    pub cost: Option<Money>,
    pub latency: Duration,
}

/// A UTC calendar day, counted from the epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Day(u64);

const MILLIS_PER_DAY: u64 = 86_400_000;

impl Day {
    pub const fn of(at: Timestamp) -> Self {
        Self(at.as_millis() / MILLIS_PER_DAY)
    }

    pub const fn from_index(index: u64) -> Self {
        Self(index)
    }

    pub const fn index(self) -> u64 {
        self.0
    }

    /// Midnight UTC that starts this day.
    pub const fn start(self) -> Timestamp {
        Timestamp::from_millis(self.0 * MILLIS_PER_DAY)
    }

    /// This many days earlier, not before the epoch.
    pub const fn minus(self, days: u64) -> Self {
        Self(self.0.saturating_sub(days))
    }
}

impl fmt::Display for Day {
    /// `2026-09-29`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Howard Hinnant's civil-from-days, for a proleptic Gregorian date.
        let z = self.0 as i64 + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        write!(f, "{year:04}-{month:02}-{day:02}")
    }
}

/// What one model cost over a period.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSpend {
    pub model: String,
    pub calls: u64,
    pub tokens: Tokens,
    /// The priced calls' total.
    pub cost: Money,
    /// Calls whose model has no known price.
    pub unpriced: u64,
}

/// The priced total of these entries.
pub fn spent(entries: &[LedgerEntry]) -> Money {
    entries
        .iter()
        .filter_map(|e| e.cost)
        .fold(Money::ZERO, Money::plus)
}

/// Per model, in order of first appearance.
pub fn spend_by_model(entries: &[LedgerEntry]) -> Vec<ModelSpend> {
    let mut out: Vec<ModelSpend> = Vec::new();
    for entry in entries {
        let position = match out.iter().position(|s| s.model == entry.model) {
            Some(position) => position,
            None => {
                out.push(ModelSpend {
                    model: entry.model.clone(),
                    calls: 0,
                    tokens: Tokens::default(),
                    cost: Money::ZERO,
                    unpriced: 0,
                });
                out.len() - 1
            }
        };
        let spend = &mut out[position];
        spend.calls += 1;
        spend.tokens = spend.tokens.plus(entry.tokens);
        match entry.cost {
            Some(cost) => spend.cost = spend.cost.plus(cost),
            None => spend.unpriced += 1,
        }
    }
    out
}

/// Whether a cap is spent: no cap, no limit.
pub fn budget_reached(spent_today: Money, cap: Option<Money>) -> bool {
    cap.is_some_and(|cap| spent_today >= cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(model: &str, cost: Option<u64>, at: u64) -> LedgerEntry {
        LedgerEntry {
            at: Timestamp::from_millis(at),
            model: model.into(),
            task: Task::Explain,
            tokens: Tokens::new(100, 20),
            cost: cost.map(Money::from_micro_usd),
            latency: Duration::from_millis(300),
        }
    }

    #[test]
    fn amounts_are_read_in_dollars_and_only_in_dollars() {
        assert_eq!(
            Money::parse("1.00 USD").unwrap(),
            Money::from_micro_usd(1_000_000)
        );
        assert_eq!(
            Money::parse("0.5 usd").unwrap(),
            Money::from_micro_usd(500_000)
        );
        assert_eq!(
            Money::parse("$2").unwrap(),
            Money::from_micro_usd(2_000_000)
        );
        assert_eq!(Money::parse("0.000001").unwrap(), Money::from_micro_usd(1));
        assert!(matches!(
            Money::parse("1.00 EUR"),
            Err(MoneyError::Currency(_))
        ));
        for bad in ["", "abc", "1.2.3", "-1", "1.0000001", "1 USD extra"] {
            assert!(
                matches!(Money::parse(bad), Err(MoneyError::Malformed(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn amounts_print_in_cents_or_finer_under_a_cent() {
        assert_eq!(Money::from_micro_usd(1_200_000).to_string(), "$1.20");
        assert_eq!(Money::from_micro_usd(700).to_string(), "$0.0007");
        assert_eq!(Money::from_micro_usd(9_999).to_string(), "$0.0099");
        assert_eq!(Money::from_micro_usd(10_000).to_string(), "$0.01");
        assert_eq!(
            Money::from_micro_usd(15_000).to_string(),
            "$0.02",
            "rounded"
        );
        assert_eq!(Money::ZERO.to_string(), "$0.00");
    }

    #[test]
    fn a_known_model_is_priced_by_its_longest_prefix_and_unknown_ones_are_not() {
        let haiku = list_price("claude-haiku-4-5-20251001").unwrap();
        assert_eq!(
            haiku.cost(Tokens::new(1_000_000, 1_000_000)),
            Money::from_micro_usd(6_000_000),
            "$1 in, $5 out per million"
        );
        assert_eq!(
            haiku.cost(Tokens::new(1_000, 200)),
            Money::from_micro_usd(2_000)
        );
        assert_eq!(
            list_price("gpt-4.1-mini-2025-04-14")
                .unwrap()
                .input_cents_per_mtok,
            40,
            "gpt-4.1-mini, not gpt-4.1"
        );
        assert_eq!(
            list_price("anthropic/claude-sonnet-4-5")
                .unwrap()
                .output_cents_per_mtok,
            1500,
            "an OpenRouter id"
        );
        assert_eq!(list_price("qwen2.5-coder:7b"), None);
        let tiny = Price {
            input_cents_per_mtok: 1,
            output_cents_per_mtok: 1,
        };
        assert_eq!(
            tiny.cost(Tokens::new(3, 4)),
            Money::ZERO,
            "under a hundredth of a micro-dollar rounds down"
        );
    }

    #[test]
    fn days_are_utc_and_print_as_dates() {
        let day = Day::of(Timestamp::from_millis(1_790_637_207_000));
        assert_eq!(day.to_string(), "2026-09-28");
        assert_eq!(Day::of(day.start()), day);
        assert_eq!(
            Day::of(Timestamp::from_millis(day.start().as_millis() - 1)),
            day.minus(1)
        );
        assert_eq!(Day::from_index(0).to_string(), "1970-01-01");
        assert_eq!(Day::from_index(19_723).to_string(), "2024-01-01");
        assert_eq!(
            Day::from_index(20_513).to_string(),
            "2026-03-01",
            "after a leap day"
        );
    }

    #[test]
    fn spending_adds_the_priced_calls_and_counts_the_others() {
        let entries = vec![
            entry("haiku", Some(2_000), 1),
            entry("local", Some(0), 2),
            entry("mystery", None, 3),
            entry("haiku", Some(3_000), 4),
        ];
        assert_eq!(spent(&entries), Money::from_micro_usd(5_000));
        let by_model = spend_by_model(&entries);
        assert_eq!(by_model.len(), 3);
        assert_eq!(by_model[0].model, "haiku");
        assert_eq!(by_model[0].calls, 2);
        assert_eq!(by_model[0].tokens, Tokens::new(200, 40));
        assert_eq!(by_model[0].cost, Money::from_micro_usd(5_000));
        assert_eq!(by_model[2].unpriced, 1);
        assert!(!budget_reached(Money::from_micro_usd(5_000), None));
        assert!(!budget_reached(
            Money::from_micro_usd(5_000),
            Some(Money::from_micro_usd(5_001))
        ));
        assert!(budget_reached(
            Money::from_micro_usd(5_000),
            Some(Money::from_micro_usd(5_000))
        ));
    }

    #[test]
    fn tasks_round_trip_through_their_names() {
        for task in [Task::QuickFix, Task::Explain] {
            assert_eq!(Task::from_name(task.name()), Some(task));
        }
        assert_eq!(Task::from_name("dance"), None);
    }
}
