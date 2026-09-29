# Changelog

All notable changes to kintsu are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed

- State lives in one SQLite file, `<state>/kintsu.db`, instead of JSON
  files: sessions, the last cases and the ignore list. The first start
  imports the JSON state and moves the files aside as `*.json.migrated`;
  `kintsu doctor` shows the store, its counts and what was imported. The
  newest 5000 cases are kept.

### Added

- `[ui] hotkey`: the key that opens the panel is configurable (`^O`,
  `ctrl-o`, `C-o`); the three hooks bind it and the bubble names it. Keys
  the terminal or the line editor own (Tab, Enter, Backspace, `^C`, `^D`,
  `^Z`, `^S`, `^Q`) are refused with a message that says which.
- `[capture] stderr_tee = true`: the zsh and bash hooks copy each
  command's stderr through `tee` into the session's file, and the capture
  reads it first, for terminals no program can read a pane of
  (Terminal.app, Ghostty…). The rules that read the output and the
  model's explanation then see the error there too. Off by default: a tee
  per command, and stderr is no longer a tty for the command. fish cannot
  redirect its own stderr; `kintsu doctor` says so. Re-run `kintsu init`
  after switching it on.
- The panel grows as the explanation streams in: the Why section shows
  the model's words as they land, with a marker while more is coming,
  and the panel takes the rows its text needs, up to fourteen, scrolling
  the screen when they would not fit below. Ollama's NDJSON and the
  server-sent events of OpenAI-compatible and Anthropic endpoints are
  read line by line; `kintsu why` and the eager fix still take the whole
  answer.
- `kintsu models`: one line per configured model with its provider, its
  tier, whether its key is found and where, and whether its server
  answers, without calling any model; `--json` for scripts. `kintsu
  models test` asks each model one word and reports the latency, or why
  it did not answer.
- `kintsu login <model>`: the key is typed once, without echo, and goes
  to the macOS Keychain or the Linux secret-service under the model's
  name, where `key = { keychain = true }` already reads it;
  `--write-config` points the configuration at it, leaving every other
  line of the file as it was. Nothing prints the key.
- A fix you take twice for the same failure becomes a rule: when the
  command line after a failure is the proposed fix and it succeeds, kintsu
  remembers; the second time, the same failure gets that fix instantly,
  pre-typed, with no model asked. Model fixes and unsure rule fixes are
  learned, pre-typed and dangerous ones are not. `kintsu learned` lists
  what was learned (`--json` for programs), `kintsu learned forget
  <program>` or `--all` unlearns. Kept in `<state>/learned.json`, 500
  entries at most.

- Sixteen more instant rules, several after thefuck's: `$ cmd` pasted
  with its prompt, a no-break space or a trailing `ç` in the line, `cd..`,
  `git-log` and `gitpush`, `mandiff`, `gradle` for `./gradlew`, `app.py`
  without its interpreter, `git git push`, `git commit -amend`, `python
  app` for `app.py`, `./x` without its execute bit, `cat` and `rm` on a
  directory, `mkdir a/b/c` without `-p`. Subcommand typos are known for
  npm, pnpm, yarn, docker, podman, brew, go, pip, kubectl, gh, apt, dnf,
  yum, hg, terraform, conda, gem, composer and systemctl, not only git
  and cargo.
- Twenty-six rules that read the command's output, once the terminal gave
  it and before any model is asked: git's own hints (`push
  --set-upstream`, a pull to set tracking, a rejected push, "stash them",
  a branch that exists, `main` for `master`, an untracked pathspec, a new
  branch, nothing staged, unrelated histories, a rebase to skip, `git rm`
  on a directory or a changed file), any tool's "did you mean" in the
  forms git, cargo, npm, gh, kubectl, pip, yarn, gem, terraform and hg
  print, `pip --user`, a missing Python module and its PyPI name, `mkdir
  -p`, a destination directory to create for `cp`, `mv` and `touch`,
  `rm`, `grep` and `cp` on a directory, a changed ssh host key,
  `--help`, and `sudo`, or its removal. A rule's fix arrives as a bubble
  like a model's would, with no model configured at all. `ssh-keygen -R`
  gets a red line.
- A cost ledger and a daily budget. Every model call is kept in
  `<state>/ledger.jsonl` with its tokens, latency and cost at the
  provider's list price (free for local models, unpriced when the model
  id is unknown). `kintsu costs [--json]` shows today and the last thirty
  days per model. `[routing.constraints] max_daily_cost = "1.00 USD"`
  skips remote models for the rest of the UTC day once reached; local
  ones keep answering, the shell is told once a day, and `kintsu doctor`
  shows the day's spend.

### Fixed

- The rules look at the shell's PATH, not the daemon's: under launchd the
  daemon's is the bare system one, and `clade` was corrected to `clang`
  instead of `claude`. The hook's frame now carries the shell's PATH and
  the daemon remembers it per session for a later click on fix.
- The daemon reads the API keys the shell sees: under launchd or systemd
  its environment has none, so `{ env = "…" }` keys never reached the
  eager fix, `kintsu why` or the panel. The hook's frame carries the
  values of the variables the configured models name, nothing else, and
  the daemon keeps them in memory per session. `kintsu doctor` says so
  when the daemon runs as a service.
- The shell registers itself when it starts: the hooks run `kintsu
  session new` once, and the daemon knows the shell, its pane, its PATH
  and its keys before its first failure. A frame that omits them gets
  what the shell registered. Shells that are gone are forgotten, so the
  daemon's registry never grows past the shells that exist.

## [0.2.0] - 2026-09-25

### Added

- The panel: `^K` expands the last bubble under the prompt, with a
  section per word (Why, Fix, Agent, Ignore, Privacy: `w f a i p`, Tab
  cycles). Why and Fix ask your models on entry and show the answer as it
  lands; ⏎ puts the fix (or `kintsu agent`) in your line and closes the
  panel, `c` copies it, Ignore applies the scope you pick, `esc` closes.
  `kintsu panel` is the command the hook runs. The bubble's actions line
  now ends with `^K more`.
- The panel remembers: an explanation, whether the panel, `kintsu why` or
  a message asked for it, and a quick-fix model's answer are kept with the
  failure, so `^K` shows them again instead of asking again.
- Pipelines: the hooks report every stage's status, and the first stage
  that failed is the failure. `gti status | head` is a typo even though
  `head` succeeded, `foo | grep x` under `pipefail` stays quiet when grep
  found nothing, and the bubble names the stage that failed.
- The bubble's words are clickable: `kintsu why`, `fix`, `agent` and
  `ignore` are OSC 8 links to `kintsu://act?case=…&do=…` on terminals
  that render them (`[ui] links = false` turns them off). `kintsu open
  <url>` is what the desktop runs on a click; the daemon answers in the
  shell the failure happened in, and a click never starts an agent nor
  inserts a command.
- `kintsu service install|uninstall`: the daemon as a launchd agent or a
  systemd user unit, and the `kintsu://` scheme handled by `kintsu open`
  (an app bundle on macOS, a `.desktop` entry on Linux).
- `kintsu setup`: three questions (a local model with Ollama, a cloud
  model and where its key is, the agent for `kintsu agent`), then the
  configuration file and doctor's report; `--yes` takes the defaults.
- Ghost text: a fix that is confident and harmless is pre-typed, dim, on
  the next zsh prompt; Tab or → accepts it. In fish, Tab on an empty line
  inserts it. The bubble says "Tab to fix".
- The failed command's output is read from the terminal after the bubble
  and kept with the case: Herdr, tmux, WezTerm, Kitty and iTerm2, tried in
  the order of `[capture] sources`, at most `max_lines` lines. `why`, the
  quick fix, `privacy` and the agent's brief see it.

### Fixed

- `^K` opens the panel in the bubble's place and puts the bubble back on
  close, instead of drawing a second copy of the failure under the prompt.
- `^K` after a command that succeeded no longer opens the panel on an
  older failure: the panel expands the failure just before, and stays
  closed once the shell has moved on.
- A model's late answer about one command no longer reads as being about
  the next one: once the shell has moved on, the answer names its command
  ("git status: local had no fix for this one.") and offers no keys; the
  proposal is kept for `kintsu fix`. The "asking…" line it would have
  replaced is blanked when the next command starts, in zsh and fish. In
  bash, `kintsu why` answers in place, since bash hears no message before
  its next prompt.
- Messages that arrive later through the hooks are coloured like the
  bubble again.
- The output kept with a failure no longer starts at kintsu's own bubble:
  a line such as "git status exited 128." was taken for the prompt's echo,
  so `why`, the quick fix and the agent saw the bubble instead of the
  error. Kintsu's lines are now skipped and stripped from the output.
- A pane read or a model's answer that lands after a newer failure no
  longer overwrites that newer failure as the shell's last one; `why`,
  `fix` and the panel act on what just failed.
- `why` is told to explain the command that failed and to treat the
  earlier commands of the shell as context only, so a typo you already
  fixed is not explained again. A model proposing the very command that
  just failed is no longer shown as a fix.

### Changed

- `^K` opens the panel instead of inserting the fix directly: ⏎ in the
  panel inserts it. `kintsu fix --raw` still prints the bare command.
- OpenAI's own endpoint is sent `max_completion_tokens`, which its newer
  models require; compatible servers still get `max_tokens`, and a server
  refusing one name is asked once more with the other.
- The `aider` preset is `aider --read {brief}`: the chat stays open with
  the brief in context, where `--message-file` processed it and exited.
- Case ids come from the system's randomness.
- Rust 1.88 is the minimum toolchain (ratatui).
- `kintsu triage` exits 0 in every case; the hooks learn that an "asking…"
  line is waiting from the marker file both `triage` and `why` leave.
- Internal: the daemon's session registry, the `libc` calls and the marker
  are gateways of their own; one use case sends every message; each part
  of a case is redacted separately; the pseudo-terminal harness for the
  hooks lives in `scripts/`.

## [0.1.1] - 2026-09-25

### Added

- The installer offers to install Ollama and pull `qwen2.5-coder:7b`
  (`--no-model` to skip), and writes the default configuration when there
  is none; that configuration routes the local model for `quick_fix` and
  `explain`, so fixes and explanations work out of the box and never leave
  the machine.
- A local model whose server is not running is not asked and not
  announced; `kintsu doctor` says how to start it.

### Changed

- `kintsu why` no longer blocks the prompt when the daemon runs: it prints
  "asking …", and the explanation arrives as a message that takes that
  line's place. Without a daemon it answers in place as before.

## [0.1.0] - 2026-09-25

### Added

- The bubble carries an "asking …" line while the quick-fix model is
  asked, replaced in place by the answer; `ui.eager_fix` defaults to
  `"auto"`, on only when that model is local.
- The resident daemon (`kintsu daemon run|status|stop`), started by the
  first hook call, answering `command_finished` within a 40 ms budget with
  a local fallback, and delivering messages into live shells: `zle -F` in
  zsh, SIGUSR1 in fish, next prompt in bash. With `ui.eager_fix = true`
  the quick-fix model is asked in the background after any failure no rule
  could fix, and its answer arrives above the prompt; `kintsu fix` and
  `^K` reuse it.
- The application: `kintsu triage`
  records every command line of a shell session and decides whether to
  show the bubble; `kintsu fix [--raw]` from five rules (command typo,
  git/cargo subcommand typo, missing `./`, `cd` typo, apt→brew on macOS)
  then from a quick-fix model; `kintsu why` from the first configured model
  that answers; `kintsu agent [--with name] [words…]` writes a redacted
  brief and launches a CLI agent; `kintsu privacy` shows what would be
  sent; `kintsu ignore --command|--dir|--session|--always` and `kintsu
  mute`; `kintsu doctor`, `kintsu default-config`, `kintsu config path`.
- Redaction of API keys, bearer tokens, JWTs, URL passwords, secret-like
  assignments and private key blocks before anything reaches a model or an
  agent; a case holding a secret only reaches local models.
- Providers: Ollama, any OpenAI-compatible endpoint (OpenAI, OpenRouter,
  Groq, LM Studio, Gemini's compatible endpoint), Anthropic; keys from an
  environment variable, a command, the OS keychain or the file.
- The configuration file (`~/.config/kintsu/config.toml`) following the
  documented schema, validated at the edge into typed settings.
- Hooks report every command line with its directory, session and
  duration, export `KINTSU_SESSION`, honour `KINTSU_DISABLE=1`, and bind
  `^K` to insert the fix for the last failure.
- CI: a coverage job (cargo-llvm-cov, fails under 90 % of lines), a
  cargo-deny job (advisories, licences, bans, sources), a release workflow
  on `v*` tags (four targets, `SHA256SUMS`), a weekly dependency workflow.
- [docs/DECISIONS.md](docs/DECISIONS.md): the decisions taken while
  building v0.1 and what was left out.

- `install.sh`, the one-line installer: detects the platform, installs the
  latest release (checksum verified) or builds from source with cargo while
  there is none, puts the binary in `~/.local/bin`, asks before adding the
  hook to zsh, bash or fish; `--dir`, `--version`, `--from-source`,
  `--no-hook`, `--yes`, `--dry-run`, `--uninstall`. Mirrored in `docs/` so
  GitHub Pages serves it.
- The design documents: the crate map and the rings
  ([docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)), the resident daemon and
  its protocol ([docs/DAEMON.md](docs/DAEMON.md)), models routed per task
  ([docs/MODELS.md](docs/MODELS.md)), the bubble, toast and panel
  ([docs/UI.md](docs/UI.md)).
- The rings as folders of one crate, `src/entities`, `src/use_cases` with
  `ports/`, `src/adapters` with `controllers/`, `presenters/` and
  `gateways/`, and `tests/dependency_rule.rs` enforcing the Dependency Rule
  and the purity of the inner rings.
- The website in `docs/`: static HTML, CSS and JavaScript. The landing
  page says the minimum, a live terminal to click into, five strengths and
  one line to install; the documentation is one page per subject:
  `docs.html` the hub, `install.html`, `keys.html`, `configuration.html`
  `faq.html`, `roadmap.html` a one-screen timeline. Published with
  GitHub Pages at <https://nathan-poncet.github.io/kintsu/>.
- The project vision, competitive landscape and feature roadmap
  ([docs/VISION.md](docs/VISION.md)).
- Shell hooks for zsh, bash and fish (`kintsu init <shell>`) that call
  `kintsu triage` after each command line.
