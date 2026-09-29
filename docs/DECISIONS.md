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
  `kintsu service install`, SQLite (built on 2026-09-29, section 34, the
  JSON files imported once), `session_new` (built on 2026-09-29, section
  28; the session is still the shell's pid), `act`/`get_case` frames.

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
shadows `kill-line` in emacs mode; `ui.hotkey` was parsed and ignored until
section 28 read it. When the panel arrives, `^K` should open it with the fix
focused.

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
`same_failure`, `off_in`; `[ui]` `mode`, `ascii`; `[daemon]` `sync_budget`
(ignored until 2026-09-29, read since, section 35). Unknown keys are
ignored so a file written for the full schema loads. `provider = "gemini"`
uses Gemini's OpenAI-compatible endpoint. `key = { keychain = true }`
reads `security find-generic-password -s kintsu -a <model>` on macOS and
`secret-tool lookup service kintsu account <model>` on Linux; `kintsu
login` to *write* there came on 2026-09-29 (section 33).

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
syntax, docs shell, installer, coverage, cargo-deny, and since 2026-09-29
the pseudo-terminal harness in check mode on real zsh, fish and bash,
section 38), `release.yml` on a
`v*` tag (verifies the tag matches `Cargo.toml`, drafts the release from
`CHANGELOG.md`, builds four targets and a Debian package for each Linux
one, uploads `SHA256SUMS` in the format `install.sh` expects, then
publishes the apt repository to `docs/apt` on `main`, section 27),
`deps.yml` weekly (advisories fail). Dependabot
opens the update PRs. `deny.toml` allows MIT, Apache-2.0, BSD-3, ISC,
Unicode-3.0, Zlib, CDLA-Permissive-2.0 and bans `openssl-sys`.

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
the design mentions is one more gateway when it is needed (section 34
made it the store); `app.rs` is the largest file and will split once the
panel arrives; the harness is manual, not in CI.

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
Ghostty user gets capture only inside Herdr or tmux, or through the
opt-in stderr tee of section 30, in zsh and bash.

Two corrections after the first day of use. The echo of the command is
the last line that *ends* with it, never a line kintsu wrote itself (the
seam marks those): the bubble "git status exited 128." under a failure
used to be taken for the prompt's echo, which left the model with kintsu's
own words as the output and the real error gone. The bubble is stripped
from the end of the output for the same reason. And a result that arrives
late, the read of the pane or a model's proposal, is saved only while its
case is still the session's last (`CaseStore::still_current`), so a slow read
never brings back a failure the shell has moved past; the message is still
delivered. The prompts name the sections: the failure to explain is
the one under "Command", its "Output" is what it printed, and "Earlier
commands in this shell" are context that was already dealt with; a small
model otherwise re-explains the typo from two commands ago. A quick-fix
answer equal to the failed command line is read as no fix.

## 18. v0.2, step B1: ghost text (2026-09-25)

A fix that is high-confidence and harmless (`Fix::is_ghostable`) is
pre-typed on the next prompt. The decision frame carries it as `ghost`;
the client writes `<state>/sessions/<id>.ghost` (the `HookNotes`
gateway, which also owns the "asking…" marker). zsh shows it dim after
the cursor with `POSTDISPLAY`, the way zsh-autosuggestions does; Tab or →
on an empty line accepts it, typing anything discards it, Enter runs it.
fish has no way to draw a suggestion the shell did not compute, so Tab on
an empty line inserts the fix instead, and the bubble says "Tab to fix"
in both shells. bash keeps `^K`. The previous Tab and → bindings are
remembered and called when there is nothing to accept, so completion
plugins keep working. The file is removed at the next command, so a fix
never applies to a later failure.

## 19. v0.2, step B2: `kintsu setup` (2026-09-25)

Three questions, shaped by what the machine has: a local model when
Ollama is installed (default `qwen2.5-coder:7b`), a cloud model
(Anthropic, OpenAI or Gemini, key in a variable or the keychain), and
the agent for `kintsu agent` among the CLIs found on the PATH. `--yes`
takes every default without asking. The answers become `Settings` in a
pure function (`use_cases/setup.rs`), rendered to TOML by the gateway
(`render_settings`, which parses back to the same settings) and written
to the configuration path; an existing file is kept unless the user says
otherwise; doctor's report follows. The questions live in a controller
that reads any `BufRead` and writes any `Write`, so they are tested with
in-memory answers.

## 20. v0.2, step C1: clickable words, `kintsu open`, `kintsu service` (2026-09-25)

Every action word of the bubble (`kintsu why`, `fix`, `agent`, `ignore`)
is an OSC 8 hyperlink to `kintsu://act?case=<id>&do=<action>` when the
output is a terminal and `[ui] links = true` (the default). The
`Action` entity names the five words; `controllers/url_scheme.rs` parses
and builds the URL; `Style::link` draws it, and never into a pipe or
under `NO_COLOR`. The desktop hands the URL to `kintsu open <url>`, which
sends an `act` frame; the daemon remembers which session each offered
case came from (`Sessions::remember_case`, the last thousand) and answers
**in that shell**, as a message: `why` explains, `fix` sends the stored
or freshly computed fix as a "Try …?" bubble (`Messages::fix_now`),
`ignore` silences that command line, `agent` and `privacy` answer with a
note, because a click can neither start an agent nor insert a command
(DAEMON.md, Security). An unknown or expired case gets "this case is
gone". Without the panel yet, this is what a click does in v0.2; the
panel (step C2) will open on the action instead.

`kintsu service install` writes the pieces and runs the registrations:
on macOS a launchd agent (`dev.kintsu.daemon`, KeepAlive) and a tiny
AppleScript app compiled by `osacompile`, declared handler of the scheme
through `plutil` and `lsregister`; on Linux a systemd user unit and a
`.desktop` entry bound with `xdg-mime`. The file contents are pure
functions of the binary's path, and the commands go through a runner,
so the tests see what would run without running it. `uninstall` undoes
it. The daemon still starts on demand without the service: the service
only keeps it alive across logins and lets the desktop reach it.

## 21. v0.2, step C2: the panel (2026-09-25)

`^K` now expands the last bubble into the panel, as UI.md always said
and as section 6 promised. The panel is `kintsu panel`, run by a widget
the hooks bind: the binary draws on `/dev/tty` and writes to stdout only
the text the user took, which the shell puts in its line editor. Two
Enters, on purpose. It is a pure view model (`presenters/panel.rs`: keys
in, effects out, rendered by ratatui and tested against `TestBackend`),
a controller that maps crossterm events to those keys, and a gateway
(`gateways/tty_panel.rs`) that owns raw mode, the mouse, the viewport
and the clipboard (OSC 52). Sections: Why (asks the explain model on
entry, once), Fix (a known rule or stored fix, else asks the quick-fix
model on entry), Agent (the configured agents; ⏎ inserts
`kintsu agent --with <name>`, the user starts it), Ignore (this command
line, the program here / in this shell / everywhere, everything for an
hour; ⏎ applies), Privacy (the redacted document). Models are asked from
a thread; answers arrive through a channel while the panel keeps
drawing. The bubble's actions line ends with `^K more`, always.

Two things differ from the design. The viewport is pinned by asking the
terminal for the cursor row on the tty (ratatui's inline viewport asks
through stdout, which the shell is capturing), and the screen is scrolled
first when the rows would not fit. While the panel runs, stdin and stdout
point at the terminal's own device (`/dev/ttys003`, found through stderr,
never the `/dev/tty` alias, which macOS cannot watch for input): zsh gives
a widget's command substitution no stdin, crossterm reads keys from stdin
and asks its questions through stdout, and both are put back before the
text to insert is written. The pty harness (`scripts/shell-harness.py
panel`) drives this in fish, zsh and bash. The height is fixed when the panel
opens (six to fourteen rows: room for what is shown, or for the answer
being asked for); long content scrolls rather than growing the panel.
The hooks then climb back to the prompt's first line and clear, so the
prompt is redrawn where it was; bash gets the same treatment through
`${PS1@P}`. A click on a word still answers in the shell (section 20);
opening the panel on the clicked action is left for the panel's next
iteration, together with `ui.hotkey`.

## Open questions after v0.2's first day (2026-09-25, evening)

Asked once steps A to C2 had landed; the maintainer answered the same
evening, six of them after a fuller explanation. One is still *pending*.

| decision | answer |
|---|---|
| 1. when to tag `v0.2.0` | after the fixes the first days of use bring; tagged on 2026-09-25, after one evening of use and its fixes (section 23) |
| 2. a click answers in the shell | keep; the design's "a click opens the panel" is dropped |
| 3. `ui.hotkey` | read it; `^K` stays the default |
| 4. the panel's height | grows as answers arrive |
| 5. the site | shows the current tag; "Play with it" stays as it is |
| 6. the stderr tee for terminals without a readable pane | the maintainer asked why capture depends on the terminal when the shell is hooked; the answer is that the shell never sees the output, only the terminal holds it. Decided on 2026-09-29: build it, opt-in; section 30 |
| 7. project awareness: `Cargo.toml`, `.kintsu.toml`, `CLAUDE.md` in the brief | v0.3 |
| 8. `$pipestatus` | build it; built, section 23 |
| 9. email and IP redaction, custom patterns | v0.3 |
| 10. `session_new` | build the frame |
| 11. `kintsu models`, `kintsu login` | v0.3 |
| 12. case ids from real randomness | yes; built, section 23 |
| 13. the daemon's idle exit | not wanted: the daemon lives until logout or `kintsu daemon stop` |
| 14. streaming | build it |
| 15. `max_completion_tokens` | now; built, section 22 |
| 16. storage | SQLite |
| 17. `app.rs` | split now; done, section 22 |
| 18. the pty harness | in CI |
| 19. the prompt-height arithmetic, once per shell | accepted; the wrapped-prompt miscount is issue #1 |
| 20. distribution | Homebrew and apt |
| 21. aider's `--message-file` | `--read {brief}` instead; built, section 23 |

## 22. The panel remembers, and opens only on the failure just before (2026-09-25)

Two things the first day of use showed. `^K` after a command that
succeeded opened the panel on an older failure: `kintsu panel` now goes
through `Expand` (`use_cases/expand.rs`), which returns the shell's last
failure only while it is also the shell's last recorded command. The
hooks never report kintsu's own commands, so `kintsu why` does not end a
case, and a repeated identical failure, which triage deduplicates, still
counts as the same one. Otherwise `kintsu panel` exits quietly and the
hook redraws the prompt. And every `^K` started from nothing: the Why
section asked the model again. An explanation is now an entity kept with
the case (`FailureCase::explanation`), saved by `Explain::run` whenever a
model answers and the case is still current, whichever path asked: the
panel, `kintsu why`, or the daemon's message. A quick-fix model's answer
asked through `FixLast::run` is saved as the proposal the same way, as
the daemon's eager fix already was. The panel opens on what the case
holds and asks nobody. The harness scenario `panel` drives both: `^K`
again shows the answer at once, `^K` after `true` opens nothing.

Also this evening, from the answers above: OpenAI's own endpoint is sent
`max_completion_tokens`, the compatible servers still `max_tokens`, and a
refusal that names the other field is retried once with it. `app.rs`
became `src/app/`: `mod.rs` keeps `Runtime`, the dispatch and the shared
helpers; `hooks.rs` (`triage`, `subscribe`), `panel.rs`, `desktop.rs`
(`open`, `service`) and `setup.rs` each compose one family of commands;
the tests are `tests.rs`.

## 23. The panel in the bubble's place, quiet late notes, the pipeline's real failure (2026-09-25, evening)

From the maintainer's first evening with the panel.

- **The panel opens where the bubble is.** `^K` drew the panel under the
  prompt and left the bubble above it: the same failure twice, one copy
  inert. Every printer of the bubble now leaves what it printed, as
  printed, in `<state>/sessions/<id>.bubble` (`HookNotes`): the toast in
  `triage`, the messages `subscribe` and `pending` deliver, `kintsu why`
  and `kintsu fix`. zsh and fish remove it when a command starts, bash,
  which has no preexec, after any command but `kintsu why` and `kintsu
  fix`, and all three on an empty Enter. The
  hook passes `kintsu panel --above <rows>` its own count to the prompt's
  first line, plus one while an "asking…" line waits; kintsu adds the
  bubble's rows (`screen_rows`: wrapping counted, escape sequences not),
  climbs, clears, draws the panel where the bubble was, and on close
  prints the bubble back and leaves the cursor where the prompt's first
  line goes. With nothing to expand only the prompt is cleared for the
  hook to redraw. The hooks no longer climb themselves. Verified by the
  harness in fish, zsh and bash.
- **A late answer names its command.** Two quick failures: the first's
  "local had no fix" arrived under the second's bubble, next to the
  second's own, indistinguishable. Once the model has answered,
  `Messages::fix` asks whether the shell still looks at the case
  (`Focus::holds`: no newer failure, no other command since). If not, the
  message is still shown, as one line that names its command, "git
  status: local had no fix for this one.", with no keys, since `^K` and
  the words belong to the current failure; the proposal is kept for
  `kintsu fix` while no newer failure replaced it. Dropping the message
  was tried first and read as an answer that never came. What waited for
  a bash prompt is marked late the same way when the next decision drains
  it (`Focus::mark_late`), and `kintsu why` in bash answers in place,
  since bash hears no message before its next prompt anyway. `Expand`
  became `Focus`, the one place that says what the shell is still looking
  at. The "asking…" line itself cannot be rewritten once another command
  has printed under it, its row being unknown by then without a terminal
  API; so when the next command starts, while the line is still one known
  distance above the cursor, the zsh and fish hooks blank it, and the
  answer arrives below, named. And the messages `kintsu pending` and
  `kintsu subscribe` bring are coloured again: their stdout is the hook's
  pipe, so their colour follows the terminal's existence and `NO_COLOR`,
  not their own streams.
- **`$pipestatus`.** The hooks send every stage's status.
  `CommandOutcome::in_pipeline` makes the first stage that failed the
  failure, a reader closing early (141) and interruptions aside, and
  `CommandLine::stages` finds that stage's program. So `gti status | head`
  is a typo although `head` exited 0, `foo | grep x` under `pipefail` is
  accepted when grep found nothing, and the toast says "`grep` exited 2 in
  cat log | grep x". Stored with the outcome, sent in the frame.
- **Case ids** come from `/dev/urandom`, the hasher only if that is
  unreadable: DAEMON.md's promise holds.
- **aider** is launched with `--read {brief}`: the brief is in the chat as a
  read-only file and the session stays open; `--message-file` processed it
  and exited. The user types what they want done.
- **No idle exit**, decided: the daemon lives until logout or
  `kintsu daemon stop`; the service keeps it alive anyway.
- **The wrapped-prompt miscount** (decision 19) is accepted and tracked as
  issue #1.

## 24. The rules look at the shell's PATH, not the daemon's (2026-09-28)

Found in use: `clade` was corrected to `clang`, not `claude`. Since
`kintsu service install` the daemon runs under launchd, whose environment
carries the bare system PATH (`/usr/bin:/bin:/usr/sbin:/sbin`), and the
typo rule looked for neighbours there: `~/.local/bin`, Homebrew, cargo and
mise were invisible. The `command_finished` frame now carries the shell's
`path`; the daemon builds the rules' environment from it for that request
and remembers it per session, so a click on `fix` later looks at the same
programs. The daemon's own PATH is only the fallback for a frame without
one. The same environment gap holds for API keys read from `{ env = … }`
under launchd; that one is still open.

## 25. More instant rules, after thefuck (2026-09-28)

The maintainer asked to take from thefuck's rules what spares a model
call. Most of thefuck's 170 rules read the command's output; the instant
rules run on the quiet path, before anything is read from the terminal,
so the first batch is what the line, the status, the PATH and the
working directory tell (section 24 made the PATH the shell's). The rules
that read the shape of the line come before the typo guess, so
`git-log` becomes `git log` and not `git-lfs`; the typo guess stays
before `./` (a `gti` here is still `git`). `rm dir` proposes `rm -r dir`
and carries the red line every recursive removal gets, so it is never
pre-typed. `git commit -amend` is a guess about intent, confidence 0.75,
not pre-typed either. Uneven quotes were on the list and are not built:
a line with an unbalanced quote never runs, the shell waits for more.
The subcommand tables moved to `entities/subcommands.rs`; they serve
typos, `gitpush` and `git-push` alike. Rules are re-implemented from the
idea, in Rust, under our own tests; thefuck is MIT.

## 26. Rules that read the output, before the model (2026-09-28)

The second batch from thefuck: the rules that need the command's output.
They run where the model call was, in the daemon's background thread,
once the capture kept the output with the case, and in `kintsu fix`, the
panel and the agent's brief through the same `rule_fix`. A rule's fix
takes the same road as a model's (saved as the proposal when the case is
still the shell's last, delivered as a bubble, late and named when the
shell moved on), and needs no model at all: with none configured the
bubble still comes. Late rule fixes are not pre-typed, like every
message. The output is data: a word taken from it goes back into a
command only when it is a plain path, branch, host or package name, and
a tool's "did you mean" is taken only next to its own "unknown command"
words, so a compiler's hint about a variable is left alone. Ties among
several suggestions are refused, as in the typo rules. The local path
(`KINTSU_NO_DAEMON`) captures but does not deliver: `kintsu fix` finds
the rule's answer there. Without a readable pane (Ghostty) nothing of
this fires unless the stderr tee of section 30 is on, in zsh or bash.

## 27. The daemon reads the keys the shell sees (2026-09-29)

The other half of section 24: under launchd the daemon's environment has
no `ANTHROPIC_API_KEY` either, so every model whose key is `{ env = … }`
was unreachable from the daemon, which is where the eager fix, `kintsu
why` and the panel's Why and Fix run. The hook's process reads the
variables the configured models name, and only those, and sends their
values in the `command_finished` frame as `env`; the daemon remembers
them per session next to the PATH and asks models through a `Secrets`
gateway that looks there first and in its own environment after. Nothing
is written or logged; the socket is the user's, `0600`. Considered and
not done: writing the values into the launchd plist at `kintsu service
install` (a secret in a `644` file, stale after a rotation) and asking
the user to switch to `{ command = … }` or the keychain (right for them,
not a reason to break `{ env = … }`). `kintsu doctor` says, when the
daemon is installed as a service and a model reads its key from the
environment, that the shell forwards it.

## 28. `session_new`: the shell registers itself when it starts (2026-09-29)

Open question 10 of 2026-09-25, answered "build the frame". The hooks run
`kintsu session new --shell <name> --pid <pid>` once at init; the client
adds the tty of its stdin, the pane identity, the shell's PATH and the
key variables the models read, and sends `session_new`. The daemon
registers the session, logs one line about it and answers `session` with
the id. The id stays the shell's pid (section 4): this frame is where a
daemon-chosen token would come back from, and nothing else changes when
that day comes.

Nothing at the shell's start waits: when no daemon listens the client
starts one and returns without a registration, the first
`command_finished` carries everything anyway; when one listens the answer
must come within the sync budget. So the very first shell after a boot is
registered only by its first frame. The hooks keep sending PATH, keys and
pane identity with every `command_finished`: a key exported after the
shell started must reach the daemon, and a daemon restarted under a
running shell must not be blind until the next shell. What the daemon
gains is the fallback order, frame first, then the registration, then its
own environment, and a registry that knows every live shell.

Forgetting: every twenty seconds, with the subscriber ping, sessions with
no subscriber whose pid (from the registration, the SIGUSR1 pid, or the
id itself when it parses) is gone are dropped, with their pending
messages, which nobody would read. A session that gave no pid is kept;
tests use such ids. Reads of the registry (`path_of`, `env_of`,
`terminal_of`) create nothing, so a forgotten session does not come back
as an empty entry when a late click asks about it.

## 29. `ui.hotkey` is read (2026-09-29)

Open question 3 of 2026-09-25, answered "read it; `^K` stays the default".
A `Hotkey` newtype is built at the edge from `[ui] hotkey`, in the notations
people write (`^O`, `ctrl-o`, `C-o`): one control letter, and not the ones
the terminal driver or the line editor own (Tab, Enter, Backspace, `^C`,
`^D`, `^Z`, `^S`, `^Q`), refused by name. `kintsu init` puts the key in
each shell's bind syntax (`bindkey '^O'`, `bind \co`, `bind -x '"\C-o"'`),
fish's vi-mode binding included. A broken configuration still gets a hook,
with `^K`: a shell without its hook is worse than a shell with the default
key, and `kintsu doctor` names the mistake. Every text that names the key
(`^K more`, the hint line, the last line of `kintsu fix`) reads it from the
style; the daemon refreshes it with the settings for the subscriber
renderer, next to `ascii` and `links`. The configuration page wrote
`hotkey = "ctrl-k"`; that spelling is accepted, `^K` is the one printed.

## 30. The stderr tee: opt-in, zsh and bash (2026-09-29)

Open question 6, decided: built. `[capture] stderr_tee = true`, the name
the configuration page already had, makes the zsh and bash hooks send
each command's stderr through `tee` into `<state>/sessions/<id>.stderr`,
one command per file. `kintsu init` emits that code only when the option
is on, so switching it needs the hook re-sourced; `kintsu doctor` says
what the shell does with it. The client names the file in the frame's
terminal identity (`stderr_copy`) when it exists; the daemon reads it
first among the sources, once, and removes it. A copy has no prompt echo,
so `output_after` keeps its tail as it is.

zsh saves fd 2 and redirects it in `preexec`, restores it in `precmd`
before `kintsu triage` prints, so the bubble never lands in the copy; a
`kintsu` command line is not copied. bash has no preexec: a `DEBUG` trap
starts the tee before the first simple command of a line, armed at the
end of the `PROMPT_COMMAND` chain so the prompt's own functions are not
copied and disarmed once started; an existing `DEBUG` trap (bash-preexec)
is left alone and the copy is not made. fish cannot redirect its own
stderr; the doctor says so when the option is on there.

The costs, and why it is off by default: a `tee` process per command,
and the command sees a pipe on stderr, not a tty, so cargo, git and pip
colour and animate less. The tee is not waited for: zsh gives no pid for
a process substitution, and a background job that inherited the pipe
would hold `wait` until it exits. kintsu reads the file after the bubble,
from the daemon's background thread, well after tee's last write; a job
left in the background keeps its stderr flowing into the file until it
exits, and the next command's tee truncates it.

## 31. The panel grows as the explanation streams in (2026-09-29)

Open questions 4 and 14 of 2026-09-25, answered "grows" and "build it".
The model gateway port has a streamed variant next to the whole answer:
`stream` hands the text over piece by piece and returns it whole, and a
gateway that cannot stream hands it over in one piece, so the fakes and
the callers that want the whole answer are unchanged. The HTTP gateway
reads Ollama's NDJSON and the server-sent events of the OpenAI-compatible
and Anthropic endpoints line by line; a status outside 2xx is a refusal
like before, and the output-limit rename (section 22) retries once here
too. Only `Explain` streams: a quick fix is one line and comes whole.

Two things differ from issue 5's sketch, for reasons found in the code.
The daemon sends no `stream` frame: the panel asks its models from its
own thread and receives through its own channel (section 21), so the
pieces travel that channel as `Arrival::Chunk`, named after their model,
and the daemon's protocol keeps the `stream` frame reserved and unused.
And the hooks changed nothing: since section 23 the binary owns the rows
it draws in and leaves the cursor where the hook redraws the prompt, so
growing is the binary's business. `Panel::height` follows the content
after every piece; the terminal gateway, when the panel wants more rows
than it has, scrolls the screen by what would not fit below and opens the
viewport again where the panel now stands. It never shrinks while open,
and never past the design's fourteen rows: longer answers scroll inside,
anchored at the top, where reading starts. A piece from another model
starts afresh (the first model failed after it had begun); a piece that
lands once the answer is whole is late and changes nothing; once the
panel closes, its channel closes and the rest of the stream is dropped.
Streaming is not "impl Future": the gateway was blocking and stays so,
the panel's thread is where the waiting happens.

## 32. The cost ledger and the daily budget (2026-09-29)

Every model call goes through `routing::ask_first`, so that is where it is
metered: the gateways answer with the tokens the provider counted (a new
`answer` method on the `ModelGateway` port, additive; `complete` stays for
callers that want the text alone), and a `CostLedger` port keeps one line
per call in `<state>/ledger.jsonl`. Cost is the provider's list price for
the model ids we know (a dated table in `entities/cost.rs`), zero for a
local model, and *unknown* otherwise: never zero for a model we cannot
price, so `kintsu costs` says "price unknown" rather than "free". Money is
micro-dollars, USD only, validated at the edge (`"1.00 USD"`, `"$2"`,
anything else is a configuration error). A day is a UTC day, because the
entities have no clock and no time zone; the documentation says midnight
UTC. `max_daily_cost` lives under `[routing.constraints]`, where the
configuration page always had it, not under `[routing]`. Once today's
priced spend reaches it, `model_candidates` drops the remote models the
way it drops them for a sensitive case; local models are never skipped.
The eager fix tells the shell once a day, through a `budget_noted` line in
the ledger; `kintsu fix` and `kintsu why` say it every time, in their own
report. A ledger that cannot be written never costs the user the answer
it just paid for, and one that cannot be read counts as empty: the budget
is a comfort, not a lock. Failed calls are not recorded: the providers
report no usage for them.

## 33. `kintsu models` and `kintsu login` (2026-09-29)

Brought forward from v0.3 (open question 11). `kintsu models` is the
table `docs/MODELS.md` describes, without the cost columns the ledger
will bring: provider, tier, key status, reach. It calls no model; the
key is looked up through the `Secrets` port and reported as found or
missing, never shown. `kintsu models test` asks each model one word
through the `ModelGateway` port and times it with the `Clock` port; a
CLI agent is left out with a note, a remote model without a key is not
asked. Exit 1 when a model that should have answered did not.

`kintsu login <model>` is the write side of `{ keychain = true }`: a new
`SecretStore` port (role noun, its error type, an in-memory fake that also
reads back, and the contract every store passes), one gateway over
`security add-generic-password -U` on macOS and `secret-tool store` on
Linux, both found on the shell's PATH. The key is a `SecretKey` newtype
whose `Debug` is redacted; it is read with the terminal's echo off through
`gateways/unix.rs`, the module allowed `libc`, or from stdin when piped.
macOS's `security` takes the password as an argument, so it is visible in
the process list for the instant the command runs; Linux's `secret-tool`
reads it on stdin. `--write-config` edits the one `key` line of the
model's table as text, so the user's comments and layout survive, instead
of re-rendering the file. Models that take no key (Ollama, CLI agents) are
refused before anything is asked.

Tests: the use cases against the fakes, the gateway against a fake
`security`/`secret-tool` that records the exact command line, and an
integration test through the real binary where the fake keychain is on a
scratch PATH: `login`, then `models` finds the key, and no output ever
contains it.

## 34. SQLite behind the three storage ports (2026-09-29)

One gateway, `sqlite_state.rs`, implements `SessionRegistry`, `CaseStore`
and `IgnoreStore` over one file, `<state>/kintsu.db`, in WAL mode with
`synchronous = NORMAL`; it is the only module that speaks SQL, and
`tests/dependency_rule.rs` already forbade `rusqlite` in the inner rings.
The three ports now have one contract suite, `storage_contract` next to
the in-memory fakes in `use_cases/testing.rs`, and the fakes, the JSON
files and SQLite pass the same tests. Cases are columns for what a query
needs (id, session, time, command, status, cwd) plus the JSON document the
JSON store already wrote; sessions keep their recent outcomes as one JSON
column; the ignore list is one row per entry. A saved case becomes the
newest of its session and overall, new or saved again, as the JSON store
did (callers check `still_current` first). Where the JSON files kept one
case per session forever, SQLite keeps the newest 5000 cases: the last of
a shell that failed long ago goes with them.

The first open creates the schema inside an immediate transaction and,
when JSON state is there, imports the sessions, the cases (`last.json`
last, so it stays the last overall) and the ignore list, then moves the
files aside as `*.json.migrated`; a file that cannot be read is left out
rather than failing the migration. A second process opening the file at
the same moment waits on the lock and finds both done. The daemon opens
the store when it starts and keeps one connection, so the first frame does
not pay for the schema inside the sync budget: under an instrumented CI
build it did, and the decision came late. `kintsu doctor`
prints a `state` line: the file, the counts, and what the migration
imported when it did. `JsonState` stays as the reader of that state and
the owner of the document shapes; it is no longer a store the roots use.

Measured end to end with the release binary, forty runs, the machine
busy with four parallel builds: a failed command's triage took 23.4 ms
median with SQLite against 20.9 ms with the JSON files, a successful one
6.7 ms against 7.1 ms, and the process alone 5.6 ms. SQLite costs the
quiet path nothing measurable beyond the noise of that load; the 5 ms
budget is not met by either store on a loaded machine, and was measured
before at a few milliseconds on an idle one. `cargo deny check` needed
nothing: `rusqlite` and `libsqlite3-sys` are MIT.

## 35. A fix taken twice becomes a rule (2026-09-29)

The vision's "learn from accepted fixes": a local memory that turns a
repeated model fix into a rule. Built as its own port, `LearnedFixes`, with
the in-memory fake, a contract suite, and a gateway over one JSON file,
`<state>/learned.json`, kept apart from the case store so it moves into
the SQLite store later without touching this. Sections 27 and 28 were
taken by pull requests open at the same time; this is 29.

*Accepted* means: in a session, the command line that followed a case is
that case's proposal, word for word, and it succeeded. Triage sees it: a
success right after a failure reads the shell's last case, and only then,
so a plain success costs nothing on the quiet path; a store that cannot be
read or written loses one lesson, never the decision. Only fixes worth
learning count: a model's, a rule's guess that was not pre-typed (a
pre-typed one is already instant; confirming `git commit -amend` →
`--amend` is worth it), and a learned fix getting surer. A fix whose
danger is not `None` is never learned. Two acceptances make a rule at
confidence 0.8, pre-typed like the built-in ones; three make it 0.9.

The key is the failure's shape: the command line's words and its exit
status, the pair that already makes two failures one duplicate. The issue
suggested program, subcommand and status; that would have offered `make
-j4 test` for a failing `make build`, and `git push --set-upstream origin
main` for a push of another branch. One entry per shape; a different fix
taken for the same shape starts the count over; 500 entries at most, the
one not taken for longest goes first.

The learned fix comes after the built-in rules, on the line then on the
output, and before any model, wherever `rule_fix` runs: triage, `kintsu
fix`, the panel, the agent's brief, the daemon's messages. `kintsu
learned` lists the entries, plain or `--json`; `kintsu learned forget
<program>` or `--all` unlearns.

## 36. A Homebrew formula, rendered from the checksums (2026-09-29)

Priority 5 below, brought forward with the rest on the maintainer's
request. The formula is `packaging/homebrew/kintsu.rb`, the output of
`scripts/homebrew-formula.py` over a release's `SHA256SUMS`: one
`url`/`sha256` pair per target under `on_macos`/`on_linux` and
`on_arm`/`on_intel`, `bin.install` of the single binary the archives
hold (the hooks live in the binary, `kintsu init <shell>` prints them),
a `test` running `kintsu --version`, `livecheck` on the latest release.
No explicit `version`: Homebrew reads it from the URL and `brew audit`
flags a redundant one. `brew audit --strict --online`, `brew style` and
`brew fetch` pass on it inside a tap. The release workflow renders it
after the checksums are published and pushes `Formula/kintsu.rb` to
`nathan-poncet/homebrew-kintsu` when the `HOMEBREW_TAP_TOKEN` secret is
set (a fine-grained token, contents write on that repository, sent as an
HTTP header rather than embedded in the remote URL); without it the job
renders the formula into its summary and says it was not pushed.
Creating the tap repository and the token stays the maintainer's: a
public repository is a decision, not a build step. The site marks the
Homebrew line "once the tap is published" until then.

## 37. An apt repository on the site (2026-09-29)

Priority 5 of the list below, asked for on 2026-09-29. Each release now
ships `.deb` packages for `amd64` and `arm64` next to the tarballs: the
release workflow wraps the static musl binaries it already cross-builds
with `cargo deb --no-build` (`[package.metadata.deb]` in `Cargo.toml`; the
binary, the README and the copyright, no maintainer script: the hook line
is the user's to add, as with every other install). The crate's
`description` became one sentence under eighty characters because Debian
shows it as the package's synopsis and cargo-deb cut the old two-sentence
one mid-phrase; "Any provider, your own key" opens the long description.

The repository itself is the site: `docs/apt/` on `main`, served by
GitHub Pages at `https://nathan-poncet.github.io/kintsu/apt`, suite
`stable`, component `main`. `scripts/apt-repo.py` writes it in pure Python
(it reads the control file out of each `.deb`, so no `dpkg-dev` is needed
and it runs on the maintainer's Mac too): `pool/main/k/kintsu/` holds the
packages, `dists/stable/main/binary-<arch>/Packages(.gz)` and
`dists/stable/Release` are rewritten from what the pool holds, and a
`--self-test` builds fake packages and checks the output. A last job of
`release.yml` runs it on the tag's packages and commits `docs/apt` to
`main` with the tagger's identity. Considered and not chosen: a flat
repository on a rolling GitHub release (no binaries in git, but an unusual
layout and a redirect chain apt has to follow); `Filename` must be
relative to the repository root, so GitHub Pages cannot point at release
assets. The pool therefore carries binaries in git, about 3.5 MB per
release, and `--keep 3` prunes older versions so it does not grow
forever. The v0.2.0 packages, built from the published release binaries,
are in the pool from this change on, so `apt install kintsu` works as
soon as it is merged.

Signing: when the `APT_SIGNING_KEY` secret holds an ASCII-armored private
key, the job imports it, signs `Release` into `InRelease` and
`Release.gpg`, and exports the public key to `docs/apt/kintsu.gpg` and
`kintsu.asc` for `signed-by`. Without it the repository is published
unsigned and the documentation says `[trusted=yes]` is needed meanwhile.
To create the key: `gpg --quick-gen-key "kintsu releases <email>" ed25519
sign never`, then `gpg --armor --export-secret-keys <fingerprint> | gh
secret set APT_SIGNING_KEY --repo nathan-poncet/kintsu`; on the next tag
the site gets the public key, and the `[trusted=yes]` line leaves the
docs. cargo-deb writes an empty `Depends:` field for a static binary;
dpkg accepts it and the `Packages` index drops it.
## 38. The harness runs in CI (2026-09-29)

Open question 18 of 2026-09-25, answered "in CI". `scripts/shell-harness.py
--check` runs every scenario and asserts what a human read on the screen
until now: the bubble under a failure and the model's answer under it, the
answer landing above a line being typed with the typed text intact, the
answer after another command, `kintsu why`, a slow model's answer naming
its command once the shell moved on, the rule fix pre-typed in zsh and
taken by Tab in fish, and the panel's steps in the three shells, the
remembered explanation proven by the fake model's call count. On a
mismatch the screen is dumped and the exit status is 1. The human mode is
unchanged.

Two things the check mode does not assert, on purpose. `kintsu why` comes
after a `clear`, so the shell has moved on and the explanation names its
command; where that answer lands relative to the `kintsu why` line depends
on whether it arrives before or after the next prompt is drawn, and one
run out of several put it over the prompt's rows. That is the prompt
arithmetic of issue #1's family, not the harness's business. And the
scenarios wait fixed times: `SLOW` stretches them all, CI runs with 2.

The harness used to `pkill` every `kintsu daemon run` between scenarios,
the maintainer's included; it now asks the daemon on its own socket to
stop, and no other. Several harnesses can run at once with distinct
`ROOT`s.

The CI job installs Ubuntu's zsh 5.9, fish 3.7 and bash 5.2; the
maintainer runs fish 4.8 and bash 5.3. Every scenario passes on both.

The first CI run showed a race the laptop never did: the runner missed
the 40 ms sync budget on a frame, the hook decided locally and saved a
second case for the same failure, and the daemon's answer, arriving on the
subscription a moment later, was labelled late because the case it was
about was no longer the shell's last. The configuration page has
documented `[daemon] sync_budget` since v0.1 while the binary ignored it;
it is read now (default 40 ms, section 8 corrected) and the harness sets
it to 2 s. Fixing the race itself, one case for one failure whichever
side decided, is left open.

## What is not built, by priority

1. The pty harness in CI.
2. The Homebrew tap's repository and token (section 36); the apt
   signing key and its secret (section 37).
