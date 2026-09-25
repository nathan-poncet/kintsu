# Changelog

All notable changes to kintsu are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- The panel: `^K` expands the last bubble under the prompt, with a
  section per word (Why, Fix, Agent, Ignore, Privacy: `w f a i p`, Tab
  cycles). Why and Fix ask your models on entry and show the answer as it
  lands; ⏎ puts the fix (or `kintsu agent`) in your line and closes the
  panel, `c` copies it, Ignore applies the scope you pick, `esc` closes.
  `kintsu panel` is the command the hook runs. The bubble's actions line
  now ends with `^K more`.
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

### Changed

- `^K` opens the panel instead of inserting the fix directly: ⏎ in the
  panel inserts it. `kintsu fix --raw` still prints the bare command.
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
