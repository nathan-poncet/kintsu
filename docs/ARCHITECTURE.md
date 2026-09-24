# Architecture

Kintsu is one Rust application that follows Clean Architecture the way the
author's other projects do: entities and use cases alone at the centre, an
interface-adapters ring around them split into controllers, gateways and
presenters, and `main.rs` at the very edge as the composition root. The
rings are folders in a single crate. The Dependency Rule is enforced by
`tests/dependency_rule.rs`, which reads the sources on every `cargo test`
and fails when an inner ring reaches outward or touches I/O, a runtime, a
terminal or the clock.

One crate on purpose: Kintsu is an application, not a library. A workspace
would only earn its keep the day adapters must be swapped as separate
crates, or the day a second deliverable needs Rust; the website does not,
it is static files under `docs/`.

*Status: the folders exist and hold the day-zero skeleton. Most modules
below are the target shape, named now so that the first real pieces land
in the right place. The process model is in [DAEMON.md](DAEMON.md), model
routing in [MODELS.md](MODELS.md), the bubble in [UI.md](UI.md).*

## The map

```
kintsu/
├── Cargo.toml                    one package, one binary
├── config/default.toml           the commented default configuration (`kintsu default-config`)
├── shell/                        kintsu.zsh · kintsu.bash · kintsu.fish   (the hooks, plain assets)
├── tests/
│   └── dependency_rule.rs        the rings may only reach inward; the inner two do no I/O
├── docs/                         the design documents, DECISIONS.md, and the website
│
└── src/
    ├── main.rs                   reads the environment once (paths, session, colour) and calls app::run
    ├── app.rs                    composition root: one arm per subcommand, builds gateways, calls a
    │                             use case, hands the result to a presenter
    ├── daemon.rs                 the resident process: socket listener, one function per frame,
    │                             background workers for the Messages use case
    ├── (planned)                 service.rs (launchd, systemd, URL scheme)
    │
    ├── entities/                 enterprise rules, no I/O, depends on nothing else in the crate
    │   ├── command.rs                CommandLine
    │   ├── exit_status.rs            ExitStatus (interruptions, 126, 127)
    │   ├── outcome.rs                CommandOutcome, its fingerprint
    │   ├── time.rs                   Timestamp · Duration, as values
    │   ├── session.rs                Session · SessionId (the last 20 outcomes of a shell)
    │   ├── case.rs                   FailureCase · CaseId (outcome, cwd, session, recent, output)
    │   ├── fix.rs                    Fix · Confidence · FixSource
    │   ├── danger.rs                 Danger · classify_danger
    │   ├── distance.rs               edit distance, closest candidate with tie rules
    │   ├── rules.rs                  Facts · Os · DirEntry · suggest_fix (the five instant rules)
    │   ├── redaction.rs              redact · Redacted · SecretKind
    │   ├── ignore.rs                 IgnoreEntry · IgnoreTarget · IgnoreScope
    │   ├── settings.rs               Settings · ModelSpec · Provider · Tier · KeySource · Routing · QuietSettings · UiSettings
    │   ├── brief.rs                  case_document · hand_off_brief (output fenced as data)
    │   ├── message.rs                Message · MessageBody (what arrives later)
    │   ├── triage.rs                 TriageDecision · QuietReason
    │   ├── shell.rs                  Shell
    │   └── (planned)                 CapturedOutput · Task · Budget · Bubble · Action
    │
    ├── use_cases/                application rules, depends on the entities only
    │   ├── triage.rs                 record, quiet checks, ignore, duplicate, rules → fix, save
    │   ├── fix_last.rs               rules first, then a stored proposal, then the quick-fix model
    │   ├── messages.rs               what the daemon sends later: the eager fix, the explanation, or why not
    │   ├── explain.rs                routed models, sensitive ⇒ local only
    │   ├── hand_off.rs               prepare the brief, then launch the agent
    │   ├── ignore.rs                 ignore and mute
    │   ├── privacy.rs                what would be sent
    │   ├── diagnose.rs               doctor's checks
    │   ├── facts.rs · prompts.rs · routing.rs
    │   ├── testing.rs                in-memory fakes of every port (cfg(test))
    │   ├── (planned)                 register_session · classify_failure · recall_case · learn_rule
    │   └── ports/                    one trait per file, role nouns, each owning its error type
    │       ├── clock.rs · ids.rs · environment.rs · session_registry.rs · case_store.rs
    │       ├── ignore_store.rs · secrets.rs · model_gateway.rs · agent_launcher.rs · notifier.rs
    │       └── (planned)             OutputSource · RuleBook · CostLedger
    │
    └── adapters/                 depends on the entities and the use cases
        ├── controllers/              cli.rs (argv → Command; the environment is main's job) ·
        │   │                         socket.rs (daemon frames → Request)
        │   └── (planned)             url_scheme.rs (kintsu://)
        ├── presenters/               style.rs (the seam) · toast.rs (toast, message_toast) · plain.rs ·
        │   │                         doctor.rs · shell_hook.rs · frames.rs (daemon → client frames)
        │   └── (planned)             panel/ (ratatui, inline viewport) · ghost_text.rs · json.rs
        └── gateways/                 daemon_client.rs (the thin client, spawns the daemon) ·
            │                         sessions.rs (Notifier: subscribers, SIGUSR1, pending messages) ·
            │                         asking_marker.rs (the file the hooks read) · ndjson.rs · unix.rs (libc) ·
            │                         json_state.rs (SessionRegistry + CaseStore + IgnoreStore) ·
            │                         toml_settings.rs · http_models.rs (Ollama, OpenAI-compatible, Anthropic) ·
            │                         shell_agents.rs (CLI agents via sh) · fs_environment.rs ·
            │                         env_secrets.rs (env, command, keychain) · system_clock.rs · random_ids.rs
            └── (planned)             output_sources/{tmux,herdr,wezterm,kitty,iterm2,stderr_tee}.rs ·
                                      store/sqlite.rs · notify/{zle_fd,signal,next_prompt}.rs
```

## The rings

1. **Entities (`src/entities/`).** What a command line is, what it
   returned, what a failure case contains, what a fix is, how dangerous it
   is, what a task for a model is and which tier may run it. Newtypes over
   primitives, enums over strings and bools, invariants enforced at
   construction. May use `thiserror`, later `regex` for the rules and the
   redaction patterns. Nothing that does I/O, nothing that knows the time,
   nothing from `use_cases` or `adapters`.

2. **Use cases (`src/use_cases/`).** One interactor per thing a user or a
   hook can ask for. They orchestrate entities against ports and hold the
   application policy: when to stay quiet, what context to collect, which
   task to route where, what must never run without confirmation.
   - **Ports** live in `use_cases/ports/`: role-noun traits, one per file,
     each owning its error type. Today: `Clock`, `IdGenerator`,
     `Environment`, `SessionRegistry`, `CaseStore`, `IgnoreStore`,
     `Secrets`, `ModelGateway`, `AgentLauncher`, `Notifier`. Planned:
     `OutputSource`, `RuleBook`, `CostLedger`.
   - Async-agnostic: a port that waits on the world returns `impl Future`.
     No executor, no `tokio`, no channel type from a runtime crate. The
     daemon decides how futures are driven.
   - Every port ships an in-memory fake next to its contract test, so a
     use case is tested in microseconds and every gateway is tested against
     the same suite.

3. **Interface adapters (`src/adapters/`).** Gateways for sure; controllers
   and presenters kept thin, and kept:
   - **Controllers**: translate an input protocol into a use case call and
     nothing else. Three inputs exist: the command line (`cli.rs`), the
     daemon protocol (`socket.rs`, frames from hooks and clients), and the
     URL scheme behind clickable bubbles (`url_scheme.rs`). Each parses,
     validates at the edge, and hands typed values to a use case.
   - **Presenters**: pure mapping from what a use case produced to a view
     state a surface renders verbatim. Four surfaces justify them: the
     toast (ANSI text with OSC 8 links), the panel (a ratatui view model),
     plain text for `--print` and pipes, and JSON for scripts and the MCP
     server. Presenters decide every string, every key hint, every
     abbreviation; they never read the clock or the terminal size, both
     are arguments.
   - **Gateways**: implement the ports over the outside world. Model
     providers, agent launchers, output sources per terminal and
     multiplexer, the SQLite case store, the TOML config, the keychains,
     the notifiers that deliver a bubble into a live shell, the clock. The
     only ring allowed to import `reqwest`, `rusqlite`, `crossterm`,
     `ratatui`, `tokio` types and the OS.

4. **Composition root (`src/main.rs`, later `app.rs`, `daemon.rs`,
   `service.rs`).** One binary, two roles: the thin client the hooks and
   the user call, and `kintsu daemon`, the resident process. `app.rs` has
   one function per subcommand: build the gateways, call a use case, hand
   the result to a presenter. `daemon.rs` owns the runtime, the socket
   listener and the session supervisor. `service.rs` writes the launchd
   plist, the systemd unit and the URL-scheme handler. Nothing here decides
   anything a test would want to check.

## One failure through the rings

```
 shell hook                     kintsu (client)                 kintsu daemon
 ──────────                     ───────────────                 ─────────────
 precmd: status ≠ 0  ──argv──▶  controllers::cli
                                ──frame──▶ socket ──────────▶  controllers::socket
                                                               use_cases::triage_outcome
                                                                 ports: SessionRegistry (recent commands, terminal identity)
                                                                        RuleBook (instant fixes)
                                                                        CaseStore (dedupe, cooldown, ignore list)
                                                                        Clock
                                                               ◀── TriageDecision within the sync budget
                                presenters::toast ◀─decision──
 prints the toast    ◀─stdout──
 (prompt is back)                                              use_cases::classify_failure (async, if a tiny model exists)
                                                                 ports: OutputSource → CapturedOutput
                                                                        RuleBook / redact_case
                                                                        route_task → ModelGateway (tier Tiny)
                                                               use_cases::… → Notifier ──▶ session subscriber
 zle -F handler      ◀─bubble──  presenters::toast ◀───────────
 prints above prompt
```

The sync path, from the hook to the printed toast, has a budget of a few
milliseconds and never touches the network. Everything that needs a model
happens after the prompt is back and arrives as a message.

## Dependency Rule and CI

```
entities  ◀──  use_cases (+ ports)  ◀──  adapters (controllers · presenters · gateways)  ◀──  main.rs
```

| folder | may import (of ours) | may also use |
|---|---|---|
| `src/entities` | nothing | `thiserror`, `regex` |
| `src/use_cases` | `crate::entities` | `thiserror`, `core::future` |
| `src/adapters` | entities, use cases | anything: HTTP, SQLite, terminals, runtimes |
| `src/main.rs` and friends | everything | anything |

`tests/dependency_rule.rs` walks `src/entities` and `src/use_cases` and
fails on `crate::adapters`, `crate::use_cases` (from an entity), and on
anything that smells of I/O, a runtime, a terminal or a clock: `std::io`,
`std::fs`, `std::process`, `std::net`, `std::os`, `std::env`,
`std::thread`, `Instant`, `SystemTime`, `tokio`, `crossterm`, `ratatui`,
`reqwest`, `rusqlite`, `println!`… It also checks the ring folders exist.
`cargo test` runs it; CI runs `cargo test`.

## Invariants

- **The quiet path is sacred.** From the hook to the printed toast: under
  5 ms, no network, no model, no config re-parse beyond a cached mtime.
- **Never execute a suggestion.** A fix is inserted into the user's line
  editor; the user presses Enter. Rules may be marked `auto` per user
  choice, never by default.
- **Captured output is data.** It is fenced in prompts, never interpreted
  as instructions, and redacted before it leaves the machine.
- **Fail closed to silence.** No daemon, no model, no terminal API: Kintsu
  says less, never blocks a prompt, never alters the exit status the prompt
  sees.
- **Newtypes over primitives; illegal states unrepresentable.** A blank
  `CommandLine` cannot exist; a `Fix` carries its `Danger`; a `Task` names
  the tier that may serve it.
- **Validate at the edge.** Config becomes typed `Settings` in a gateway,
  argv becomes a `Command` in a controller; no inner ring ever interprets
  a raw string.

## Tests

- **Entities and use cases** are covered line by line with plain unit
  tests; use cases run against the in-memory fakes of their ports.
- **Ports** have one contract suite each, run against every gateway that
  implements them: the SQLite store and the in-memory store pass the same
  `CaseStore` tests, every `ModelGateway` passes the same streaming and
  error-shape tests against a scripted HTTP server.
- **Presenters** are tested as pure functions: given a view state and a
  width, this exact text; the panel against ratatui's `TestBackend`.
- **Controllers** are tested on their parsing and their refusal messages.
- **The binary** is tested end to end by driving the hooks in real shells:
  zsh and bash accept piped input in interactive mode; fish needs a
  pseudo-terminal that answers its capability queries, or `emit
  fish_postexec` for the handler alone.
- Test names state behaviour (`a_command_the_user_stopped_stays_quiet`).
  Deterministic always: fakes, temp directories, scripted servers. No real
  network, no real terminal, no sleeps.
