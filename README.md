# kintsu

**A command just failed. kintsu is the small bubble under it that asks
"want me to look into that?" and hands the whole case to the AI agent of
your choice, right there in your terminal.**

[![CI](https://github.com/nathan-poncet/kintsu/actions/workflows/ci.yml/badge.svg)](https://github.com/nathan-poncet/kintsu/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-2ea44f)](LICENSE)
![Linux and macOS](https://img.shields.io/badge/platform-Linux%20%7C%20macOS-c084fc)
![Status: pre-alpha](https://img.shields.io/badge/status-pre--alpha-f59e0b)

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/S3V726AT7H)

> **Status: pre-alpha, design phase.** The shell hooks work and a
> placeholder hint is printed after a failure. Nothing talks to a model yet.
> The plan lives in [docs/VISION.md](docs/VISION.md); pushback is welcome in
> the issues.

## The itch

You type a command. It fails. You squint at the error, copy it, switch to a
browser or a chat window, paste it, re-explain the context your shell
already knew, read the answer, come back, retype. [Warp](https://www.warp.dev)
made that loop disappear, but only inside Warp, with Warp's account and
Warp's AI.

kintsu wants the same loop in *your* terminal and *your* shell, with *your*
model and *your* key.

## How it will work

```
$ gti status
zsh: command not found: gti
▎ Did you mean git status?
▎ Tab to fix · Why · Agent · Ignore · ^K more
$ git status
```

A small bubble, one gold seam on the left, one sentence, a line of words.
The words are links, so they are clickable; `^K` expands the bubble into a
panel with real buttons, streaming explanations and a mouse; and nothing
ever steals a keystroke from your prompt.

- **Fix**, the fast path: a one-line correction shown as ghost text you
  accept with Tab. Rules first (in the spirit of
  [thefuck](https://github.com/nvbn/thefuck)), a small model when rules
  don't know. You press Enter; Kintsu never runs anything itself.
- **Agent**, the real path: Kintsu packages the command, its exit status,
  the captured output, the working directory, git state and recent history
  into a brief and hands it to the agent you configured: Claude Code,
  Codex, OpenCode, aider, Gemini CLI, a plain API call… to investigate and fix.
- **Why**: an explanation, no action.
- **Ignore**: for this command, this session, or forever.

Answers that take longer than a prompt should wait arrive later, as
messages above your prompt, while you keep working. Quiet by default:
nothing on success, on Ctrl-C, on a denylisted command, or twice in a row
for the same failure.

## Principles

- **Your terminal, your shell.** One native binary plus a 30-line hook for
  zsh, bash and fish. Works in any terminal emulator; better with tmux,
  Herdr, WezTerm, Kitty or iTerm2, which let Kintsu read the actual output.
- **One brain, every shell.** A resident daemon per user, started on
  demand, remembers your sessions and your fixes across every tab and
  every shell, and never makes a prompt wait.
- **Bring your own models.** Several at once, chosen per task: a tiny
  local one to decide whether a failure deserves your attention, a small
  one for one-line fixes, a large one to explain, your CLI agent to
  investigate. Anthropic, OpenAI, Gemini, any OpenAI-compatible endpoint,
  Ollama for fully local. Keys from the keychain, env or config. Or no key
  at all: reuse the agent you already pay for.
- **Agent-agnostic hand-off.** kintsu is not another chat TUI. It prepares
  the case and delegates to the agent you like.
- **Never runs anything on its own.** Suggestions are shown, you confirm.
  Dangerous patterns are flagged.
- **Show what leaves the machine.** Preview and redact before sending.
  No telemetry.
- **Fast or invisible.** The hook must cost nothing you can feel at the
  prompt.

## Next to the alternatives

| | trigger | any terminal | shells | providers | agent hand-off | open source |
|---|---|---|---|---|---|---|
| Warp | on failure | Warp only | any | Warp's | Warp's agent | no |
| thefuck | you type `fuck` | yes | any | none (rules) | no | yes |
| zsh-autofix | on failure | yes | zsh + oh-my-zsh | Ollama | no | yes |
| ai-fix | you type `ai-fix` | yes | zsh, bash, fish | Claude, OpenAI | no, re-runs the command | yes |
| butterfish | you ask | PTY wrapper | bash, zsh | OpenAI-compatible | its own | yes |
| **kintsu** | on failure | yes | zsh, bash, fish | any, incl. local | pluggable | MIT |

Notes on each, and on Copilot CLI, Amazon Q, Gemini CLI and friends, in
[docs/VISION.md](docs/VISION.md#what-exists-already).

## Try the skeleton

Requires a Rust toolchain ([rustup.rs](https://rustup.rs)).

```sh
git clone https://github.com/nathan-poncet/kintsu.git
cd kintsu
cargo install --path .
```

Then in your shell configuration:

```sh
eval "$(kintsu init zsh)"     # ~/.zshrc
eval "$(kintsu init bash)"    # ~/.bashrc
kintsu init fish | source     # ~/.config/fish/config.fish
```

Today this prints a one-line hint after a failed command, nothing more.
Remove the line to uninstall.

## Roadmap

1. **v0.1, the bubble**: the daemon and its socket, hooks, context
   capture, rule-based fixes, one provider layer (OpenAI-compatible,
   Anthropic, Ollama), `why`, hand-off to a CLI agent by template, config
   file, noise control.
2. **v0.2, it reads the output**: stderr/stdout capture through terminal
   and multiplexer APIs, redaction and "what leaves the machine" preview,
   ghost-text fixes, danger guard, project awareness.
3. **v0.3, your agent, your keys**: keychain storage, subscription reuse,
   local models as first-class citizens, cheap-model triage, an MCP server
   exposing the last failure to any agent.

Details, ideas parking lot and open questions:
[docs/VISION.md](docs/VISION.md).

## Website

The site lives in `docs/` as plain static files, the same convention as
whisk, and is not published yet. Preview it locally:

```sh
python3 -m http.server -d docs 8000
```

Then open <http://localhost:8000>. The page opens on a tour in nine
chapters, one per action of the bubble, with pause, chapter navigation and
a step-by-step mode; each chapter says who is doing the work, a rule, a
local model, a cloud model or your agent. Click the words in the bubble, or
press `Tab`, `^K`, `w`, `f`, `a`, `i`, `p` and `Esc` once the terminal has
focus, to take the wheel.

## Design documents

- [VISION.md](docs/VISION.md): the itch, the landscape, the roadmap.
- [ARCHITECTURE.md](docs/ARCHITECTURE.md): entities, use cases, adapters,
  the crate map and the Dependency Rule.
- [DAEMON.md](docs/DAEMON.md): the resident process, its socket protocol,
  how a bubble reaches a live shell.
- [MODELS.md](docs/MODELS.md): several models, routed per task.
- [UI.md](docs/UI.md): the bubble, toast and panel, keys and clicks.

## Why "kintsu"

Kintsugi (金継ぎ) is the Japanese art of repairing broken pottery with gold:
the crack becomes the most visible and the most beautiful part of the piece.
A failed command is a crack. kintsu is the gold.

## Contributing

Issues and pull requests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md)
for the conventions, [SECURITY.md](SECURITY.md) for anything sensitive, and
the [code of conduct](CODE_OF_CONDUCT.md).

## License

[MIT](LICENSE) © 2026 Nathan Poncet.
