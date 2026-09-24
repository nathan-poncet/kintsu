# Contributing to kintsu

Thanks for helping! kintsu is pre-alpha: the bubble and the commands work,
the daemon does not exist yet. Opinions on [docs/VISION.md](docs/VISION.md)
and [docs/DECISIONS.md](docs/DECISIONS.md) are as welcome as code. Code
contributions follow the rules below.

## Setup

```sh
git clone https://github.com/nathan-poncet/kintsu.git
cd kintsu
cargo build --release
eval "$(./target/release/kintsu init zsh)"    # or bash / fish
```

## Before you push

CI runs exactly these, on Linux and macOS. Run them locally first:

```sh
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
bash -n shell/kintsu.bash && zsh -n shell/kintsu.zsh && fish -n shell/kintsu.fish
cargo llvm-cov --all-targets --ignore-filename-regex 'src/main\.rs' --fail-under-lines 90   # cargo install cargo-llvm-cov
cargo deny check                                                                           # cargo install cargo-deny
```

The hooks draw above a live prompt, which no unit test can see. After a
change under `shell/`, run the screen harness (needs `pip install pyte`,
zsh and fish):

```sh
cargo build && python3 scripts/shell-harness.py && python3 scripts/shell-harness.py zsh
```

A release is a tag: bump `version` in `Cargo.toml`, move the `[Unreleased]`
notes under the version in `CHANGELOG.md`, `git tag vX.Y.Z && git push --tags`.
The release workflow verifies the tag, drafts the release from the
changelog, builds the four targets and publishes `SHA256SUMS`.

## Architecture in one minute

Clean Architecture in one crate, the rings are folders; see
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

| ring | folder | may import (of ours) |
|---|---|---|
| Entities | `src/entities/` | nothing |
| Use cases + ports | `src/use_cases/`, `src/use_cases/ports/` | entities |
| Adapters: controllers, gateways, presenters | `src/adapters/` | entities, use cases |
| Composition root | `src/app.rs`, `src/main.rs` | everything |

`tests/dependency_rule.rs` fails the build when an inner ring reaches
outward or touches I/O, a runtime, a terminal or the clock. Ground rules:

- **The inner rings do no I/O.** Entities and use cases decide; gateways act.
- **Validate at the edge.** Config becomes typed `Settings` in the adapter;
  the kernel never interprets a raw string.
- **A new side effect gets a port first**, with an in-memory fake for tests.
- **The quiet path is sacred.** Nothing that runs after every failed
  command may touch the network or do anything a user could feel.
- **Never execute a suggestion.** The user presses Enter, always.

## Tests

TDD is the house style: write the failing test first.

- Test names state behaviour: `a_successful_command_stays_quiet`, never
  `test_triage_1`.
- Deterministic always: fake providers, fake agents, fake output sources,
  temp directories. No real network, no real terminal, no sleeps.
- A new rule gets a test with its canonical failing command and expected fix.

## Commits and PRs

- Code, comments and commit messages are in **English**.
- Prefix commit subjects with a [Gitmoji](https://gitmoji.dev): ✨ feature,
  🐛 fix, ♻️ refactor, ✅ tests, 📝 docs, 👷 CI, 🔒 security…
- Keep PRs focused: one feature or fix per PR, with tests for behaviour
  changes.
- Anything a user would notice gets a line under `[Unreleased]` in
  [CHANGELOG.md](CHANGELOG.md).

## The website

`docs/index.html`, `docs/styles.css` and `docs/script.js` are the site,
plain static files next to the design documents, served by GitHub Pages
(source: `main`, folder `/docs`). Preview it locally with:

```sh
python3 -m http.server -d docs 8000    # then open http://localhost:8000
```

No build step, no framework, no tracking. The interactive demo is vanilla
JavaScript and the page still reads without it.

`install.sh` at the root is the installer; `docs/install.sh` is the copy
GitHub Pages serves at the one-liner's URL. Edit the root one and copy it
over; CI fails when they differ.

The documentation pages share one shell: header with search, sidebar,
"On this page", previous/next. `scripts/docs-shell.py` writes it into every
page and builds `docs/search-index.js` from their headings; run it after
editing a documentation page (CI runs it with `--check`). Edit the content
inside `<main>`, never the generated parts.

## Reporting bugs and proposing features

Use the issue templates. Security flaws go through the private channel
described in [SECURITY.md](SECURITY.md), never through a public issue.
Everyone taking part is held to the [code of conduct](CODE_OF_CONDUCT.md).

## Licensing of contributions

kintsu is MIT. By opening a pull request you agree that your contribution
is licensed under the same terms.
