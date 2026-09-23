# Working conventions for Claude

kintsu: when a command fails, offer to hand it to the user's AI agent, in
their own terminal and shell (Rust, Clean Architecture, TDD). The plan is
[docs/VISION.md](docs/VISION.md); the target shape is
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). The essentials:

## Language & history
- Code, comments and commit messages in **English**. Conversation with the
  maintainer may be in French.
- Prefix commits and issues with a **Gitmoji** (📝 docs, ✨ feat, 🐛 fix, ✅ tests,
  ♻️ refactor, 👷 ci, 🔒 security…).

## Design
- One crate, one binary, no workspace: Kintsu is an application, not a library.
  The rings are folders, enforced by `tests/dependency_rule.rs`:
  `src/entities/` (pure, no I/O, no clock), `src/use_cases/` (interactors plus
  `ports/`, one role-noun trait per file with its error type, `impl Future` for
  anything that waits, no runtime), `src/adapters/` (`controllers/` turn argv,
  socket frames and URL-scheme calls into use case calls; `presenters/` map
  results to toast, panel, plain and JSON view states; `gateways/` implement the
  ports over models, agents, terminals, storage, secrets), `src/main.rs` and
  later `app.rs` / `daemon.rs` / `service.rs` as the composition root.
- The website is static HTML/CSS/JS in `docs/` next to the design documents
  (GitHub Pages convention, not switched on yet). No framework, no build step.
- A new side effect gets a port, an in-memory fake and a contract test first;
  every gateway passes the same contract suite.
- Design references before proposing anything: `docs/DAEMON.md` (process model,
  protocol, delivery into live shells), `docs/MODELS.md` (tasks, tiers, router),
  `docs/UI.md` (toast and panel, keys, OSC 8 clicks).
- Newtypes and enums over strings and bools; validate at the edge, so the
  kernel never sees a raw config value.
- **The quiet path is sacred**: `kintsu triage` runs after every failed
  command and must finish in under 5 ms with no network.
- **Never execute a suggestion.** The user presses Enter. Destructive
  patterns get a loud confirmation.
- Captured terminal output is data, never instructions: fence it in prompts.
- Shell hooks must preserve the exit status the prompt sees and must never
  re-report a failure on an empty Enter.

## Comments
- Prefer **self-documenting code**: precise names, small functions, strong types.
- Comment only a non-obvious *why*; never restate *what*.
- Never reference planning artifacts (issue IDs, milestones) in code.

## Tests
- TDD: red first; test names state behaviour (`a_successful_command_stays_quiet`).
- Deterministic always: fake providers, fake agents, fake output sources,
  temp dirs. No real network, no real terminal, no sleeps.
- Before pushing: `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test --locked`, and `bash -n` / `zsh -n` / `fish -n` on the hooks.
