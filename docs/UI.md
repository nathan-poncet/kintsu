# The bubble

*Status (2026-09-25): the toast, ghost text, the clickable words and the
panel are built as described, with the differences recorded in
[DECISIONS.md](DECISIONS.md) sections 18 and 20 to 23: a click answers in
the shell rather than opening the panel, by decision; the panel opens in
the bubble's place, only while the failure is the shell's last command,
remembers the answers it got and puts the bubble back on close; its height
is fixed when it opens and `ui.hotkey` is not yet read (`^K`), both to
come.*

Kintsu talks in small messages that land in the terminal, like a quiet
colleague leaning over: one line when it has something, silence
otherwise. Keyboard first, clickable for everyone else, and as plain as a
terminal allows.

## Two states of one thing

| | toast | panel |
|---|---|---|
| what | the bubble, collapsed: one to three lines printed in the flow | the same bubble, expanded in place: an inline viewport under the prompt |
| when | right after a failure, or later as a message | on `^K`, on a click, on `kintsu` |
| input | none stolen: shell keybindings and links only | full: single keys, Tab focus, mouse, scroll |
| drawn by | plain ANSI text with OSC 8 links, written into the scrollback | ratatui in an inline viewport, 6 to 14 lines, never full screen |

The toast never takes a keystroke away from the prompt. Everything it
offers is reachable three ways: a key the shell hook bound, a click on the
word, or a command (`kintsu why`, `k w`).

## Anatomy

```
$ gti status
zsh: command not found: gti
▎ Did you mean git status?
▎ Tab to fix · Why · Agent · Ignore · ^K more
$ git status
```

- **The seam.** A single gold bar `▎` on the left of every line Kintsu
  writes. It is the only decoration and the only colour. Gold for the
  kintsugi, and because it reads on dark and light backgrounds alike.
- **One sentence.** What Kintsu thinks, in the user's language, at most
  two lines. No "error detected", no "AI assistant"; the sentence a person
  would say.
- **The actions line.** Words, not buttons. The first is the one a key
  does right now; the others are links. `^K more` is always last and
  always the same.
- **Ghost text.** When a fix is confident and harmless, the corrected
  command is already on the next prompt, dim. `Tab` or `→` accepts it;
  typing anything else discards it; Enter runs it, and only Enter.

Widths: the toast wraps at the terminal width, capped at 80 columns; the
command echo is abbreviated at 60 characters.

## States

**No fix known.**

```
$ npm run build
… forty lines of a bundler stack trace …
▎ Build failed after 12 s. Want a look?
▎ Why · Fix · Agent · Ignore · ^K more
$
```

**The panel, after `^K` or a click.**

```
▎ npm run build · exit 1 · 12 s                                            esc
▎
▎ Why w   Fix f   Agent a   Ignore i   Privacy p
▎
▎ Module not found: 'node:sqlite' needs Node 22.5 or later; this shell runs
▎ 20.11. nvm has 22.12 installed.
▎
▎ ▸ nvm use 22 && npm run build                            Insert ⏎ · Copy c
```

The section names are the same words as the toast's links; the focused
one is underlined, the active one bold. Content streams in under them.
`⏎` on a suggestion inserts it in the shell's line editor and closes the
panel; the user presses Enter again to run it. Two Enters, on purpose.

**A message arriving while you work.**

```
$ vim src/build.js
$
▎ While you were away: the build failure is a Node version mismatch, not
▎ your code. ^K to see the fix.
$
```

Delivered above the prompt in zsh and fish, at the next prompt in bash.
The prompt line is redrawn intact.

**Hand-off to an agent.**

```
▎ Handing this to Claude Code: command, output (redacted: 1 token), cwd,
▎ git state, last 10 commands. ⏎ to start · p to review what is sent
```

**Nothing configured.**

```
▎ exit 127 · Tab to fix from rules · kintsu setup to add a model
```

Said once per day, then the rules do their job silently.

**A fix that worked.**

```
▎ Fixed. Next time gti will be corrected without asking.
```

Only shown the first time a rule is learned from an accepted fix, and only
if `ui.learning_notes` is on.

## Keys

| where | key | does |
|---|---|---|
| at the prompt, empty line | `Tab` or `→` | accept the ghost-text fix |
| at the prompt | `^K` | expand the last bubble into the panel, or collapse it |
| anywhere | `kintsu why` `fix` `agent` `ignore` `privacy`, alias `k` | the same actions by command |
| panel | `w` `f` `a` `i` `p` | switch section or act |
| panel | `⏎` | insert the focused suggestion in the line editor and close |
| panel | `c` | copy the focused suggestion |
| panel | `Tab` / `Shift-Tab`, `←` `→` | move focus between actions |
| panel | `↑` `↓`, `PgUp` `PgDn`, mouse wheel | scroll content |
| panel | `esc`, `q` | collapse |
| panel | `?` | the key map, in place |

`^K` is the default hotkey and is configurable (`ui.hotkey`); in zsh and
fish it is a widget the hook binds, in bash a `bind -x`.

## Mouse

Clicking works in two different ways, and both keep the keyboard promise.

- **In the toast**, every action word is an OSC 8 hyperlink to
  `kintsu://act?case=<token>&do=<action>`. Terminals that render
  hyperlinks make the word clickable with no mouse tracking at all, so
  text selection in the terminal keeps working. `kintsu service install`
  registers the scheme handler: a small app bundle on macOS, a `.desktop`
  entry with `x-scheme-handler/kintsu` on Linux. The handler (`kintsu
  open <url>`) forwards the URL to the daemon, which answers in the shell
  the failure happened in: `why` explains, `fix` sends the fix as a
  bubble, `ignore` silences that command line; once the panel exists, a
  click opens it on that action instead. A click can never run a command
  or start an agent by itself, so `agent` and `privacy` answer with a
  note pointing at the command.
- **In the panel**, ratatui enables mouse tracking while the panel is
  open and releases it on close: click an action, a suggestion, a link in
  the explanation; wheel to scroll.

| terminal | toast links | notes |
|---|---|---|
| iTerm2, WezTerm, Kitty, Ghostty, foot, VS Code, Windows Terminal, GNOME Terminal and other VTE | yes | Kitty may need `open_url_with` to accept custom schemes |
| Alacritty | via hints | configure a `kintsu://` hint or use the keys |
| Terminal.app | no | keys and commands only; the words still render |

## Voice and visual language

- **One accent.** Gold: `#D9A441` in truecolor, 179 in 256 colours,
  yellow in 16. `ui.accent` overrides it. Red appears only on a danger
  confirmation, green never.
- **No boxes, no fills, no icons** in the default variant. `ui.style =
  "boxed"` draws rounded borders for those who want them; `ui.ascii = true`
  swaps `▎ ⏎ → …` for `| Enter -> ...`.
- **Sentences, not labels.** "Did you mean git status?" not "Suggestion:".
  The user's language, from `ui.language` or the locale.
- **Never a wall.** A toast is three lines at most; long explanations live
  in the panel and stream.
- **Novice mode without a novice mode.** The words are the buttons. The
  first three bubbles ever shown add one dim line explaining `Tab` and
  `^K`; `ui.hints = "verbose"` keeps that line.

## Noise

| rule | default |
|---|---|
| the same failure twice in a row | one bubble |
| a dismissed case | quiet for that command for 10 minutes |
| `kintsu ignore` | scopes: this command, this directory, this session, always |
| `kintsu mute 1h` | nothing at all for an hour |
| commands that are never triaged | editors, pagers, `ssh`, `top`, `watch`, `man`, anything interactive, configurable |
| exit statuses that are never failures | 130, 141, 148, plus the user's list |
| async messages per case | coalesced, at most one every 10 s while collapsed |
| `ui.mode` | `toast` (default) · `hint` (one dim line, no links) · `panel` (always expand) · `silent` (hotkey only) |

## Accessibility

- Every action has a key and a command; the mouse is never required.
- `NO_COLOR` is honoured; nothing carries meaning by colour alone.
- `ui.screen_reader = true` drops columns and glyphs for full sentences,
  one per line, and announces arrivals with a bell if the user asks.
- The panel queries the background colour (OSC 11) on open and picks the
  accent shade that contrasts.
