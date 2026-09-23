# kintsu: vision and brainstorm

*Living document, started 2026-09-23. Everything here is up for debate;
open an issue to push back.*

## The itch

A developer runs somewhere between a few dozen and a few hundred commands a
day. A meaningful share of them fail: typos, a missing flag, a tool that
isn't installed, a port already in use, a dependency that changed its CLI,
a permissions issue, a Docker daemon that isn't running. Each failure costs
the same detour: read the error, copy it, leave the terminal, paste it into
a search engine or a chat, re-explain what you were doing, read the answer,
come back, retype. One to three minutes each, dozens of times a day, and a
context switch every time.

The shell already knows everything the assistant will ask for: the exact
command, its exit status, what it printed, the working directory, the git
branch, the last ten commands, the OS, the installed versions. None of it is
used.

Warp proved the UX: when a command fails, the terminal itself offers to
explain and fix it, with the output already attached. But that only works
inside Warp, with a Warp account, with Warp's choice of models and quota.
Plenty of people will never switch terminal for that.

## Who it is for

- Developers who chose their terminal (Ghostty, WezTerm, Kitty, iTerm2,
  Alacritty, or a plain tmux over SSH) and their shell, and will not trade
  them for an AI feature.
- People already paying for one agent (Claude Code, Codex, Copilot, Gemini
  CLI, OpenCode with any model) who want the terminal to feed it instead of
  copy-pasting into it.
- Privacy-minded and corporate users who need a local model or a specific
  approved endpoint.
- Secondarily, people new to the shell, for whom `command not found: gti`
  is not obviously a typo.

## The core loop

```
 command exits non-zero
          │
          ▼
 shell hook (zsh preexec/precmd, bash PROMPT_COMMAND, fish fish_postexec)
          │  status, command line, duration
          ▼
 kintsu triage  ── quiet? (success, Ctrl-C, denylist, cooldown, dedupe) ──▶ nothing
          │
          ├── a rule knows the fix (typo, missing sudo, wrong package manager…)
          │        ▼
          │   ghost-text suggestion, Tab to accept
          │
          └── needs a brain
                   ▼
              bubble: fix · agent · why · ignore
                   │
     ┌─────────────┼──────────────────┐
     ▼             ▼                  ▼
 one-shot fix   hand-off to        explanation
 from a model   the configured     streamed
 (confirm       agent with the     in place
  before run)   full case
```

Everything up to the bubble runs locally and must be imperceptible. The
model is only involved once the user asks.

## What exists already

Researched 2026-09-23. Stars are indicative.

| tool | what it does | how it triggers | providers | limits for our purpose |
|---|---|---|---|---|
| [Warp](https://www.warp.dev) | Terminal with Agent Mode. After a failure, a hint offers to attach the output as agent context; agent-generated commands that fail are re-analysed automatically. | on failure, inside Warp | Warp's models and quota | Proprietary terminal, account required, no BYOK, not your terminal. |
| [thefuck](https://github.com/nvbn/thefuck) (~90k★) | Rule-based correction of the last command. | you type `fuck` | none | No LLM, Python startup latency, unmaintained stretches. Its rule set is a goldmine to port. |
| [fixit](https://github.com/nvbn/thefuck#alternatives) and other rewrites | thefuck in a faster language. | you type the alias | none | Same scope as thefuck. |
| [zsh-autofix](https://github.com/deXterbed/zsh-autofix) | Redirects stderr in preexec, shows a ghost-text fix after a failure. | on failure | Ollama only | zsh + oh-my-zsh + zsh-autosuggestions only; stderr redirect breaks interactive programs; tiny project. Validates the ghost-text UX. |
| [ai-fix](https://github.com/anasmohiuddinsyed-bit/ai-fix) | Reads the last command from history, re-runs it to capture the error, asks a model. | you type `ai-fix` | Claude Haiku, GPT-4o-mini | Re-running a failed command is unsafe (non-idempotent commands). Python. |
| [butterfish](https://github.com/bakks/butterfish) (~550★) | Wraps the shell in a PTY; capital-letter prompts; "why did that fail?" sees the history. | you ask | OpenAI-compatible | Heavy PTY wrapper, its own agent, bash/zsh only. |
| [why](https://github.com/kavix/why) | Diagnostic pipelines (DNS, TCP, TLS…) that isolate a root cause, then a model explains. | you run `why <cmd>` | Ollama, Gemini, OpenAI, Anthropic | Great idea for structured diagnostics, not failure-triggered, Go, very early. |
| [zsh-ai-assist](https://github.com/MKSG-MugunthKumar/zsh-ai-assist) | Command generation and error fixing from a keyword. | you ask | Claude | zsh/fish, single provider. |
| Copilot CLI, Amazon Q CLI, Gemini CLI, OpenCode, Claude Code, Codex, aider | Agents that live in the terminal. | you open them | each its own | None watches your shell for failures. They are the **targets** kintsu hands off to, not competitors. |
| shell_gpt, aichat, `llm`, oterm, shellm | General LLM CLIs and chat TUIs. | you ask | many | Not failure-triggered; could serve as provider back-ends. |
| iTerm2 AI plugin, Ghostty/Kitty/WezTerm | Emulators with optional AI or with scriptable scrollback APIs. | varies | varies | The scrollback APIs are exactly what kintsu needs to read the output. |

### The gap

Nobody combines all of these:

1. **Triggered by the failure itself**, not by remembering to type a
   magic word.
2. **Works in any terminal emulator and the three main shells.**
3. **Provider-agnostic, bring your own key, local models included.**
4. **Hands off to a real agent** (the one you already use) with the full
   case, instead of shipping yet another mediocre chat TUI.
5. **Reads the actual output safely**, without re-running the command and
   without breaking interactive programs.
6. **Costs nothing at the prompt**: a native binary with sub-millisecond
   startup for the quiet path.

Warp has 1 and 5 but not 2, 3, 4. thefuck has 2 and 6 but not 1, 3, 4, 5.
zsh-autofix has 1 but not 2, 3, 4, and 5 only partially.

## Positioning

> kintsu turns every failed command into a hand-off to the AI agent of your
> choice, in the terminal and shell you already use.

Tagline candidates: "Repair broken commands with gold." · "Your terminal,
your agent, your key." · "The bubble under the error."

## Principles

- **Your terminal, your shell.** A hook, not a terminal, not a shell, not a
  PTY wrapper.
- **Bring your own model.** Every provider, every key source, local first
  class. Reusing an existing CLI agent counts as a provider.
- **Agent-agnostic hand-off.** kintsu prepares the case; the agent solves
  it. Adapters for the popular agents, a template for anything else.
- **Never runs anything on its own.** Every suggested command goes through
  the user. Dangerous patterns are flagged loudly.
- **Show what leaves the machine.** Preview, redact, choose. No telemetry,
  no phone-home, no account.
- **Fast or invisible.** Quiet path under 5 ms. If the model is slow, the
  prompt is not.
- **Quiet by default.** A tool that speaks after every failure is a tool
  people uninstall. Dedupe, cooldown, denylist, one-key dismiss.
- **Boring to install.** One binary, one line in the rc file, removable
  in one line.

## Features

### v0.1: the bubble

- Hooks for zsh, bash, fish via `kintsu init <shell>`.
- Context collected locally: command line, exit status, duration, cwd,
  shell and version, OS, git branch and dirty state, last N commands.
- Output capture where it is free: `tmux capture-pane`, Herdr `pane read`,
  opt-in stderr tee for zsh. Otherwise the case is command + status only.
- Rule engine for instant fixes, ported from thefuck's most useful rules:
  command-name typos against `$PATH`, `git` "did you mean", `cd` into a
  file, `apt` on macOS / `brew` on Linux, missing `sudo` (flagged, never
  auto), forgotten `./`, wrong branch name.
- One provider layer: OpenAI-compatible (covers OpenAI, OpenRouter, Groq,
  LM Studio, llama.cpp…), Anthropic, Ollama. Keys from env or config.
- `kintsu why`: streamed explanation of the last failure.
- `kintsu fix`: one-shot corrected command, shown, confirm to run.
- `kintsu agent`: hand-off by template. Presets for `claude -p`,
  `codex exec`, `opencode run`, `aider --message`, `gemini -p`,
  `gh copilot`; `command = "..."` for anything else.
- A resident daemon per user, started on demand, with the shell hooks as
  thin clients over a Unix socket ([DAEMON.md](DAEMON.md)).
- The toast, printed in the flow, expandable into the panel with `^K` or a
  click; no stolen keystrokes ([UI.md](UI.md)).
- Config in `~/.config/kintsu/config.toml`, `kintsu default-config`,
  `kintsu doctor`.
- Noise control: exit-code allow/deny, command denylist (editors, pagers,
  ssh, top, watch…), cooldown, same-failure dedupe, `KINTSU_DISABLE=1`,
  per-directory off switch.

### v0.2: it reads the output

- Output capture through semantic prompt marks (OSC 133, FinalTerm) and
  terminal APIs: WezTerm `wezterm cli get-text`, Kitty `kitten @ get-text`,
  iTerm2 Python API, tmux, Herdr. Falls back gracefully.
- Redaction before sending: API keys, tokens, JWTs, passwords in URLs,
  emails and IPs (opt-in), custom patterns. "What leaves the machine"
  preview.
- Ghost-text fixes with Tab to accept, in the style of zsh-autosuggestions,
  in zsh and fish.
- Danger guard: `rm -rf`, `git push --force`, `DROP`, `chmod 777`,
  `curl | sh`, and anything a rule marks as destructive gets a red
  confirmation.
- Project awareness: detect `package.json`, `Cargo.toml`, `pyproject.toml`,
  `Makefile`, `docker-compose.yml`; a `.kintsu.toml` per repository; pick up
  `CLAUDE.md` / `AGENTS.md` when handing off.
- Pipeline failures via `$pipestatus`, not just the last status.

### v0.3: your agent, your keys

- Key storage in the macOS Keychain and Linux secret-service; `kintsu login`.
- Subscription reuse: talk to Claude Code, Codex or Copilot through their
  CLIs so no separate API key is needed.
- Local models as first-class citizens: Ollama, LM Studio, llama.cpp, MLX;
  a tiny model for triage and classification, a large one for the agent.
- Multi-model routing: cheap model decides whether it is worth bothering
  the expensive one.
- `kintsu mcp`: an MCP server exposing `last_failure`, `recent_history`,
  `captured_output` so Claude Code, Cursor or any MCP client can pull the
  case themselves.
- Cost tracking and a daily budget.

### Later, and the ideas parking lot

- Learn from accepted fixes: a local memory that turns a repeated LLM fix
  into a rule.
- Team runbooks: a `.kintsu/runbooks/` folder in the repo, so "port 5432
  in use" gets the team's answer, not the internet's.
- CI mode: `kintsu wrap -- make test` explains a failure in the job log or
  as a GitHub Action comment.
- Explain long output even on success (`kintsu explain`).
- Slow-command hints: something took 40 s, here is why, or here is the
  cached alternative.
- Windows and PowerShell, nushell, elvish.
- Explanations in the user's language (French first, obviously).
- Interactive bubble inside multiplexer popups (`tmux display-popup`,
  Herdr overlay) where stealing keys is safe.
- A Herdr plugin packaging, like herdr-fingers.
- Homebrew tap, cargo-binstall, prebuilt binaries.

## UX details

Moved to [UI.md](UI.md): the bubble has two states, a toast printed in the
flow that never steals a keystroke, and a panel it expands into on `^K` or
on a click. Action words are OSC 8 links, so the toast is clickable in most
terminals without mouse tracking. Messages arrive above the prompt in zsh
and fish, at the next prompt in bash.

## Capturing the output: the hard part

The command's output is the most valuable context and the hardest to get
without side effects.

| approach | pros | cons |
|---|---|---|
| Redirect stderr to a file in preexec (zsh-autofix) | simple, exact | breaks interactive programs and colours; needs a denylist; zsh only |
| `exec 2> >(tee)` process substitution | keeps output on screen | ordering issues between stdout and stderr, still interferes with TTY detection |
| PTY wrapper (butterfish) | sees everything | heavy, fragile, replaces the user's shell experience |
| Re-run the command (ai-fix) | zero setup | unsafe, non-idempotent commands, slow |
| Multiplexer scrollback: `tmux capture-pane`, Herdr `pane read` | zero interference, exact | only when a multiplexer is present |
| Terminal APIs: WezTerm, Kitty, iTerm2 | zero interference | terminal-specific, needs remote control enabled |
| OSC 133 semantic marks | precise output boundaries | requires shell integration and emulator support (WezTerm, Kitty, iTerm2, Ghostty, VS Code, Warp) |

Plan: start with the multiplexer scrollback and terminal APIs (free and
exact), delimited with OSC 133 marks where supported. Offer the stderr tee
as an explicit opt-in for people without any of these. Never re-run.

## Architecture sketch

Moved to [ARCHITECTURE.md](ARCHITECTURE.md), [DAEMON.md](DAEMON.md) and
[MODELS.md](MODELS.md). In five lines:

- One crate, the rings as folders: `src/entities`, `src/use_cases` with
  its `ports/`, `src/adapters` with `controllers/`, `gateways/` and
  `presenters/`, `main.rs` as the composition root;
  `tests/dependency_rule.rs` enforces the Dependency Rule.
- One resident daemon per user, started on demand or installed as a
  service, talking NDJSON over a Unix socket to thin clients: the hooks,
  the `kintsu` commands, the URL-scheme handler behind clicks.
- The sync path (hook to toast) is rules only and takes milliseconds;
  everything that needs a model runs after the prompt is back and lands as
  a message.
- Several models, chosen per task by a router: tiny local for
  classification, small for one-line fixes, large for explanations, the
  user's own CLI agent for investigations, with privacy and budget
  constraints that can force local only.
- Rust, single static binary, Clean Architecture and TDD.

## Privacy and safety

- Nothing leaves the machine until the user picks an action.
- Preview of the exact payload; redaction on by default for known secret
  shapes; custom patterns.
- The provider is the only recipient. No kintsu servers, ever.
- No command is executed without the user pressing Enter on it. Rules may
  be marked `auto` per user choice, never by default.
- Prompt-injection awareness: output captured from the terminal is data,
  never instructions; the hand-off prompt says so and fences it.
- Destructive patterns are flagged before the agent or the user runs them.

## Non-goals

- Not a terminal emulator, not a shell, not a PTY wrapper.
- Not a general chat TUI. If you want to chat, kintsu hands you to one.
- Not an autocomplete engine (zsh-autosuggestions, Fig/Amazon Q, inshellisense
  do that).
- Not a replacement for reading the error yourself when it is obvious.

## Open questions

Decided on 2026-09-23: a resident daemon rather than a stateless hook; a
passive toast that expands into a panel rather than a bubble that steals
keys; OSC 8 links for clickability; several models routed per task; the
name.

Still open:

- Where does the agent open by default: same pane, a multiplexer split, or
  a streamed answer in the panel?
- Should a rule ever auto-apply? Current answer: no, opt-in per rule at
  most.
- Is next-prompt delivery acceptable for bash, or is bash-preexec worth
  depending on?
- Eager `QuickFix` on every offer, or only on request? Eager feels magical
  and costs a model call per failure.
- MIT or Apache-2.0?

## The name

Kintsugi (金継ぎ, "golden joinery") is the Japanese craft of repairing
broken pottery with lacquer mixed with gold. The repair is not hidden; it
becomes the most visible and most valued part of the object. A failed
command is a crack. kintsu is the gold seam, and, with a bit of luck, what
you learn from the fix is worth more than a command that never failed.

Availability checked 2026-09-23: free on crates.io, npm, PyPI and Homebrew;
no GitHub project of that exact name. Runner-up was `pardon` (free on
crates.io and Homebrew, taken on npm by Adobe). Confirmed on 2026-09-23.
