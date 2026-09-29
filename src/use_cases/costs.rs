//! `kintsu costs`: what the models cost today and over the last thirty
//! days, per model, against the daily cap.

use crate::entities::{Day, ModelSpend, Money, Settings, spend_by_model, spent};
use crate::use_cases::ports::{Clock, CostLedger, LedgerError};

/// The ledger, summed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostReport {
    /// Today, UTC.
    pub day: Day,
    pub today: Vec<ModelSpend>,
    pub today_total: Money,
    /// The last thirty days, today included.
    pub month: Vec<ModelSpend>,
    pub month_total: Money,
    pub cap: Option<Money>,
}

/// Sums the ledger.
pub struct Costs<'a> {
    pub settings: &'a Settings,
    pub ledger: &'a dyn CostLedger,
    pub clock: &'a dyn Clock,
}

impl Costs<'_> {
    pub fn run(&self) -> Result<CostReport, LedgerError> {
        let day = Day::of(self.clock.now());
        let month = self.ledger.since(day.minus(29).start())?;
        let today: Vec<_> = month
            .iter()
            .filter(|e| e.at >= day.start())
            .cloned()
            .collect();
        Ok(CostReport {
            day,
            today_total: spent(&today),
            today: spend_by_model(&today),
            month_total: spent(&month),
            month: spend_by_model(&month),
            cap: self.settings.max_daily_cost,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{Duration, LedgerEntry, Task, Timestamp, Tokens};
    use crate::use_cases::testing::{FakeClock, MemoryLedger};

    #[test]
    fn today_and_the_month_are_summed_per_model_against_the_cap() {
        let ledger = MemoryLedger::default();
        let today = Day::of(Timestamp::from_millis(1_790_637_207_000));
        let clock = FakeClock::at(today.start().as_millis() + 1_000);
        let entry = |at: Timestamp, model: &str, micro: Option<u64>| LedgerEntry {
            at,
            model: model.into(),
            task: Task::Explain,
            tokens: Tokens::new(100, 10),
            cost: micro.map(Money::from_micro_usd),
            latency: Duration::from_millis(1),
        };
        ledger
            .record(&entry(today.minus(40).start(), "haiku", Some(9_000_000)))
            .unwrap();
        ledger
            .record(&entry(today.minus(3).start(), "haiku", Some(200_000)))
            .unwrap();
        ledger
            .record(&entry(today.start(), "local", Some(0)))
            .unwrap();
        ledger
            .record(&entry(clock.now(), "haiku", Some(50_000)))
            .unwrap();
        ledger.record(&entry(clock.now(), "mystery", None)).unwrap();
        let settings = Settings {
            max_daily_cost: Some(Money::from_micro_usd(1_000_000)),
            ..Settings::default()
        };
        let report = Costs {
            settings: &settings,
            ledger: &ledger,
            clock: &clock,
        }
        .run()
        .unwrap();
        assert_eq!(report.day, today);
        assert_eq!(report.today_total, Money::from_micro_usd(50_000));
        assert_eq!(report.today.len(), 3);
        assert_eq!(report.today[1].model, "haiku");
        assert_eq!(report.today[2].unpriced, 1);
        assert_eq!(
            report.month_total,
            Money::from_micro_usd(250_000),
            "forty days ago is out of the month"
        );
        assert_eq!(report.month[0].calls, 2, "haiku, three days ago and today");
        assert_eq!(report.cap, Some(Money::from_micro_usd(1_000_000)));
    }
}
