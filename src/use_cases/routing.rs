//! Which models may see a case, asking them in order until one answers,
//! and what each answer cost.

use crate::entities::{
    Day, Duration, FailureCase, LedgerEntry, ModelSpec, Money, Provider, Settings, Task, Tokens,
    budget_reached, list_price, spent,
};
use crate::use_cases::ports::{
    Answer, Clock, CostLedger, ModelError, ModelGateway, Prompt, Secrets,
};

/// The models named for a task, in order, that are allowed to see this
/// case: never a CLI agent, only local ones when the case holds a secret
/// and the user asked for that, and only local ones once the day's budget
/// is spent.
pub fn model_candidates<'a>(
    settings: &'a Settings,
    names: &[String],
    case: &FailureCase,
    over_budget: bool,
) -> Vec<&'a ModelSpec> {
    let local_only = (settings.sensitive_local_only && case.is_sensitive()) || over_budget;
    settings
        .candidates(names)
        .into_iter()
        .filter(|m| m.provider != Provider::CliAgent)
        .filter(|m| !local_only || m.is_local())
        .collect()
}

/// Whether some named model was dropped only because the case is sensitive.
pub fn excluded_for_sensitivity(settings: &Settings, names: &[String], case: &FailureCase) -> bool {
    settings.sensitive_local_only
        && case.is_sensitive()
        && settings
            .candidates(names)
            .iter()
            .any(|m| m.provider != Provider::CliAgent && !m.is_local())
}

/// Whether some named model was dropped because the day's budget is spent.
pub fn excluded_for_budget(settings: &Settings, names: &[String], over_budget: bool) -> bool {
    over_budget
        && settings
            .candidates(names)
            .iter()
            .any(|m| m.provider != Provider::CliAgent && !m.is_local())
}

/// The priced total of today's calls, UTC. A ledger that cannot be read
/// counts as empty: the budget is a comfort, not a lock.
pub fn spent_today(ledger: &dyn CostLedger, clock: &dyn Clock) -> Money {
    let day = Day::of(clock.now());
    ledger
        .since(day.start())
        .map(|entries| spent(&entries))
        .unwrap_or(Money::ZERO)
}

/// Whether today's spend reached the configured cap.
pub fn over_budget(settings: &Settings, ledger: &dyn CostLedger, clock: &dyn Clock) -> bool {
    settings.max_daily_cost.is_some()
        && budget_reached(spent_today(ledger, clock), settings.max_daily_cost)
}

/// Where an answer's cost goes: which ledger, whose clock, for which task.
pub struct Meter<'a> {
    pub ledger: &'a dyn CostLedger,
    pub clock: &'a dyn Clock,
    pub task: Task,
}

/// The first model that answers, with its name; otherwise every failure.
/// Each answer is metered: a local model costs nothing, a remote one its
/// list price on the tokens the provider counted.
#[allow(clippy::type_complexity)]
pub fn ask_first(
    models: &dyn ModelGateway,
    secrets: &dyn Secrets,
    candidates: &[&ModelSpec],
    prompt: &Prompt,
    meter: &Meter<'_>,
) -> Result<(String, Answer), Vec<(String, ModelError)>> {
    ask_in_order(secrets, candidates, meter, |spec, key| {
        models.answer(spec, key, prompt)
    })
}

/// `ask_first`, with the answer handed over as it comes: `on_chunk` gets
/// the model's name and the piece, so a caller can start afresh when a
/// model that had begun fails and the next one answers. A streamed answer
/// carries no token count, so a remote one is recorded at an unknown cost.
#[allow(clippy::type_complexity)]
pub fn ask_first_streaming(
    models: &dyn ModelGateway,
    secrets: &dyn Secrets,
    candidates: &[&ModelSpec],
    prompt: &Prompt,
    meter: &Meter<'_>,
    on_chunk: &mut dyn FnMut(&str, &str),
) -> Result<(String, String), Vec<(String, ModelError)>> {
    ask_in_order(secrets, candidates, meter, |spec, key| {
        models
            .stream(spec, key, prompt, &mut |chunk| on_chunk(&spec.name, chunk))
            .map(|text| Answer {
                text,
                tokens: Tokens::default(),
                fix: None,
            })
    })
    .map(|(name, answer)| (name, answer.text))
}

#[allow(clippy::type_complexity)]
fn ask_in_order(
    secrets: &dyn Secrets,
    candidates: &[&ModelSpec],
    meter: &Meter<'_>,
    mut ask: impl FnMut(&ModelSpec, Option<&str>) -> Result<Answer, ModelError>,
) -> Result<(String, Answer), Vec<(String, ModelError)>> {
    let mut failures = Vec::new();
    for spec in candidates {
        let key = secrets.lookup(&spec.key);
        let started = meter.clock.now();
        match ask(spec, key.as_deref()) {
            Ok(answer) if !answer.text.trim().is_empty() => {
                // A ledger that cannot be written must not cost the user
                // the answer it just paid for.
                let _ = meter.ledger.record(&metered(spec, meter, started, &answer));
                return Ok((spec.name.clone(), answer));
            }
            Ok(_) => failures.push((
                spec.name.clone(),
                ModelError::Malformed("empty answer".into()),
            )),
            Err(e) => failures.push((spec.name.clone(), e)),
        }
    }
    Err(failures)
}

/// A local answer costs nothing; a remote one its list price on the tokens
/// counted, or an unknown cost when nothing was counted or the model has
/// no list price. Never zero for a remote model.
fn metered(
    spec: &ModelSpec,
    meter: &Meter<'_>,
    started: crate::entities::Timestamp,
    answer: &Answer,
) -> LedgerEntry {
    let cost = if spec.is_local() {
        Some(Money::ZERO)
    } else if answer.tokens == Tokens::default() {
        None
    } else {
        list_price(&spec.model).map(|price| price.cost(answer.tokens))
    };
    LedgerEntry {
        at: started,
        model: spec.name.clone(),
        task: meter.task,
        tokens: answer.tokens,
        cost,
        latency: Duration::from_millis(
            meter
                .clock
                .now()
                .as_millis()
                .saturating_sub(started.as_millis()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{Duration, KeySource, Tier, Timestamp, Tokens};
    use crate::use_cases::ports::AnswerShape;
    use crate::use_cases::testing::{
        FakeClock, MapSecrets, MemoryLedger, ScriptedModels, case, spec,
    };

    fn settings() -> Settings {
        let mut cloud = spec("cloud", Provider::Anthropic, Tier::Small);
        cloud.key = KeySource::Env("ANTHROPIC_API_KEY".into());
        cloud.model = "claude-haiku-4-5-20251001".into();
        Settings {
            models: vec![
                cloud,
                spec("local", Provider::Ollama, Tier::Small),
                spec("claude", Provider::CliAgent, Tier::Agent),
            ],
            ..Default::default()
        }
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn prompt() -> Prompt {
        Prompt {
            system: "s".into(),
            user: "u".into(),
            max_tokens: 10,
            shape: AnswerShape::Prose,
        }
    }

    fn meter<'a>(ledger: &'a MemoryLedger, clock: &'a FakeClock) -> Meter<'a> {
        Meter {
            ledger,
            clock,
            task: Task::QuickFix,
        }
    }

    #[test]
    fn a_sensitive_case_only_reaches_local_models_and_agents_are_never_models() {
        let s = settings();
        let plain = case("gti status", 127, None);
        let secret = case(
            "curl -H 'Authorization: Bearer sk-live-abcdefghijklmnop' https://x",
            22,
            None,
        );
        let order = names(&["cloud", "local", "claude"]);
        let pick = |c: &FailureCase| {
            model_candidates(&s, &order, c, false)
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(pick(&plain), vec!["cloud", "local"]);
        assert_eq!(pick(&secret), vec!["local"]);
        assert!(excluded_for_sensitivity(&s, &order, &secret));
        assert!(!excluded_for_sensitivity(&s, &order, &plain));
        let mut relaxed = settings();
        relaxed.sensitive_local_only = false;
        assert_eq!(model_candidates(&relaxed, &order, &secret, false).len(), 2);
    }

    #[test]
    fn once_the_budget_is_spent_only_local_models_are_asked() {
        let s = settings();
        let plain = case("gti status", 127, None);
        let order = names(&["cloud", "local"]);
        let picked: Vec<&str> = model_candidates(&s, &order, &plain, true)
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        assert_eq!(picked, vec!["local"]);
        assert!(excluded_for_budget(&s, &order, true));
        assert!(!excluded_for_budget(&s, &order, false));
        assert!(
            !excluded_for_budget(&s, &names(&["local"]), true),
            "nothing remote was named"
        );
    }

    #[test]
    fn the_budget_is_todays_priced_spend_against_the_cap_in_utc() {
        let ledger = MemoryLedger::default();
        let today = Day::of(Timestamp::from_millis(1_790_637_207_000));
        let clock = FakeClock::at(today.start().as_millis() + 3_600_000);
        let entry = |at: Timestamp, micro: Option<u64>| LedgerEntry {
            at,
            model: "cloud".into(),
            task: Task::Explain,
            tokens: Tokens::new(1, 1),
            cost: micro.map(Money::from_micro_usd),
            latency: Duration::from_millis(1),
        };
        ledger
            .record(&entry(today.minus(1).start(), Some(900_000)))
            .unwrap();
        ledger.record(&entry(today.start(), Some(400_000))).unwrap();
        ledger.record(&entry(clock.now(), None)).unwrap();
        assert_eq!(
            spent_today(&ledger, &clock),
            Money::from_micro_usd(400_000),
            "yesterday and the unpriced call do not count"
        );
        let mut s = settings();
        assert!(!over_budget(&s, &ledger, &clock), "no cap, no limit");
        s.max_daily_cost = Some(Money::from_micro_usd(500_000));
        assert!(!over_budget(&s, &ledger, &clock));
        s.max_daily_cost = Some(Money::from_micro_usd(400_000));
        assert!(
            over_budget(&s, &ledger, &clock),
            "reaching the cap is spending it"
        );
    }

    #[test]
    fn models_are_asked_in_order_until_one_answers_with_their_key() {
        let s = settings();
        let models = ScriptedModels::answering(&[
            ("cloud", Err(ModelError::Unreachable("timeout".into()))),
            ("local", Ok("because")),
        ]);
        let secrets = MapSecrets::with(&[("ANTHROPIC_API_KEY", "k-123")]);
        let candidates: Vec<&ModelSpec> =
            vec![s.model("cloud").unwrap(), s.model("local").unwrap()];
        let (ledger, clock) = (MemoryLedger::default(), FakeClock::at(5));
        let (name, answer) = ask_first(
            &models,
            &secrets,
            &candidates,
            &prompt(),
            &meter(&ledger, &clock),
        )
        .unwrap();
        assert_eq!((name.as_str(), answer.text.as_str()), ("local", "because"));
        assert_eq!(models.asked(), vec!["cloud", "local"]);
        assert_eq!(models.calls.borrow()[0].1.as_deref(), Some("k-123"));
        assert_eq!(models.calls.borrow()[1].1, None);
    }

    #[test]
    fn every_answer_is_metered_local_ones_for_nothing_remote_ones_at_list_price() {
        let s = settings();
        let (ledger, clock) = (MemoryLedger::default(), FakeClock::at(5_000));
        let models = ScriptedModels::answering(&[("cloud", Ok("try this"))])
            .counting(&[("cloud", Tokens::new(1_000, 200))]);
        let candidates: Vec<&ModelSpec> = vec![s.model("cloud").unwrap()];
        ask_first(
            &models,
            &MapSecrets::with(&[]),
            &candidates,
            &prompt(),
            &meter(&ledger, &clock),
        )
        .unwrap();
        let entries = ledger.entries.borrow();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].model, "cloud");
        assert_eq!(entries[0].task, Task::QuickFix);
        assert_eq!(entries[0].tokens, Tokens::new(1_000, 200));
        assert_eq!(
            entries[0].cost,
            Some(Money::from_micro_usd(2_000)),
            "$1 per million in, $5 per million out"
        );
        assert_eq!(entries[0].at, Timestamp::from_millis(5_000));
        drop(entries);

        let local = ScriptedModels::answering(&[("local", Ok("because"))])
            .counting(&[("local", Tokens::new(9, 9))]);
        ask_first(
            &local,
            &MapSecrets::with(&[]),
            &[s.model("local").unwrap()],
            &prompt(),
            &meter(&ledger, &clock),
        )
        .unwrap();
        assert_eq!(ledger.entries.borrow()[1].cost, Some(Money::ZERO));

        let mut unknown = spec("mystery", Provider::OpenAiCompatible, Tier::Small);
        unknown.model = "some-new-model".into();
        let mystery = ScriptedModels::answering(&[("mystery", Ok("hm"))]);
        ask_first(
            &mystery,
            &MapSecrets::with(&[]),
            &[&unknown],
            &prompt(),
            &meter(&ledger, &clock),
        )
        .unwrap();
        assert_eq!(
            ledger.entries.borrow()[2].cost,
            None,
            "no list price, no cost: never zero"
        );
    }

    #[test]
    fn streaming_asks_in_the_same_order_and_names_the_model_on_every_piece() {
        let s = settings();
        let (ledger, clock) = (MemoryLedger::default(), FakeClock::at(5_000));
        let models = ScriptedModels::answering(&[
            ("cloud", Err(ModelError::Unreachable("timeout".into()))),
            ("local", Ok("Node is too old. Use 22.")),
        ]);
        let candidates: Vec<&ModelSpec> =
            vec![s.model("cloud").unwrap(), s.model("local").unwrap()];
        let mut pieces = Vec::new();
        let (name, answer) = ask_first_streaming(
            &models,
            &MapSecrets::with(&[]),
            &candidates,
            &prompt(),
            &meter(&ledger, &clock),
            &mut |model, chunk| pieces.push(format!("{model}:{chunk}")),
        )
        .unwrap();
        assert_eq!(
            (name.as_str(), answer.as_str()),
            ("local", "Node is too old. Use 22.")
        );
        assert_eq!(
            pieces,
            vec![
                "local:Node ",
                "local:is ",
                "local:too ",
                "local:old. ",
                "local:Use ",
                "local:22."
            ]
        );
        assert_eq!(models.asked(), vec!["cloud", "local"]);
        let entries = ledger.entries.borrow();
        assert_eq!(entries.len(), 1, "the answer that came is metered");
        assert_eq!(entries[0].cost, Some(Money::ZERO), "local: free");
    }

    #[test]
    fn a_streamed_remote_answer_has_an_unknown_cost_not_a_zero_one() {
        let s = settings();
        let (ledger, clock) = (MemoryLedger::default(), FakeClock::at(5_000));
        let models = ScriptedModels::answering(&[("cloud", Ok("try this"))]);
        ask_first_streaming(
            &models,
            &MapSecrets::with(&[]),
            &[s.model("cloud").unwrap()],
            &prompt(),
            &meter(&ledger, &clock),
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(ledger.entries.borrow()[0].cost, None);
    }

    #[test]
    fn when_nobody_answers_every_failure_is_reported_and_nothing_is_metered() {
        let s = settings();
        let models = ScriptedModels::answering(&[
            ("cloud", Ok("  ")),
            ("local", Err(ModelError::Refused("429".into()))),
        ]);
        let candidates: Vec<&ModelSpec> =
            vec![s.model("cloud").unwrap(), s.model("local").unwrap()];
        let (ledger, clock) = (MemoryLedger::default(), FakeClock::at(5));
        let failures = ask_first(
            &models,
            &MapSecrets::with(&[]),
            &candidates,
            &prompt(),
            &meter(&ledger, &clock),
        )
        .unwrap_err();
        assert_eq!(failures.len(), 2);
        assert_eq!(failures[0].1, ModelError::Malformed("empty answer".into()));
        assert_eq!(failures[1].1, ModelError::Refused("429".into()));
        assert!(ledger.entries.borrow().is_empty());
    }
}
