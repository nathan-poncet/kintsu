//! `kintsu stats`: the failures looked at, the fixes offered and taken by
//! each rule and model, and the explanations asked, over the last thirty
//! days.

use crate::entities::{Day, Scorecard, Task, score};
use crate::use_cases::ports::{Clock, CostLedger, Scoreboard, ScoreboardError};

/// How far back the report looks, today included.
pub const DAYS: u64 = 30;

/// The scoreboard, tallied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatsReport {
    /// Today, UTC.
    pub day: Day,
    pub card: Scorecard,
    /// Model explanations asked over the period, from the cost ledger.
    pub explanations: u64,
}

/// Tallies the scoreboard and counts the explanations.
pub struct Stats<'a> {
    pub scoreboard: &'a dyn Scoreboard,
    pub ledger: &'a dyn CostLedger,
    pub clock: &'a dyn Clock,
}

impl Stats<'_> {
    pub fn run(&self) -> Result<StatsReport, ScoreboardError> {
        let day = Day::of(self.clock.now());
        let from = day.minus(DAYS - 1).start();
        let events = self.scoreboard.since(from)?;
        // A ledger that cannot be read costs the count, not the report.
        let explanations = self
            .ledger
            .since(from)
            .map(|calls| calls.iter().filter(|c| c.task == Task::Explain).count() as u64)
            .unwrap_or(0);
        Ok(StatsReport {
            day,
            card: score(&events, day, DAYS),
            explanations,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{
        Duration, FixEvent, FixEventKind, FixSource, LedgerEntry, Timestamp, Tokens,
    };
    use crate::use_cases::testing::{FakeClock, MemoryLedger, MemoryScoreboard};

    #[test]
    fn the_last_thirty_days_are_tallied_and_explanations_counted_from_the_ledger() {
        let board = MemoryScoreboard::default();
        let ledger = MemoryLedger::default();
        let today = Day::from_index(20_724);
        let clock = FakeClock::at(today.start().as_millis() + 5_000);
        let mark = |day: Day, kind: FixEventKind| {
            board
                .mark(&FixEvent {
                    at: Timestamp::from_millis(day.start().as_millis() + 1),
                    kind,
                })
                .unwrap()
        };
        mark(today.minus(40), FixEventKind::Failure);
        mark(today.minus(2), FixEventKind::Failure);
        mark(
            today.minus(2),
            FixEventKind::Offered(FixSource::Model("local".into())),
        );
        mark(
            today.minus(1),
            FixEventKind::Taken(FixSource::Model("local".into())),
        );
        mark(today, FixEventKind::Failure);
        for (task, at) in [
            (Task::Explain, today.minus(3)),
            (Task::QuickFix, today),
            (Task::Explain, today.minus(31)),
        ] {
            ledger
                .record(&LedgerEntry {
                    at: at.start(),
                    model: "local".into(),
                    task,
                    tokens: Tokens::default(),
                    cost: None,
                    latency: Duration::from_millis(1),
                })
                .unwrap();
        }
        let report = Stats {
            scoreboard: &board,
            ledger: &ledger,
            clock: &clock,
        }
        .run()
        .unwrap();
        assert_eq!(report.day, today);
        assert_eq!(report.card.days.len(), 30);
        assert_eq!(report.card.failures, 2, "the one 40 days ago is out");
        assert_eq!((report.card.offered, report.card.taken), (1, 1));
        assert_eq!(report.card.sources[0].rate(), Some(1.0));
        assert_eq!(report.explanations, 1, "only the one inside the window");
    }
}
