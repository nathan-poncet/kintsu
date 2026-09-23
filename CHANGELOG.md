# Changelog

All notable changes to kintsu are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- The project vision, competitive landscape and feature roadmap
  ([docs/VISION.md](docs/VISION.md)).
- Shell hooks for zsh, bash and fish (`kintsu init <shell>`) that call
  `kintsu triage` with the exit status and text of every command that fails.
- `kintsu triage`, a placeholder that prints a one-line hint after a failure
  and stays quiet on success, Ctrl-C, broken pipes and Ctrl-Z. Nothing talks
  to a model yet.
