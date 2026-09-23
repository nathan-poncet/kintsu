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
- Passive bubble by default (a printed hint) plus a hotkey (`Ctrl-K`?) and
  the bare `kintsu` command to act on the last failure. No stolen keystrokes.
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

**The bubble.** Appears under the failed command's output, before the next
prompt. Three lines at most. Default variant is a single dim line so it
never dominates the screen; a boxed variant and "hotkey only" are options.
It never steals keystrokes at the prompt: reading a key inside `precmd`
would block or eat the next command. Actions come from the hotkey, the
bare `kintsu` command, or a shell widget (ZLE in zsh, `bind` in fish,
`bind -x` in bash).

**Ghost text.** For rule and one-shot fixes, the corrected command appears
as ghost text in the line editor; Tab accepts, anything else ignores.
Confirmation before execution is implicit here: the user presses Enter.

**Agent hand-off.** The agent starts in the same pane by default (the
user's terminal, the user's agent, they know how to use it). Options: a
tmux/Herdr split, or `--print` mode for a non-interactive answer streamed
under the bubble.

**Noise.** The same failure twice in a row is reported once. A cooldown
after a dismiss. `kintsu ignore` with scopes: this command, this directory,
this session, always. `kintsu mute 1h`.

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

Same shape as herdr-fingers: one Rust binary, Clean Architecture, TDD.

- `domain/`: the failure case, rules, redaction, triage decision. Pure,
  no I/O.
- `usecases/`: `triage`, `why`, `fix`, `agent`, `ignore`, against ports
  (`OutputSource`, `Model`, `Agent`, `Clipboard`, `Config`, `History`).
- `adapters/`: shell hooks, tmux/Herdr/WezTerm/Kitty/iTerm2 output sources,
  provider clients, agent launchers, TOML config, keychain, ratatui popup.
- `app.rs`: composition root, one function per subcommand.

Rust because the quiet path runs after every command and must start in
well under a millisecond; a single static binary installs everywhere; the
author already has the tooling from herdr-fingers.

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

- Passive hint versus interactive bubble: is a printed line plus a hotkey
  enough, or do people expect to press a key right there?
- What happens with no provider configured: rule fixes only, plus a
  one-line invitation to `kintsu setup`?
- Where does the agent open: same pane, split, or streamed answer?
- Should a rule ever auto-apply? Current answer: no, opt-in per rule at most.
- Bash without bash-preexec: is PROMPT_COMMAND plus history good enough?
- MIT or Apache-2.0?
- Does the name survive a week?

## The name

Kintsugi (金継ぎ, "golden joinery") is the Japanese craft of repairing
broken pottery with lacquer mixed with gold. The repair is not hidden; it
becomes the most visible and most valued part of the object. A failed
command is a crack. kintsu is the gold seam, and, with a bit of luck, what
you learn from the fix is worth more than a command that never failed.

Availability checked 2026-09-23: free on crates.io, npm, PyPI and Homebrew;
no GitHub project of that exact name. Runner-up was `pardon` (free on
crates.io and Homebrew, taken on npm by Adobe).
