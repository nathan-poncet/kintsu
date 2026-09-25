# kintsu

**A command just failed. kintsu is the small bubble under it that asks
"want me to look into that?" and hands the whole case to the AI agent of
your choice, right there in your terminal.**

[![CI](https://github.com/nathan-poncet/kintsu/actions/workflows/ci.yml/badge.svg)](https://github.com/nathan-poncet/kintsu/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-2ea44f)](LICENSE)
![Linux and macOS](https://img.shields.io/badge/platform-Linux%20%7C%20macOS-c084fc)
![Status: pre-alpha](https://img.shields.io/badge/status-pre--alpha-f59e0b)

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/S3V726AT7H)

> **Status: pre-alpha.** The bubble, the rule-based fixes, `why` with your
> model, the hand-off to a CLI agent, ignore and mute, privacy preview,
> doctor, and the resident daemon that delivers a model's answer above
> your prompt while you keep working: all of it runs today. What was
> decided on the way is in [docs/DECISIONS.md](docs/DECISIONS.md); the
> plan in [docs/VISION.md](docs/VISION.md). Pushback is welcome in the issues.

## The itch

You type a command. It fails. You squint at the error, copy it, switch to a
browser or a chat window, paste it, re-explain the context your shell
already knew, read the answer, come back, retype. [Warp](https://www.warp.dev)
made that loop disappear, but only inside Warp, with Warp's account and
Warp's AI.

kintsu wants the same loop in *your* terminal and *your* shell, with *your*
model and *your* key.

## How it works

```
$ gti status
zsh: command not found: gti
▎ Did you mean git status?
▎ ^K to insert · kintsu why · kintsu agent · kintsu ignore
$ git status
```

A small bubble, one gold seam on the left, one sentence, a line of
actions. A confident, harmless fix is already on your next prompt, dim:
Tab takes it, you press Enter. Otherwise `^K` puts the fix in your prompt. Nothing ever
steals a keystroke from your prompt, and nothing runs on its own. When no
rule knows and a local model is routed for quick fixes (or `eager_fix =
true` for a remote one), a resident daemon asks it in the background; the
bubble says "asking local…" and the answer takes that line's place a
moment later, without interrupting what you type. Clickable words and the
expanded panel come next.

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

One line. The installer detects your platform, takes the latest release
or, when there is none, builds from source with cargo, puts the binary in
`~/.local/bin`, asks before adding the hook to your shell, offers to set
up a local model (Ollama with `qwen2.5-coder:7b`) so fixes and
explanations never leave your machine, and writes a default configuration
if you have none:

```sh
curl -fsSL https://nathan-poncet.github.io/kintsu/install.sh | sh
```

`--dir`, `--no-hook`, `--no-model`, `--yes`, `--dry-run` and `--uninstall`
do what they say; the script is [200 lines of plain sh](install.sh), read it
first if you like. By hand, with a Rust toolchain ([rustup.rs](https://rustup.rs)):

```sh
cargo install --git https://github.com/nathan-poncet/kintsu
```

Then, if you skipped the hook, in your shell configuration:

```sh
eval "$(kintsu init zsh)"     # ~/.zshrc
eval "$(kintsu init bash)"    # ~/.bashrc
kintsu init fish | source     # ~/.config/fish/config.fish
```

Remove the line to uninstall. With no configuration file, the rules work
and nothing leaves your machine. The default configuration routes the
local model for fixes and explanations; to add a cloud model or an agent:

```sh
kintsu default-config > ~/.config/kintsu/config.toml   # then edit it
kintsu doctor                                          # checks the hook, the models, the keys
```

The commands, all about the last failure of the current shell:

| command | does |
|---|---|
| `kintsu fix` | the corrected command, from a rule or your quick-fix model; `--raw` is what `^K` uses |
| `kintsu why` | an explanation from the first configured model that answers, delivered above your prompt while you keep working; a case holding a secret only reaches local models |
| `kintsu agent [--with name] [words…]` | writes the brief and launches your CLI agent (Claude Code, Codex, OpenCode, aider, Gemini CLI, Copilot CLI…) |
| `kintsu privacy` | exactly what a model or an agent would receive, secrets masked |
| `kintsu ignore [--command\|--dir\|--session\|--always] [program]` | quiet for that command line, or that program here / in this shell / everywhere |
| `kintsu mute [1h]` | nothing for a while |
| `kintsu setup`, `doctor`, `default-config`, `config path` | configure in three questions, check, print the defaults, show where the files are |
| `kintsu daemon status`, `daemon stop` | the resident process; the hooks start it on their own |

## Roadmap

1. **v0.1, the bubble**: the daemon and its socket, hooks, rule-based
   fixes, one provider layer (OpenAI-compatible, Anthropic, Ollama),
   `why`, hand-off to a CLI agent by template, config file, noise
   control. Built; what differs from the design documents is in
   [docs/DECISIONS.md](docs/DECISIONS.md).
2. **v0.2, it reads the output**: output capture through terminal and
   multiplexer APIs (Herdr, tmux, WezTerm, Kitty, iTerm2: built), ghost-text
   fixes, clickable words, the panel, project awareness.
3. **v0.3, your agent, your keys**: keychain storage, subscription reuse,
   local models as first-class citizens, cheap-model triage, an MCP server
   exposing the last failure to any agent.

Details, ideas parking lot and open questions:
[docs/VISION.md](docs/VISION.md).

## Website

<https://nathan-poncet.github.io/kintsu/>, served by GitHub Pages from
`docs/` as plain static files. Preview it locally:

```sh
python3 -m http.server -d docs 8000
```

Then open <http://localhost:8000>. The landing page says the minimum: what
Kintsu is, a live terminal you can click into, five strengths, one line to
install. Everything else is documentation, one page per subject: `docs.html` the
hub, `install.html` every install method, `keys.html` the keys and
commands, `configuration.html` the configuration reference, `faq.html` the
questions, `roadmap.html` a one-screen timeline.

## Design documents

- [VISION.md](docs/VISION.md): the itch, the landscape, the roadmap.
- [ARCHITECTURE.md](docs/ARCHITECTURE.md): entities, use cases, adapters,
  the crate map and the Dependency Rule.
- [DAEMON.md](docs/DAEMON.md): the resident process, its socket protocol,
  how a bubble reaches a live shell.
- [MODELS.md](docs/MODELS.md): several models, routed per task.
- [UI.md](docs/UI.md): the bubble, toast and panel, keys and clicks.
- [DECISIONS.md](docs/DECISIONS.md): what v0.1 built, what it left out, and why.

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
