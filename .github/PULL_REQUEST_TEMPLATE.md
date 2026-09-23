## What & why

<!-- What this PR changes and the problem it solves. Link the issue if there is one. -->

## Checklist

- [ ] `cargo fmt --all --check` passes
- [ ] `cargo clippy --all-targets -- -D warnings` passes
- [ ] Tests cover the behaviour change (`cargo test`)
- [ ] The quiet path still does no network and nothing a user could feel
- [ ] No suggested command can execute without the user pressing Enter
- [ ] Anything a user would notice has a line under `[Unreleased]` in `CHANGELOG.md`
- [ ] Commit subjects start with a [Gitmoji](https://gitmoji.dev) and are in English
