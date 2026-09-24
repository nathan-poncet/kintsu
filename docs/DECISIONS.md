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

## 2. No daemon in v0.1 ⚑ review

`docs/DAEMON.md` and the roadmap put the resident daemon in v0.1. It is
not built. `kintsu triage` runs synchronously after each command line and
keeps its state in small JSON files under `~/.local/state/kintsu`.

Why: everything a user can see in v0.1 (toast, `fix`, `why`, `agent`,
`ignore`, `privacy`, `doctor`) works without a resident process, and the
daemon is the single biggest chunk of risk (socket, supervision, delivery
into a live shell). Building the product first shows what the daemon must
carry; building the daemon first would have shipped nothing usable.

Consequence: no asynchronous messages ("while you were away"), no ghost
text, no panel, no clickable words in v0.1. The toast shows commands
instead of links. The roadmap page and `DAEMON.md` still describe the
daemon as v0.1; **decide whether to move it to v0.2 in the documents** or
to build it before tagging v0.1.

Cost measured: a `kintsu triage` call is one process start plus one JSON
read/write, well under the 5 ms budget on this machine; the PATH is
scanned only when the shell answered 127 or 126.

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

## What is not built, by priority

1. The daemon and everything it unlocks (async messages, panel, ghost
   text, clickable words).
2. Output capture (tmux/WezTerm/Kitty/iTerm2 APIs, opt-in stderr tee).
3. `kintsu setup`, `kintsu models`, `kintsu login`, `kintsu service`.
4. Learning rules from accepted fixes; the cost ledger; budgets.
5. A Homebrew tap, `cargo binstall` metadata, a Nix flake (the install
   page lists them).
