# Contributing to kintsu

Thanks for helping! kintsu is at the design stage, so the most useful
contribution today is an opinion on [docs/VISION.md](docs/VISION.md): open
an issue, disagree, propose. Code contributions follow the rules below.

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
cargo xtask check
cargo build --release --locked
bash -n shell/kintsu.bash && zsh -n shell/kintsu.zsh && fish -n shell/kintsu.fish
```

## Architecture in one minute

Clean Architecture, one crate per ring; see
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

| ring | crate | may depend on (of ours) |
|---|---|---|
| Entities | `crates/entities` (`kintsu-entities`) | nothing |
| Use cases + ports | `crates/use_cases` (`kintsu-use-cases`) | entities |
| Adapters: controllers, gateways, presenters | `crates/adapters` (`kintsu-adapters`) | entities, use cases |
| Composition root | `apps/kintsu` | all three |

`cargo xtask check` (also run by `cargo test`) fails the build when a crate
reaches outward or when an inner ring touches I/O, a runtime, a terminal or
the clock. Ground rules:

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

## Reporting bugs and proposing features

Use the issue templates. Security flaws go through the private channel
described in [SECURITY.md](SECURITY.md), never through a public issue.
Everyone taking part is held to the [code of conduct](CODE_OF_CONDUCT.md).

## Licensing of contributions

kintsu is MIT. By opening a pull request you agree that your contribution
is licensed under the same terms.
