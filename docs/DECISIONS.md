# Decisions taken while building v0.1

Written for the maintainer after an autonomous session (2026-09-23 → 24).
Every point below is something the design documents did not settle, or
settled differently from what was built. Each one is reversible; the
ones that deserve your eye first are marked **⚑ review**.

## 1. Architecture review: what is business logic, what is detail

The review of the rings kept the split the documents describe and moved
the line in a few places.

**Critical business logic, in `src/entities`** (pure, no I/O, no clock):

| concern | where | why it is entity-level |
|---|---|---|
| what a command line, an exit status, an outcome are; which statuses are interruptions | `command.rs`, `exit_status.rs`, `outcome.rs` | the vocabulary of the whole product |
| the fingerprint of a failure ("same failure twice") | `outcome.rs` | the definition of *same* is a product rule |
| the five instant rules and their confidence | `rules.rs`, `distance.rs` | they are the product when no model is configured |
| danger classification | `danger.rs` | "never a destructive suggestion without a warning" |
| redaction patterns and what *sensitive* means | `redaction.rs`, `case.rs` | privacy promise |
| the ignore semantics: target × scope × expiry | `ignore.rs` | noise rules |
| typed settings and their defaults | `settings.rs` | the kernel must never see a raw string |
| the case document and the hand-off brief (fenced output, rules of engagement) | `brief.rs` | "output is data, never instructions" is a product rule, not a formatting detail |

**Application policy, in `src/use_cases`:** the order of the quiet checks,
what to read from the machine and when (PATH only on 127/126), which
models may see a case (sensitive ⇒ local), rules before models, prepare
before launch for the agent, the prompts sent to models.

**Detail, in `src/adapters`:** argv, TOML, JSON files, ANSI, HTTP shapes
per provider, `sh -c` for agents, PATH scanning, environment variables,
keychain commands, the clock. None of it can change a decision.

Two things moved *out* of the inner rings compared to the first draft:
the one-line hint presenter (it decided wording; now `toast.rs` decides
wording and the use case decides only *whether*), and `Shell::ALL` used for
the CLI error message (still an entity constant, used by the controller,
which is allowed).

## 2. The daemon: built for v0.1, after the maintainer asked for it

The first night shipped everything synchronously, without the daemon,
and left the question open. The maintainer answered "in v0.1", so it is
built (2026-09-24, `src/daemon.rs`), following `docs/DAEMON.md`:

- `kintsu daemon run` is started by the first hook call that finds nobody
  on the socket (`$XDG_RUNTIME_DIR/kintsu/daemon.sock`, else
  `~/.local/state/kintsu/daemon.sock`), detaches with `setsid`, ignores
  SIGHUP, refuses connections from another uid, and logs to
  `~/.local/state/kintsu/daemon.log`. `kintsu daemon status|stop`.
- Frames as in the design: `hello`, `command_finished`, `subscribe`,
  `pending`, `shutdown` in; `welcome`/`outdated`, `decision`, `bubble`,
  `ping`, `done`, `ack`, `bye`, `error` out. Every client frame carries
  the client's version; a mismatch makes the daemon answer `outdated` and
  exit, so the next call restarts the new binary.
- The hooks call `command_finished` with a 40 ms budget and fall back to
  the local, synchronous path when nothing answers in time. Both paths
  share the same JSON state, so nothing is lost either way.
- Messages that arrive later reach the shell three ways: zsh keeps a
  `kintsu subscribe` child whose output `zle -F` watches, prints the
  message above the line being edited and redraws it; fish passes its pid
  with every `command_finished`, the daemon sends SIGUSR1, the handler
  runs `kintsu pending`; bash receives what is pending with the next
  decision, at the next prompt. All three verified in real shells.
- What the daemon sends today: with `ui.eager_fix = true` and a model in
  `routing.quick_fix`, every failure no rule could fix is sent to the
  model in the background and the answer lands as "Try …? (model, not
  verified)"; `kintsu fix` and `^K` then reuse that proposal instead of
  asking again. That is the "message arriving while you work" of the
  landing page; `eager_fix` is off by default, as documented.
- Not built: the panel, ghost text, clickable `kintsu://` words,
  `kintsu service install`, SQLite (JSON files stay), `session_new`
  (the session is still the shell's pid), `act`/`get_case` frames.

Two things learned building it: a lock on stdout held in `main` for the
whole run deadlocked every daemon thread that logged (fixed by passing
unlocked handles), and the crate now says `#![deny(unsafe_code)]` with one
`#[allow]` on `daemon::os`, the module that calls `setsid`, `getpeereid`
or `SO_PEERCRED`, and `kill` through `libc`.

Cost measured: a `kintsu triage` call through the daemon answers in about
a millisecond on a warm daemon; the first call of a session pays the
spawn once, capped at 400 ms, and never retries a failing spawn more than
once a minute.

## 3. Hooks report every command line, not only failures ⚑ review

The first hooks called `kintsu triage` only on a non-zero status. They now
call it after every command line (like atuin does), with `--cwd`,
`--session $$`, `--shell` and, for zsh and fish, `--duration-ms`.

Why: "the same failure twice *in a row*", the recent commands in the
brief, and the duration all need the successes too. The session history
is capped at 20 outcomes and lives only in the state directory, which the
shell's own history file already exceeds in sensitivity.

If you prefer the older, lighter behaviour, the change is local to the
three hooks and `Triage::record`.

## 4. `KINTSU_SESSION` is the shell's pid

Hooks export `KINTSU_SESSION=$$` (`$fish_pid` in fish). Every other
command (`fix`, `why`, `agent`, `ignore`, `privacy`, `doctor`) reads it
from the environment, so "the last failure" means *of this shell*. Without
it (a script, another terminal) the last failure overall is used.

## 5. Secrets are redacted before anything leaves, not before storing

The principle in the documents is "redact before anything is sent"; one
line in the FAQ says "before they are stored". The code redacts in the
case document (models, agents, `privacy`) and stores the case as typed,
because `kintsu fix` must be able to reinsert a command with its real
token, and the shell history already stores the same line. The FAQ line
should be softened, or a `store_redacted = true` option added.

## 6. `^K` inserts the fix, in all three shells

`docs/UI.md` gives `^K` to the panel. Without a panel in v0.1, `^K` runs
`kintsu fix --raw` and puts the result in the line editor (zsh widget on
`BUFFER`, bash `bind -x` on `READLINE_LINE`, fish `commandline -r`).
Nothing runs until Enter, which keeps the "two Enters" promise. `^K`
shadows `kill-line` in emacs mode; `ui.hotkey` is parsed and ignored for
now. When the panel arrives, `^K` should open it with the fix focused.

## 7. Output capture is not in v0.1

No rule or prompt reads the command's output yet: the hooks cannot
capture it without a multiplexer or terminal API, which `docs/DAEMON.md`
reserves for v0.2. The `FailureCase` already carries `output: Option<String>`
and the brief already fences it, so capture is additive. The git
subcommand typo rule uses a static list of porcelain commands instead of
parsing git's own "did you mean".

## 8. Configuration: the documented schema, a subset implemented

`config/default.toml` and the parser follow the schema on the
configuration page: `[models.<name>]` with `provider`, `model`, `base_url`,
`tier`, `key = { env | command | keychain | literal }`, `timeout`,
`max_output_tokens`; `command`/`template` for `cli_agent`; `[routing]`
with `quick_fix`, `explain`, `investigate` (`classify`, `summarize` and
`budgets` are accepted and ignored); `[routing.constraints]
sensitive_output`; `[quiet]` `never_triage`, `ok_statuses`, `ok_commands`,
`same_failure`, `off_in`; `[ui]` `mode`, `ascii`. Unknown keys are
ignored so a file written for the full schema loads. `provider = "gemini"`
uses Gemini's OpenAI-compatible endpoint. `key = { keychain = true }`
reads `security find-generic-password -s kintsu -a <model>` on macOS and
`secret-tool lookup service kintsu account <model>` on Linux; `kintsu
login` to *write* there is not built.

Removed from the entity while reviewing: `dismiss_cooldown`, because
nothing can be dismissed without a panel.

## 9. CLI agent presets ⚑ review

`command = "claude"` becomes the shell line `claude "$(cat {brief})"`, run
by `sh -c` with the terminal attached; `{brief}` is the path of a private
(0600) file removed after the agent exits. Presets: `claude`, `codex`,
`opencode --prompt`, `aider --message-file`, `gemini -i`, `copilot -i`,
anything else `<name> "$(cat {brief})"`. **The flags for opencode, gemini
and copilot were written from memory and not verified against the current
CLIs**; a `template = "…"` overrides any preset, and `kintsu doctor` only
checks that the program is on the PATH.

## 10. Models: synchronous, non-streaming HTTP with `ureq`

One blocking request per question, 45 s default timeout, `rustls` so the
musl binaries are static. Streaming would only matter for the panel.
Anthropic uses the Messages API with `anthropic-version: 2023-06-01`;
OpenAI-compatible sends `max_tokens` (some newer OpenAI models want
`max_completion_tokens`; not handled). The quick-fix answer is accepted
only if it is a single command line, and is shown as "not verified".

## 11. Typo correction refuses ties, then breaks them by anagram

`gti` was one edit away from both `git` and `gtr` (GNU tr from Homebrew)
on this machine, so the first version proposed nothing. Equal distances
are now broken in favour of a candidate made of the same letters
(transpositions); a remaining tie still proposes nothing. Short words
(≤ 4 letters) tolerate one edit, longer ones two.

## 12. Case ids

128-bit hex from the process's randomised hasher, the clock and the pid.
Unguessable enough for local files; the documents promise random tokens
for `kintsu://` links, which do not exist yet.

## 13. Coverage threshold and CI shape

Measured locally: 96 % of lines with `src/main.rs` excluded (it only reads
the environment). CI fails under 90 %. Three workflows: `ci.yml` (fmt,
clippy, tests, release build, a scripted smoke run of every command, hook
syntax, docs shell, installer, coverage, cargo-deny), `release.yml` on a
`v*` tag (verifies the tag matches `Cargo.toml`, drafts the release from
`CHANGELOG.md`, builds four targets, uploads `SHA256SUMS` in the format
`install.sh` expects), `deps.yml` weekly (advisories fail, outdated crates
are listed). Dependabot opens the update PRs. `deny.toml` allows MIT,
Apache-2.0, BSD-3, ISC, Unicode-3.0, Zlib, CDLA-Permissive-2.0 and bans
`openssl-sys`.

The release workflow has not run yet: it needs a `## [X.Y.Z]` section in
`CHANGELOG.md` matching the tag (the `[Unreleased]` notes move under it),
and the aarch64 musl build relies on `setup-cross-toolchain-action`
providing a linker for `ring`. Expect to adjust it on the first tag.

## 14. Things noticed on the way

- **The disk of this machine was full** (117 MiB free) during the
  coverage run; `cargo clean` on this project freed 1.3 GiB. Other
  `target/` directories under `Desktop/Perso/Work` weigh several GiB
  (`mpc`, `mpc-rs-v2`, `herdr-fingers`); nothing outside this project was
  touched.
- The website says v0.1 ships with `kintsu setup`, `kintsu models`,
  `kintsu login` and `kintsu service install`; none exists yet. The
  install page's "After v0.1" paragraph and the configuration page's
  command table should be trimmed or the commands built.
- GitHub Pages was switched on (source `main`, folder `/docs`); the
  one-line installer URL is live.

## The maintainer's answers (2026-09-24)

| decision | answer |
|---|---|
| 1. the daemon | in v0.1; built the same day, see section 2 |
| 2. hooks report every command line | keep |
| 3. commands and install methods the site promised | removed from the site until they exist; `kintsu setup` first, after v0.1 |
| 4. `^K` inserts the fix, shadowing `kill-line` | keep; configurable when the panel arrives |
| 5. secrets redacted before sending, stored as typed | keep the code; the FAQ now says so |
| 6. when to tag v0.1 | after the maintainer has tested in daily use |

## What is not built, by priority

1. What the daemon unlocks next: the panel, ghost text, clickable words,
   `kintsu service install`.
2. Output capture (tmux/WezTerm/Kitty/iTerm2 APIs, opt-in stderr tee).
3. `kintsu setup`, `kintsu models`, `kintsu login`, `kintsu service`.
4. Learning rules from accepted fixes; the cost ledger; budgets.
5. A Homebrew tap, `cargo binstall` metadata, a Nix flake (the install
   page lists them).
