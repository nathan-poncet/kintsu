# Changelog

All notable changes to kintsu are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- The design documents: the crate map and the rings
  ([docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)), the resident daemon and
  its protocol ([docs/DAEMON.md](docs/DAEMON.md)), models routed per task
  ([docs/MODELS.md](docs/MODELS.md)), the bubble, toast and panel
  ([docs/UI.md](docs/UI.md)).
- The rings as folders of one crate, `src/entities`, `src/use_cases` with
  `ports/`, `src/adapters` with `controllers/`, `presenters/` and
  `gateways/`, and `tests/dependency_rule.rs` enforcing the Dependency Rule
  and the purity of the inner rings.
- The website in `docs/`: static HTML, CSS and JavaScript, a tour of the
  bubble in nine chapters with pause, chapter navigation and a step-by-step
  mode, then every action one by one; ready for GitHub Pages, not
  published yet.
- The project vision, competitive landscape and feature roadmap
  ([docs/VISION.md](docs/VISION.md)).
- Shell hooks for zsh, bash and fish (`kintsu init <shell>`) that call
  `kintsu triage` with the exit status and text of every command that fails.
- `kintsu triage`, a placeholder that prints a one-line hint after a failure
  and stays quiet on success, Ctrl-C, broken pipes and Ctrl-Z. Nothing talks
  to a model yet.
