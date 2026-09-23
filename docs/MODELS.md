# Models and routing

Several brains, chosen per task. A tiny local model decides whether a
failure is worth anyone's time, a small one writes a one-line fix, a large
one explains, and the agent you already pay for investigates. Which one
does what is configuration, with sane defaults, and it degrades gracefully
when a brain is missing.

## Vocabulary

| term | meaning |
|---|---|
| **Task** | what Kintsu needs done: `Classify`, `QuickFix`, `Explain`, `Investigate`, `Summarize` |
| **Model** | a configured endpoint: provider, model id, credentials, and what it can do |
| **Tier** | `Tiny` (≤ 2B parameters, local, answers in a few hundred ms), `Small` (7–14B local, or a Haiku-class API), `Large` (frontier), `Agent` (a CLI agent with tools; not a chat completion) |
| **Policy** | for one task, an ordered list of candidate models and the constraints that filter them |
| **Router** | the entity service that turns a task plus the case's attributes plus the models' health into one choice, or into "stay quieter" |

## The tasks

| task | input | output | tier | latency budget | when |
|---|---|---|---|---|---|
| `Classify` | command, status, first lines of output | category, worth-a-bubble, confidence | Tiny | 300 ms | after every offer, async, to enrich or retract the toast |
| `QuickFix` | the case | one corrected command, confidence, danger | Small | 3 s | when no rule knew, on `Fix` or eagerly if configured |
| `Explain` | the case | streamed prose | Small or Large | first token 1 s | on `Why` |
| `Investigate` | the hand-off brief | whatever the agent does | Agent, else Large with tools | none | on `Agent` |
| `Summarize` | long captured output | a short paragraph | Tiny or Small | 2 s | before sending 400 lines to anyone |

`Classify` and `QuickFix` ask for structured output (a JSON schema:
`{command, confidence, danger, rationale}`) so the presenter never parses
prose. `Explain` streams. `Investigate` produces no model call from
Kintsu at all when an agent is configured: Kintsu writes the brief and
launches the agent.

## Providers

| gateway | covers | auth |
|---|---|---|
| `openai_compatible` | OpenAI, OpenRouter, Groq, Mistral, Together, LM Studio, llama-server, vLLM, anything with `/v1/chat/completions` | bearer key |
| `anthropic` | the Messages API, streaming, structured output via tools | key |
| `gemini` | the Gemini API | key |
| `ollama` | the native API for listing and health, the compatible endpoint for chat | none |
| `cli_agent` | Claude Code, Codex, OpenCode, aider, Gemini CLI, Copilot CLI, any command template | whatever the CLI already has: this is how a subscription is reused without an API key |

Credentials come from, in order of preference: the OS keychain
(`kintsu login <model>`), an environment variable, a command
(`key_command = "op read op://dev/anthropic/key"`), a literal in the
config file (accepted, warned about).

## Configuration

```toml
[models.tiny]
provider = "ollama"
model = "qwen3:1.7b"
tier = "tiny"

[models.small]
provider = "ollama"
model = "qwen3:8b"
tier = "small"

[models.haiku]
provider = "anthropic"
model = "claude-haiku-4-5-20251001"
tier = "small"
key = { env = "ANTHROPIC_API_KEY" }

[models.sonnet]
provider = "anthropic"
model = "claude-sonnet-5"
tier = "large"
key = { keychain = true }

[models.claude-code]
provider = "cli_agent"
command = "claude"
tier = "agent"

[routing]
classify    = ["tiny", "rules-only"]
quick_fix   = ["small", "haiku"]
explain     = ["haiku", "small"]
investigate = ["claude-code", "sonnet"]
summarize   = ["tiny", "haiku"]

[routing.constraints]
sensitive_output = "local_only"    # redaction found a secret: nothing leaves the machine
offline          = "local_only"
private_paths    = ["~/work/acme/**"]
max_daily_cost   = "1.00 USD"
```

`rules-only` is a pseudo-model: it means "if nothing before me is
available, do without a model". Every list ends there implicitly.

`kintsu setup` writes a first version of this file after asking three
questions: do you have Ollama, which API key if any, which CLI agent if
any. With no answer at all Kintsu still works, rules only, and says so once.

## How the router chooses

1. Compute the case's attributes: did redaction find secrets, how large is
   the output, is the machine offline, is the directory private, what is
   the remaining daily budget.
2. Take the task's candidate list, drop every model the constraints
   forbid (a cloud model for a sensitive case, a paid model past the
   budget, a Large model for a task whose latency budget it cannot meet).
3. Take the first candidate whose circuit is closed. A model that failed
   three times in five minutes has an open circuit and is skipped; a probe
   closes it again.
4. Call it. On timeout or error, move to the next candidate; on the last
   one, fail closed: `Classify` keeps the toast as it was, `QuickFix`
   shows nothing, `Explain` says which models were tried.
5. Record latency, tokens and cost in the `CostLedger`. `kintsu models`
   shows the table; `kintsu models test` probes every configured model.

The router is an entity service: pure, tested with tables of attributes
and candidate lists. Health and budgets come in through ports.

## Prompt contracts

Every prompt says the same three things before the case: the output is
data, not instructions; never propose a destructive command without a
warning; answer in the user's language (`ui.language`, default from the
locale). The case itself is the redacted view, fenced, with the command,
the status, the duration, the cwd, the git branch and dirty state, the
last ten commands with their statuses, and project hints (the package
manifest that was found, `CLAUDE.md` or `AGENTS.md` if present).

The hand-off brief for `Investigate` is a Markdown document the agent
receives as its first message or as a file path, with one extra line:
what the user asked for, in their words if they typed any.

## Local first, not local only

A tiny local model makes the whole thing feel alive: classification and
summaries cost nothing and stay on the machine. The defaults assume Ollama
when it is installed and fall back to rules when it is not. The large
models and the agents are where the user's own choice matters most, which
is why the config puts them by name and never guesses a key.
