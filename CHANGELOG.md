# Changelog

All notable changes to kintsu are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- The application, synchronously and without the daemon: `kintsu triage`
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
