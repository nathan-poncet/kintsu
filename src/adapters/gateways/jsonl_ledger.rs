//! The cost ledger as a file: one JSON line per model call, appended, and
//! a line per day the user was told the budget is spent.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::entities::{Day, Duration, LedgerEntry, Money, Task, Timestamp, Tokens};
use crate::use_cases::ports::{CostLedger, LedgerError};

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
    use crate::use_cases::testing::cost_ledger_contract;

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
        let entries = ledger.since(Timestamp::from_millis(0)).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].cost, None);
        assert_eq!(entries[0].task, Task::Explain);
    }
}
