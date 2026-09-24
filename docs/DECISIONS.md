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
  `kintsu subscribe` child whose output `zle -F` watches; fish passes its
  pid with every `command_finished`, the daemon sends SIGUSR1, the handler
  runs `kintsu pending`; bash receives what is pending with the next
  decision, at the next prompt. In zsh and fish the handler climbs to the
  first line of the prompt (its height comes from rendering `$PROMPT` /
  `fish_prompt` again, plus the lines of the edited text), clears from
  there, prints the message and lets the shell redraw the prompt below it,
  typed text intact. Printing at the cursor and asking for a repaint, the
  obvious way, made fish overwrite the message and left a stale prompt in
  zsh. Verified in a screen emulator driving real zsh and fish sessions,
  with a three-line prompt and text being typed when the message lands.
- What the daemon sends today: with `ui.eager_fix = true` and a model in
  `routing.quick_fix`, every failure no rule could fix is sent to the
  model in the background and the answer lands as "Try …? (model, not
  verified)"; `kintsu fix` and `^K` then reuse that proposal instead of
  asking again. While the model is being asked, the bubble carries a third
  dim line, "asking haiku…"; the answer replaces it in place when no other
  prompt was drawn in between (the hooks count prompts, and the client's
  exit status 3 tells them a line is waiting), and a model that failed or
  had nothing says so in one line instead of leaving it dangling. An
  animated spinner was considered and rejected: every frame would redraw
  the prompt under the user's fingers. That is the "message arriving while
  you work" of the landing page; `eager_fix` is off by default, as
  documented.
- A key given as `{ env = … }` is read by the daemon, which inherits the
  environment of the shell that spawned it: set the variable before the
  first failure of the day, or `kintsu daemon stop` after changing it.
  `doctor` says when it is missing; the daemon log says which model
  refused and why. Keychain and command sources are read per call.
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
anything else `<name> "$(cat {brief})"`. Verified on 2026-09-25 against
the installed CLIs (`claude --help`, `codex --help`, `copilot --help`)
and the published references (opencode, Gemini CLI, aider): all six
presets match. One caveat: aider's `--message-file` processes the brief
and exits instead of staying in chat, because aider has no "start
interactive with a prompt" flag. A `template = "…"` overrides any preset;
`kintsu doctor` only checks that the program is on the PATH.

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
| 6. when to tag v0.1 | after the maintainer has tested in daily use; tagged `v0.1.0` on 2026-09-25 |

## Open questions for the maintainer (2026-09-24, evening), answered on 2026-09-25

Answers: 1. `"auto"`, on when the model is local, is the default. 2. The
landing keeps the vision under the pre-alpha label. 3. `why` is
asynchronous through the daemon (an "asking…" line, then the explanation
as a message; `kintsu why` leaves a marker file the hooks read so the
answer replaces the line, and exits 0 so prompts show no error); the
panel stays the big piece of v0.2. 4. `v0.1.0` is tagged; fixes bump the
patch number.

1. **`eager_fix` by default.** Off today, so nothing is sent to a model
   without an explicit command. On, every failure no rule can fix goes to
   the `quick_fix` model with the command, the directory and the last ten
   command lines (secrets redacted, cloud excluded for sensitive cases).
   Options: keep it off; on; on only when the first `quick_fix` model is
   local. Suggested: on only when local, which keeps the default private.
2. **The landing page's terminal shows v0.2**: `Tab to fix`, ghost text,
   clickable words, the panel. The installed product says `^K to insert ·
   kintsu why · kintsu agent · kintsu ignore` and an "asking…" line.
   Options: leave the vision with the pre-alpha label; align the "Play
   with it" mode with v0.1 and keep the guided tour as the vision.
   Suggested: align "Play with it", since the installer is one line away.
3. **`kintsu why` stays synchronous** (the prompt waits for the answer),
   while the eager fix arrives as a message. Consistent alternative: run
   `why` through the daemon and deliver it as a message too. Suggested:
   keep it synchronous until the panel exists; an explicit question
   deserves an immediate answer.
4. **Tagging v0.1.** Everything the roadmap listed for v0.1 exists. What
   is left is the maintainer's daily-use verdict, the agent presets for
   opencode, gemini and copilot (unverified), and the first run of the
   release workflow. Suggested: a few days of use, then `v0.1.0`.

## 15. A local model by default (2026-09-25)

Asked by the maintainer for v0.1: the installer offers to install Ollama
(Homebrew on macOS, the official script on Linux) and to pull
`qwen2.5-coder:7b`, about 4.7 GB, and writes the default configuration
when there is none. That configuration declares the model as `local`,
`tier = "large"`, and routes it for `quick_fix` and `explain`; with
`eager_fix = "auto"` the message after a failure works out of the box and
nothing leaves the machine. `--no-model` or `KINTSU_NO_MODEL=1` skips it.
Because the server may be stopped, the daemon probes the local endpoint
(a 50 ms TCP connect) before announcing "asking local…", and `doctor`
warns with the command that starts it.

## 16. Architecture review after the daemon (2026-09-25)

Reviewed: every ring against the Dependency Rule, the responsibilities of
the composition roots, the two contracts between the binary and the
hooks. Kept as is: the entities, the ports and their fakes, the
presenters, the TOML and JSON edges. Changed:

- The daemon's session registry, which is the daemon's `Notifier`, lived
  in `daemon.rs` next to the socket loop and called a presenter itself.
  It is now `gateways/sessions.rs`, rendering injected by the composition
  root, with its own tests over socket pairs. The `libc` calls moved to
  `gateways/unix.rs`, the one module allowed `unsafe`; the daemon file
  is one function per frame.
- Two mechanisms told the hooks "an asking… line is waiting": exit status
  3 from `kintsu triage`, and a marker file from `kintsu why`, both built
  ad hoc in `app.rs`. One remains: the marker, through
  `gateways/asking_marker.rs`, read by the hooks after any `kintsu` call.
  Exit statuses are ordinary again.
- The eager fix and the asynchronous explanation duplicated the policy
  "a model that had nothing, or failed, says so in one line" in two
  shapes. One use case, `Messages`, owns it; `Explain` is synchronous
  only.
- `FailureCase::redacted` masked one concatenated text that
  `case_document` cut back into parts by counting lines. Each part is
  now redacted on its own.
- The pseudo-terminal harness that verifies the hooks' drawing lived
  outside the repository. It is `scripts/shell-harness.py`, documented
  in CONTRIBUTING.
- Smaller: `--session` without a value names the flag; the subscription
  loop moved from `app.rs` into the client gateway; the `explain` frame
  lost an unused field.
- Found by the harness while re-checking: fish may run the SIGUSR1
  handler between a command and its next prompt (an instant model
  answers before the prompt is drawn). The handler now knows whether a
  prompt is on screen (`fish_preexec` / `fish_prompt` events) and, when
  none is, prints under what was just printed instead of climbing into
  it.

Known debt, accepted for now: the prompt-height arithmetic exists twice,
once per shell, because the shells differ; state is JSON files behind
the `SessionRegistry`, `CaseStore` and `IgnoreStore` ports, so the SQLite
the design mentions is one more gateway when it is needed; `app.rs` is
the largest file and will split once the panel arrives; the harness is
manual, not in CI.

## 17. v0.2, step A: the output is captured (2026-09-25)

The hook's `kintsu triage` sends the pane identity it finds in its
environment (`HERDR_PANE_ID`, `TMUX_PANE` and `TMUX`, `WEZTERM_PANE`,
`KITTY_WINDOW_ID`, `ITERM_SESSION_ID`, `TERM_PROGRAM`). After an offer,
and only then, the daemon reads the pane's recent text in the background
through the `OutputSource` port, cuts what follows the last echo of the
command (`output_after`, an entity), keeps at most `capture.max_lines`,
and saves it with the case; the follow-up model, `why`, `privacy` and the
brief see it from then on. Without a daemon the local path does the same
after printing the bubble. The sync budget is untouched: nothing is read
before the bubble.

Sources, tried in the configured order, each through its own CLI:
`herdr pane read <id> --source recent-unwrapped`, `tmux capture-pane -p -J`,
`wezterm cli get-text`, `kitten @ get-text`, and iTerm2 through
`osascript` (the visible screen only). Herdr and tmux were exercised on
this machine; the other three follow their documentation and are covered
by the same invocation tests. Ghostty has no way to read a pane, so a
Ghostty user gets capture only inside Herdr or tmux. The opt-in stderr
tee for shells without any source is not built.

## What is not built, by priority

1. What the daemon unlocks next: the panel, ghost text, clickable words,
   `kintsu service install`.
2. The stderr tee for terminals without a readable pane; `session_new`.
3. `kintsu setup`, `kintsu models`, `kintsu login`, `kintsu service`.
4. Learning rules from accepted fixes; the cost ledger; budgets.
5. A Homebrew tap, `cargo binstall` metadata, a Nix flake (the install
   page lists them).
