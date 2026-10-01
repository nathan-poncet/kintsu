//! What was offered and what was taken: the score of the rules and the
//! models, and how many failures were looked at, day by day.

use crate::entities::{Day, FixSource, Timestamp};

/// One thing that happened to a failure, as the scoreboard keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixEvent {
    pub at: Timestamp,
    pub kind: FixEventKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixEventKind {
    /// A failure got a bubble, with or without a fix.
    Failure,
    /// This rule or model proposed a fix.
    Offered(FixSource),
    /// The proposed fix was run next and succeeded.
    Taken(FixSource),
}

/// One source's score over a period.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceScore {
    pub source: FixSource,
    pub offered: u64,
    pub taken: u64,
}

impl SourceScore {
    /// Taken over offered, when anything was offered.
    pub fn rate(&self) -> Option<f32> {
        (self.offered > 0).then(|| self.taken as f32 / self.offered as f32)
    }
}

/// How many failures one day brought.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayCount {
    pub day: Day,
    pub failures: u64,
}

/// The tally of a period.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scorecard {
    /// Every day of the period, oldest first, zero days included.
    pub days: Vec<DayCount>,
    /// The sources that offered most first, then by name.
    pub sources: Vec<SourceScore>,
    pub failures: u64,
    pub offered: u64,
    pub taken: u64,
}

/// Tallies the events of the `days` days that end on `today`. Events
/// outside that window are ignored, so the caller may hand over more.
pub fn score(events: &[FixEvent], today: Day, days: u64) -> Scorecard {
    let first = today.minus(days.saturating_sub(1));
    let in_window = |e: &&FixEvent| {
        let day = Day::of(e.at);
        day >= first && day <= today
    };
    let mut card = Scorecard {
        days: (first.index()..=today.index())
            .map(|index| DayCount {
                day: Day::from_index(index),
                failures: 0,
            })
            .collect(),
        sources: Vec::new(),
        failures: 0,
        offered: 0,
        taken: 0,
    };
    for event in events.iter().filter(in_window) {
        match &event.kind {
            FixEventKind::Failure => {
                card.failures += 1;
                let index = (Day::of(event.at).index() - first.index()) as usize;
                card.days[index].failures += 1;
            }
            FixEventKind::Offered(source) => {
                card.offered += 1;
                card.source(source).offered += 1;
            }
            FixEventKind::Taken(source) => {
                card.taken += 1;
                card.source(source).taken += 1;
            }
        }
    }
    card.sources.sort_by(|a, b| {
        b.offered
            .cmp(&a.offered)
            .then_with(|| a.source.to_string().cmp(&b.source.to_string()))
    });
    card
}

impl Scorecard {
    fn source(&mut self, source: &FixSource) -> &mut SourceScore {
        let position = match self.sources.iter().position(|s| &s.source == source) {
            Some(position) => position,
            None => {
                self.sources.push(SourceScore {
                    source: source.clone(),
                    offered: 0,
                    taken: 0,
                });
                self.sources.len() - 1
            }
        };
        &mut self.sources[position]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY_MS: u64 = 86_400_000;

    fn at(day: u64, kind: FixEventKind) -> FixEvent {
        FixEvent {
            at: Timestamp::from_millis(day * DAY_MS + 1_000),
            kind,
        }
    }

    fn rule(name: &str) -> FixSource {
        FixSource::Rule(name.into())
    }

    #[test]
    fn failures_are_counted_per_day_over_the_window_and_zero_days_are_kept() {
        let today = Day::from_index(100);
        let events = [
            at(98, FixEventKind::Failure),
            at(98, FixEventKind::Failure),
            at(100, FixEventKind::Failure),
            at(97, FixEventKind::Failure),
        ];
        let card = score(&events, today, 3);
        assert_eq!(
            card.days,
            vec![
                DayCount {
                    day: Day::from_index(98),
                    failures: 2
                },
                DayCount {
                    day: Day::from_index(99),
                    failures: 0
                },
                DayCount {
                    day: Day::from_index(100),
                    failures: 1
                },
            ]
        );
        assert_eq!(card.failures, 3, "the one before the window is left out");
    }

    #[test]
    fn sources_are_scored_offered_against_taken_most_offered_first() {
        let today = Day::from_index(10);
        let events = [
            at(10, FixEventKind::Offered(rule("command typo"))),
            at(10, FixEventKind::Taken(rule("command typo"))),
            at(9, FixEventKind::Offered(FixSource::Model("local".into()))),
            at(9, FixEventKind::Offered(FixSource::Model("local".into()))),
            at(9, FixEventKind::Offered(FixSource::Model("local".into()))),
            at(9, FixEventKind::Taken(FixSource::Model("local".into()))),
            at(8, FixEventKind::Offered(rule("sudo"))),
        ];
        let card = score(&events, today, 30);
        let names: Vec<String> = card.sources.iter().map(|s| s.source.to_string()).collect();
        assert_eq!(
            names,
            vec!["model · local", "rule · command typo", "rule · sudo"]
        );
        assert_eq!((card.sources[0].offered, card.sources[0].taken), (3, 1));
        assert_eq!(card.sources[1].rate(), Some(1.0));
        assert_eq!(card.sources[2].rate(), Some(0.0));
        assert_eq!((card.offered, card.taken), (5, 2));
        assert_eq!(
            SourceScore {
                source: rule("x"),
                offered: 0,
                taken: 0
            }
            .rate(),
            None
        );
    }
}
