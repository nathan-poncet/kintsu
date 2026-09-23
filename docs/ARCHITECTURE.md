# Architecture

Kintsu follows Clean Architecture the way the author's other projects do:
entities and use cases alone at the centre, an interface-adapters ring
around them split into controllers, gateways and presenters, and the
processes at the very edge as composition roots. In Rust the rings are
crates, so the compiler enforces the Dependency Rule between them, and
`cargo xtask check` verifies the crate graph and the purity of the inner
rings on every push.

*Status: the crates exist and hold the day-zero skeleton. Most modules
below are the target shape, named now so that the first real pieces land
in the right place. The process model is in [DAEMON.md](DAEMON.md), model
routing in [MODELS.md](MODELS.md), the bubble in [UI.md](UI.md).*

## The map

```
kintsu/
├── Cargo.toml                    workspace: apps/*, crates/*, tools/*
├── shell/                        kintsu.zsh · kintsu.bash · kintsu.fish   (the hooks, plain assets)
│
├── crates/
│   ├── entities/                 kintsu-entities — enterprise rules, no I/O, depends on nothing of ours
│   │   └── src/
│   │       ├── command.rs            CommandLine
│   │       ├── exit_status.rs        ExitStatus
│   │       ├── outcome.rs            CommandOutcome
│   │       ├── triage.rs             TriageDecision · QuietReason
│   │       ├── shell.rs              Shell
│   │       └── (planned)             Session · FailureCase · Context (cwd, git, history) · CapturedOutput ·
│   │                                 Redaction · Rule · Fix · Danger · Task · ModelTier · RoutingPolicy ·
│   │                                 Bubble · Action · Budget
│   │
│   ├── use_cases/                kintsu-use-cases — application rules, depends on the entities only
│   │   └── src/
│   │       ├── triage_outcome.rs     the one interactor that exists today
│   │       ├── (planned)             register_session · record_command_start · triage_outcome · classify_failure ·
│   │       │                         propose_fix · explain_failure · hand_off_to_agent · ignore_failure ·
│   │       │                         recall_case · route_task · redact_case · install_shell_hook · diagnose
│   │       └── ports/                one trait per file, role nouns, each owning its error type
│   │           └── (planned)         SessionRegistry · CaseStore · Clock · Notifier · OutputSource ·
│   │                                 ModelGateway · AgentLauncher · SecretStore · ConfigSource ·
│   │                                 RuleBook · CostLedger · Randomness
│   │
│   └── adapters/                 kintsu-adapters — depends on the entities and the use cases
│       └── src/
│           ├── controllers/          cli.rs (argv → Command)
│           │   └── (planned)         socket.rs (daemon frames → use cases) · url_scheme.rs (kintsu:// → use cases)
│           ├── presenters/           hint.rs · shell_hook.rs
│           │   └── (planned)         toast.rs · panel/ (ratatui, inline viewport) · ghost_text.rs ·
│           │                         handoff_brief.rs · plain.rs · json.rs
│           └── gateways/
│               └── (planned)         models/{openai_compatible,anthropic,gemini,ollama,cli_agent}.rs ·
│                                     agents/{claude_code,codex,opencode,aider,gemini_cli,copilot,template}.rs ·
│                                     output_sources/{tmux,herdr,wezterm,kitty,iterm2,stderr_tee}.rs ·
│                                     sessions/in_memory.rs · store/sqlite.rs · config/toml.rs ·
│                                     secrets/{keychain,secret_service,env,command}.rs ·
│                                     notify/{zle_fd,signal,next_prompt}.rs · clock.rs · rules/builtin.rs
│
├── apps/
│   └── kintsu/                   the binary: the client subcommands and `kintsu daemon`
│       └── src/
│           ├── main.rs               composition root, today
│           └── (planned)             app.rs (one function per subcommand) · daemon.rs (tokio runtime, socket
│                                     listener, session supervisor) · service.rs (launchd, systemd, URL scheme)
│
└── tools/
    └── xtask/                    `cargo xtask check`: Dependency Rule + purity of the inner rings
```

## The rings

1. **Entities (`kintsu-entities`).** What a command line is, what it
   returned, what a failure case contains, what a fix is, how dangerous it
   is, what a task for a model is and which tier may run it. Newtypes over
   primitives, enums over strings and bools, invariants enforced at
   construction. Allowed dependencies: `thiserror`, later `regex` for the
   rules and the redaction patterns. Nothing that does I/O, nothing that
   knows the time.

2. **Use cases (`kintsu-use-cases`).** One interactor per thing a user or
   a hook can ask for. They orchestrate entities against ports and hold the
   application policy: when to stay quiet, what context to collect, which
   task to route where, what must never run without confirmation.
   - **Ports** live in `use_cases/ports/`: role-noun traits, one per file,
     each owning its error type (`SessionRegistry`, `CaseStore`, `Clock`,
     `Notifier`, `OutputSource`, `ModelGateway`, `AgentLauncher`,
     `SecretStore`, `ConfigSource`, `RuleBook`, `CostLedger`).
   - Async-agnostic: a port that waits on the world returns `impl Future`.
     No executor, no `tokio`, no channel type from a runtime crate. The
     daemon decides how futures are driven.
   - Every port ships an in-memory fake next to its contract test, so a
     use case is tested in microseconds and every gateway is tested against
     the same suite.

3. **Interface adapters (`kintsu-adapters`).** You expected gateways for
   sure and maybe not the other two. The plan keeps all three, thin:
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

4. **Apps (`apps/kintsu`).** One binary, two roles: the thin client the
   hooks and the user call, and `kintsu daemon`, the resident process.
   `app.rs` has one function per subcommand: build the gateways, call a use
   case, hand the result to a presenter. `daemon.rs` owns the runtime, the
   socket listener and the session supervisor. `service.rs` writes the
   launchd plist, the systemd unit and the URL-scheme handler. Nothing here
   decides anything a test would want to check.

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
kintsu-entities   ◀── kintsu-use-cases ◀── kintsu-adapters ◀── kintsu (app)   [sink]
                                                                xtask         [sink, knows none of ours]
```

| crate | may depend on (of ours) | may also use |
|---|---|---|
| `kintsu-entities` | nothing | `thiserror`, `regex` |
| `kintsu-use-cases` | `kintsu-entities` | `thiserror`, `core::future` |
| `kintsu-adapters` | entities, use cases | anything: HTTP, SQLite, terminals, runtimes |
| `kintsu` | all three | anything |
| `xtask` | nothing | `cargo_metadata` |

`tools/xtask` reads `cargo metadata`, checks the graph against that table
and greps the sources of the two inner crates for anything that smells of
I/O, a runtime, a terminal or a clock (`std::io`, `std::fs`,
`std::process`, `std::net`, `std::env`, `std::thread`, `Instant`,
`SystemTime`, `tokio`, `crossterm`, `ratatui`, `reqwest`, `rusqlite`,
`println!`…). `cargo test` runs the same checks as tests; CI runs both.

Adding a crate is a deliberate act: it must be given a ring in `xtask`'s
table or the check fails. When a bounded context grows big enough to
deserve its own crates (the rules engine, the MCP server), it gets the same
three-ring split and joins the table.

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
- **The app** is tested end to end by driving the hooks in real shells:
  zsh and bash accept piped input in interactive mode; fish needs a
  pseudo-terminal that answers its capability queries, or `emit
  fish_postexec` for the handler alone.
- Test names state behaviour (`a_command_the_user_stopped_stays_quiet`).
  Deterministic always: fakes, temp directories, scripted servers. No real
  network, no real terminal, no sleeps.
