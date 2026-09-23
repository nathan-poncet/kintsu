# The daemon

Kintsu is resident. One process per user, started once, alive across
every shell and every terminal window, whatever the shell. The hooks and
the `kintsu` commands are thin clients that talk to it over a Unix socket.

## Why a resident process

- **One brain for every shell.** A failure in a fish tab and a fix in a
  zsh pane are the same conversation; sessions come and go, the state
  stays.
- **The prompt never waits.** Anything slower than a rule runs after the
  hook has returned, and its result arrives later as a message.
- **Warm state.** Config parsed once, SQLite open, HTTP connections kept
  alive, the local model's server already up, the keychain read once.
- **Memory across time.** "You hit this yesterday and `nvm use 22` fixed
  it" needs somewhere to remember.
- **Messages can arrive.** The UI the project wants, bubbles landing in
  the terminal while you work, is only possible if something is awake to
  send them.

## One binary, two roles

| role | started by | lives | does |
|---|---|---|---|
| `kintsu` (client) | the hooks, the user, a click | milliseconds | parses, sends one frame, prints one answer |
| `kintsu daemon` | the client on demand, or launchd / systemd | until logout or `kintsu daemon stop` | everything else |

The client connects to the socket; if nothing answers it spawns
`kintsu daemon` detached (`setsid`, stdio to the log) and retries once.
That is enough for most people: the first failure of the day starts the
daemon. `kintsu service install` makes it permanent and restart-on-crash:
a LaunchAgent (`~/Library/LaunchAgents/dev.kintsu.daemon.plist`,
`KeepAlive`) on macOS, a user unit (`~/.config/systemd/user/kintsu.service`,
socket-activated when available) on Linux. `kintsu service uninstall`
removes it.

Single instance: a lock file next to the socket. Version handshake: the
client's `hello` carries its version; a daemon older than its client
answers `outdated`, the client asks it to drain and exit, then respawns
it. Upgrading Kintsu never leaves a stale daemon around.

## Paths

| what | where |
|---|---|
| socket | `$XDG_RUNTIME_DIR/kintsu/daemon.sock`, else `$TMPDIR/kintsu-$UID/daemon.sock` (macOS), else `~/.local/state/kintsu/daemon.sock` |
| lock, pid | next to the socket |
| state | `~/.local/state/kintsu/` : `kintsu.db` (SQLite), `daemon.log` |
| config | `~/.config/kintsu/config.toml`, plus `.kintsu.toml` per project |
| secrets | the OS keychain, or the sources named in config (`env`, `command`) |

Directories are `0700`, the socket `0600`, and the daemon checks the
peer's uid on every connection (`SO_PEERCRED` / `LOCAL_PEERCRED`). No TCP
listener, ever, unless the user configures the optional MCP endpoint, and
that one binds to loopback with a token.

## Sessions

A session is one interactive shell. The hook obtains an id at shell
startup (`kintsu session new`, a random token kept in a shell variable) so
that `exec`, subshells and forked terminals do not confuse the daemon.
With the id, the hook sends what it knows: shell and version, pid, tty,
and the terminal identity the notifiers and output sources need
(`TERM_PROGRAM`, `TMUX_PANE`, `HERDR_PANE`, `WEZTERM_PANE`,
`KITTY_WINDOW_ID`, `ITERM_SESSION_ID`).

Per session the daemon keeps a ring of the last commands with their
status and duration, the current directory, the last case, the pending
bubbles, and the subscriber connection if the shell has one.

## Protocol

Newline-delimited JSON over the socket, one object per line, `"v": 1` on
every frame. Debuggable with `nc -U`; a binary framing can replace it later
without touching the use cases, the controller is the only reader.

Client to daemon:

| type | fields | answer |
|---|---|---|
| `hello` | `version`, `session` (optional) | `welcome` or `outdated` |
| `session_new` | shell, pid, tty, terminal identity | `session` (id) |
| `command_started` | `session`, `command`, `cwd` | none |
| `command_finished` | `session`, `command`, `status`, `pipestatus`, `duration_ms`, `cwd` | `decision` within the sync budget, else `later` |
| `subscribe` | `session` | a stream of `bubble` frames until the connection closes |
| `pending` | `session` | the `bubble` frames not yet delivered (bash, or after a reconnect) |
| `act` | `case`, `action` (`fix`, `why`, `agent`, `ignore`, `privacy`, `dismiss`), `origin` (`key`, `click`, `command`) | `stream` frames, then `done` |
| `get_case` | `case` or `session` | the case, redacted view included |
| `shutdown` | | `bye` |

Daemon to client:

| type | fields |
|---|---|
| `decision` | `quiet` with a reason, or `offer` with the case id, the toast view state and, when a rule knew, the `fix` |
| `bubble` | `case`, `sender`, view state, `replaces` (to update in place) |
| `stream` | `case`, `chunk` |
| `done`, `ack`, `error` | |

The sync budget on `command_finished` is 40 ms by default. If the daemon
answers in time, the client prints the toast right under the command's
output, which is where it looks best. If not, the client returns and the
toast arrives as a bubble.

## Delivering a bubble into a live shell

The hard part of "messages arriving": something must draw above a prompt
the user may be typing on, without corrupting the line.

| shell | mechanism | latency |
|---|---|---|
| zsh | the hook starts `kintsu subscribe --session <id>` in the background and watches its stdout with `zle -F`; on a frame it calls `zle -I`, prints the bubble, and `zle reset-prompt` redraws the line intact | immediate |
| fish | the subscriber sends `SIGUSR1` to the shell; a `--on-signal SIGUSR1` function pulls `kintsu pending` and prints, then `commandline -f repaint` | immediate |
| bash | no safe way to draw while readline waits; bubbles are flushed at the next prompt, or on demand with a `bind -x` hotkey | next prompt |

Ghost-text fixes use the same paths: zsh's `POSTDISPLAY` region, fish's
`commandline` with an autosuggestion-style dim tail, nothing in bash.
Inserting an accepted fix into the line editor uses `print -z` in zsh,
`commandline -r` in fish and `READLINE_LINE` in bash, so the user always
presses Enter themselves.

## Time and resource budgets

| path | budget |
|---|---|
| hook overhead on success | under 1 ms (a variable assignment, no process) |
| `kintsu triage` client, quiet answer | under 5 ms including process start |
| daemon rule verdict | under 2 ms |
| daemon idle | no polling, under 30 MB resident, zero CPU |
| local model | never in-process: Ollama, llama-server or LM Studio run it, the daemon only holds a connection |

## Failure modes

| what breaks | what the user sees |
|---|---|
| no daemon and it cannot start | nothing; the client logs once per hour |
| daemon hung | the sync budget expires, nothing is printed; the next client restarts it after a health probe fails |
| stale socket after a crash | the client removes it and respawns |
| subscriber died | zsh and fish fall back to next-prompt delivery until the hook restarts it |
| model unreachable | rule fixes still work; the toast says "no model reachable" once, then stays quiet about it |
| SSH | the daemon runs on the remote host, per user, like everything else |

Nothing in this table changes the exit status the prompt shows, and
nothing blocks the prompt.

## Security

- Local socket, `0600`, peer uid checked. No network listener by default.
- The daemon never runs a command. It returns text; the client inserts it
  in the line editor; the user presses Enter.
- `act` frames coming from a click (the URL-scheme handler) may only open
  or focus; they cannot start an agent or insert a command. A web page
  containing `kintsu://` links can at most pop the panel.
- Case ids are random 128-bit tokens and expire with the case.
- Keys stay in the daemon's memory, read from the keychain once, never
  logged, never in the protocol.
