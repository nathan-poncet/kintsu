# Architecture

*Draft. The code is a skeleton; this describes the target shape so that the
first real pieces land in the right place. Rationale in
[VISION.md](VISION.md#architecture-sketch).*

## One binary, four rings

| Ring | Folder | May use | Role |
|---|---|---|---|
| Kernel | `src/domain/` | `regex`, `thiserror` and nothing that does I/O | the failure case, triage decision, rules, redaction |
| Use cases | `src/usecases/` | the kernel and the ports it declares | `triage`, `why`, `fix`, `agent`, `ignore` |
| Adapters | `src/adapters/` | anything | shell hooks, output sources, model providers, agent launchers, config, keychain, popup |
| Composition root | `src/app.rs`, `src/main.rs` | anything | one function per subcommand: build adapters, call a use case |

A dependency-rule test will fail the build if the kernel or a use case
imports an adapter, `std::io`, `std::fs`, `std::process` or a terminal
crate.

## The pipeline

```
shell hook ──▶ kintsu triage ──▶ TriageDecision
                  │                 ├─ Quiet            (nothing printed)
                  │                 ├─ RuleFix(cmd)     (ghost text or hint)
                  │                 └─ Offer(case)      (the bubble)
                  │
                  └─ collects: command, status, duration, cwd, git,
                     history, output via an OutputSource if one is
                     available (tmux, Herdr, WezTerm, Kitty, iTerm2,
                     opt-in stderr tee)

kintsu why / fix / agent ──▶ load last case ──▶ redact ──▶ preview? ──▶
                                Model (why, fix)  or  Agent (hand-off)
```

## Ports (first cut)

- `OutputSource`: `read_last_command_output(case) -> Option<Text>`
- `Model`: `complete(prompt, stream) -> Stream<Chunk>`; implementations for
  OpenAI-compatible, Anthropic, Ollama.
- `Agent`: `hand_off(case, prompt) -> Result<()>`; implementations for
  Claude Code, Codex, OpenCode, aider, Gemini CLI, Copilot, and a command
  template.
- `History`: recent commands from the shell.
- `CaseStore`: persist the last failure so `kintsu why` can find it.
- `Config`: typed `Settings` parsed at the edge from TOML.

## Performance budget

The quiet path (`kintsu triage` deciding to say nothing) runs after every
failed command and must finish in under 5 ms including process start. No
network, no config re-parse beyond a cached mtime check, no allocation
storm.
