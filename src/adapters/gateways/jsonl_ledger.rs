//! The cost ledger as a file: one JSON line per model call, appended, a
//! line per day the user was told the budget is spent, and the scoreboard's
//! lines, what was offered and what was taken, in the same file.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::entities::{
    Day, Duration, FixEvent, FixEventKind, FixSource, LedgerEntry, Money, Task, Timestamp, Tokens,
};
use crate::use_cases::ports::{CostLedger, LedgerError, Scoreboard, ScoreboardError};

pub struct JsonlLedger {
    path: PathBuf,
}

impl JsonlLedger {
    /// `<state>/ledger.jsonl`.
    pub fn new(state_dir: &Path) -> Self {
        Self {
            path: state_dir.join("ledger.jsonl"),
        }
    }

    fn append(&self, line: &Value) -> Result<(), LedgerError> {
        let io = |e: std::io::Error| LedgerError::Io(format!("{}: {e}", self.path.display()));
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(io)?;
        writeln!(file, "{line}").map_err(io)
    }

    /// Every line that parses; a damaged line is skipped, not fatal.
    fn lines(&self) -> Result<Vec<Value>, LedgerError> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(text
                .lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(LedgerError::Io(format!("{}: {e}", self.path.display()))),
        }
    }
}

fn entry_from(line: &Value) -> Option<LedgerEntry> {
    if line["kind"] != "call" {
        return None;
    }
    Some(LedgerEntry {
        at: Timestamp::from_millis(line["at"].as_u64()?),
        model: line["model"].as_str()?.to_string(),
        task: Task::from_name(line["task"].as_str()?)?,
        tokens: Tokens::new(
            line["input"].as_u64().unwrap_or(0),
            line["output"].as_u64().unwrap_or(0),
        ),
        cost: line["cost_micro_usd"].as_u64().map(Money::from_micro_usd),
        latency: Duration::from_millis(line["latency_ms"].as_u64().unwrap_or(0)),
    })
}

fn source_json(source: &FixSource) -> (&'static str, &str) {
    match source {
        FixSource::Rule(name) => ("rule", name),
        FixSource::Model(name) => ("model", name),
    }
}

fn source_from(line: &Value) -> Option<FixSource> {
    let name = line["source"].as_str()?.to_string();
    match line["source_kind"].as_str()? {
        "rule" => Some(FixSource::Rule(name)),
        "model" => Some(FixSource::Model(name)),
        _ => None,
    }
}

fn event_from(line: &Value) -> Option<FixEvent> {
    let kind = match line["kind"].as_str()? {
        "failure" => FixEventKind::Failure,
        "offered" => FixEventKind::Offered(source_from(line)?),
        "taken" => FixEventKind::Taken(source_from(line)?),
        _ => return None,
    };
    Some(FixEvent {
        at: Timestamp::from_millis(line["at"].as_u64()?),
        kind,
    })
}

impl Scoreboard for JsonlLedger {
    fn mark(&self, event: &FixEvent) -> Result<(), ScoreboardError> {
        let mut line = json!({"at": event.at.as_millis()});
        match &event.kind {
            FixEventKind::Failure => line["kind"] = json!("failure"),
            FixEventKind::Offered(source) | FixEventKind::Taken(source) => {
                let (kind, name) = source_json(source);
                line["kind"] = json!(if matches!(event.kind, FixEventKind::Offered(_)) {
                    "offered"
                } else {
                    "taken"
                });
                line["source_kind"] = json!(kind);
                line["source"] = json!(name);
            }
        }
        self.append(&line)
            .map_err(|LedgerError::Io(text)| ScoreboardError::Io(text))
    }

    fn since(&self, from: Timestamp) -> Result<Vec<FixEvent>, ScoreboardError> {
        let lines = self
            .lines()
            .map_err(|LedgerError::Io(text)| ScoreboardError::Io(text))?;
        Ok(lines
            .iter()
            .filter_map(event_from)
            .filter(|e| e.at >= from)
            .collect())
    }
}

impl CostLedger for JsonlLedger {
    fn record(&self, entry: &LedgerEntry) -> Result<(), LedgerError> {
        self.append(&json!({
            "kind": "call",
            "at": entry.at.as_millis(),
            "model": entry.model,
            "task": entry.task.name(),
            "input": entry.tokens.input,
            "output": entry.tokens.output,
            "cost_micro_usd": entry.cost.map(Money::micro_usd),
            "latency_ms": entry.latency.as_millis(),
        }))
    }

    fn since(&self, from: Timestamp) -> Result<Vec<LedgerEntry>, LedgerError> {
        Ok(self
            .lines()?
            .iter()
            .filter_map(entry_from)
            .filter(|e| e.at >= from)
            .collect())
    }

    fn budget_noted(&self, day: Day) -> Result<bool, LedgerError> {
        Ok(self
            .lines()?
            .iter()
            .any(|l| l["kind"] == "budget_noted" && l["day"].as_u64() == Some(day.index())))
    }

    fn note_budget(&self, day: Day) -> Result<(), LedgerError> {
        self.append(&json!({"kind": "budget_noted", "day": day.index()}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::use_cases::testing::{cost_ledger_contract, scoreboard_contract};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kintsu-ledger-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_file_ledger_honours_the_contract() {
        let dir = scratch("contract");
        cost_ledger_contract(&JsonlLedger::new(&dir));
        assert!(dir.join("ledger.jsonl").is_file(), "created on first write");
    }

    #[test]
    fn the_file_scoreboard_honours_the_contract_and_shares_the_ledgers_file() {
        let dir = scratch("scoreboard");
        let ledger = JsonlLedger::new(&dir);
        scoreboard_contract(&ledger);
        ledger
            .record(&LedgerEntry {
                at: Timestamp::from_millis(5),
                model: "haiku".into(),
                task: Task::Explain,
                tokens: Tokens::new(1, 2),
                cost: None,
                latency: Duration::from_millis(9),
            })
            .unwrap();
        assert_eq!(
            CostLedger::since(&ledger, Timestamp::from_millis(0))
                .unwrap()
                .len(),
            1,
            "the ledger does not read the scoreboard's lines"
        );
        assert_eq!(
            Scoreboard::since(&ledger, Timestamp::from_millis(0))
                .unwrap()
                .len(),
            3,
            "nor the scoreboard the ledger's"
        );
        assert!(dir.join("ledger.jsonl").is_file());
    }

    #[test]
    fn a_damaged_line_is_skipped_and_the_rest_still_read() {
        let dir = scratch("damaged");
        let ledger = JsonlLedger::new(&dir);
        ledger
            .record(&LedgerEntry {
                at: Timestamp::from_millis(5),
                model: "haiku".into(),
                task: Task::Explain,
                tokens: Tokens::new(1, 2),
                cost: None,
                latency: Duration::from_millis(9),
            })
            .unwrap();
        let mut file = OpenOptions::new()
            .append(true)
            .open(dir.join("ledger.jsonl"))
            .unwrap();
        writeln!(file, "{{not json").unwrap();
        writeln!(file, "{{\"kind\":\"call\",\"at\":\"soon\"}}").unwrap();
        let entries = CostLedger::since(&ledger, Timestamp::from_millis(0)).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].cost, None);
        assert_eq!(entries[0].task, Task::Explain);
    }
}
