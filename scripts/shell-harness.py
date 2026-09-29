#!/usr/bin/env python3
"""Drives real zsh and fish sessions in a pseudo-terminal, against a fake
Ollama server, and prints what a screen emulator shows afterwards. This is
how the hooks' drawing is checked: the bubble, the "asking…" line, and the
message that replaces it above the prompt while text is being typed.

Needs `pyte` (pip install pyte), zsh and fish on the PATH, and a built
binary (cargo build). Run it after touching shell/*; CI runs `--check`.

    python3 scripts/shell-harness.py            # fish scenarios
    python3 scripts/shell-harness.py zsh        # zsh scenarios
    python3 scripts/shell-harness.py why        # kintsu why in both shells
    python3 scripts/shell-harness.py late       # the answer lands after another command
    python3 scripts/shell-harness.py ghost      # a typo, the pre-typed fix, Tab, Enter
    python3 scripts/shell-harness.py panel      # ^K, the panel, Enter, w, Esc (fish, zsh, bash)
    python3 scripts/shell-harness.py tee        # capture.stderr_tee: a refused touch, the sudo fix, kintsu privacy (zsh, bash)
    DELAY=1.5 …                                 # slow the fake model down

Environment: BIN (directory of the kintsu binary, default target/debug),
ROOT (scratch directory, default /tmp/kintsu-harness; keep it short, it
holds a Unix socket), PYLIB (extra sys.path entry for pyte).

    python3 scripts/shell-harness.py --check            # every scenario, asserted; CI runs this
    python3 scripts/shell-harness.py --check ghost panel

In check mode nothing is printed unless an expectation fails; then the
screen is dumped and the exit status is 1. The daemon it stops between
scenarios is its own, on ROOT's socket, never the user's.
"""
import os, pty, select, time, re, json, threading, socketserver, http.server, sys, shutil
if os.environ.get("PYLIB"):
    sys.path.insert(0, os.environ["PYLIB"])
import pyte

ROOT = os.environ.get("ROOT", "/tmp/kintsu-harness")
SLOW = float(os.environ.get("SLOW", "1"))   # stretches every wait: CI runners are slower than a laptop
BIN = os.path.abspath(os.environ.get("BIN", os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "target", "debug")))
STREAMED = ["Node 20 is too old for this build.\n", "The bundler needs node:sqlite, ", "which arrived in Node 22.5.\n",
            "nvm has 22.12 installed.\n", "Switch to it, then build again.\n", "Nothing in your code is wrong.\n",
            "The lockfile is fine too.\n", "That is all.\n"]
CALLS = {"n": 0}   # how often the fake model was asked, for "nobody is asked"
class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"   # chunked answers need it; every answer still closes the connection
    def do_POST(self):
        CALLS["n"] += 1
        n = int(self.headers.get("content-length", 0)); asked = self.rfile.read(n)
        time.sleep(float(os.environ.get("DELAY", "0")))
        self.close_connection = True
        if b'"stream":true' in asked.replace(b" ", b""):
            # The panel asks for the answer as it comes: eight pieces, a
            # fifth of a second apart, so the screen shows it growing.
            self.send_response(200); self.send_header("content-type", "application/x-ndjson"); self.send_header("transfer-encoding", "chunked"); self.send_header("connection", "close"); self.end_headers()
            for piece in STREAMED + [None]:
                line = json.dumps({"message": {"role": "assistant", "content": piece or ""}, "done": piece is None}).encode() + b"\n"
                self.wfile.write(f"{len(line):x}\r\n".encode() + line + b"\r\n"); self.wfile.flush()
                if piece is not None: time.sleep(0.2)
            self.wfile.write(b"0\r\n\r\n"); self.wfile.flush()
            return
        body = json.dumps({"message": {"role": "assistant", "content": "nvm use 22 && npm run build"}, "done": True}).encode()
        self.send_response(200); self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(body))); self.send_header("connection", "close"); self.end_headers(); self.wfile.write(body)
    def log_message(self, *a): pass
srv = socketserver.TCPServer(("127.0.0.1", 0), H); port = srv.server_address[1]
threading.Thread(target=srv.serve_forever, daemon=True).start()
os.makedirs(ROOT, exist_ok=True)
TEE = sys.argv[1:] == ["tee"]
capture = '[capture]\nstderr_tee = true\n' if TEE else ''
open(f"{ROOT}/config.toml", "w").write(f'[models.local]\nprovider = "ollama"\nmodel = "m"\nbase_url = "http://127.0.0.1:{port}"\n[routing]\nquick_fix = ["local"]\nexplain = ["local"]\n{capture}[ui]\neager_fix = true\n')
# A small, known PATH: the binary under test, a fake `git` so `gti` has one
# unambiguous neighbour, and the system directories.
os.makedirs(f"{ROOT}/bin", exist_ok=True)
with open(f"{ROOT}/bin/git", "w") as fake:
    fake.write("#!/bin/sh\necho 'On branch main (fake git)'\n")
os.chmod(f"{ROOT}/bin/git", 0o755)
SHELL_PATH = f"{BIN}:{ROOT}/bin:/usr/bin:/bin"
env = dict(os.environ, KINTSU_SOCKET=f"{ROOT}/d.sock", KINTSU_STATE_DIR=f"{ROOT}/state", KINTSU_CONFIG=f"{ROOT}/config.toml",
           PATH=SHELL_PATH, KINTSU_DEBUG="1", TERM="xterm-256color", NO_COLOR="1", LINES="24", COLUMNS="80")
env.pop("KINTSU_SESSION", None)

def respond(fd, chunk, screen=None):
    # answer the terminal queries fish 4 sends, so it shows a prompt
    if b"\x1b[c" in chunk or b"\x1b[0c" in chunk: os.write(fd, b"\x1b[?62;1;2;6;9;15;22c")
    if b"\x1b[>0q" in chunk or b"\x1b[>q" in chunk: os.write(fd, b"\x1bP>|pyte(0)\x1b\\")
    if b"\x1b]11;?" in chunk: os.write(fd, b"\x1b]11;rgb:0000/0000/0000\x1b\\")
    for m in re.finditer(rb"\x1bP\+q([0-9a-fA-F]+)\x1b\\", chunk): os.write(fd, b"\x1bP0+r" + m.group(1) + b"\x1b\\")
    if b"\x1b[?u" in chunk: os.write(fd, b"\x1b[?0u")
    if b"\x1b[6n" in chunk:
        row, col = (screen.cursor.y + 1, screen.cursor.x + 1) if screen else (1, 1)
        os.write(fd, f"\x1b[{row};{col}R".encode())

def fresh_start():
    """Stops the daemon of a previous scenario, the one listening on this
    harness's own socket and no other, and clears its state."""
    import subprocess
    subprocess.run([f"{BIN}/kintsu", "daemon", "stop"], env=env, capture_output=True); time.sleep(0.3 * SLOW)
    shutil.rmtree(f"{ROOT}/state", ignore_errors=True)
    for f in ("d.sock", "d.spawn"):
        try: os.remove(f"{ROOT}/{f}")
        except FileNotFoundError: pass

def run(variant_fn, typed=None, wait=4.0, enter_first=False, why=False, ghost=False, panel=False, record=None):
    fresh_start()
    pid, fd = pty.fork()
    if pid == 0:
        os.execve(shutil.which("fish") or "fish", ["fish", "-N", "-i"], env)
    import fcntl, termios, struct
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    screen = pyte.Screen(80, 24); stream = pyte.ByteStream(screen)
    raw = b""
    def drain(t):
        nonlocal raw
        end = time.time() + t * SLOW
        while time.time() < end:
            r, _, _ = select.select([fd], [], [], 0.05)
            if r:
                try: chunk = os.read(fd, 65536)
                except OSError: return
                raw += chunk; stream.feed(chunk); respond(fd, chunk, screen)
    def send(s, t): os.write(fd, s.encode()); drain(t)
    drain(1.5)
    send(f"set -gx PATH {BIN} {ROOT}/bin /usr/bin /bin\n", 0.5)
    send("function fish_prompt; printf '\\n~\\n❯ '; end\n", 0.5)
    send("kintsu init fish | source\n", 0.8)
    if variant_fn: send(variant_fn + "\n", 0.5)
    send("clear\n", 0.5)
    send("true\n", 1.5)       # starts the daemon, out of the way of the timing
    send("clear\n", 0.5)
    if ghost:
        send("gti status\n", 1.0)          # a rule fix: the next prompt pre-types it (fish: Tab takes it)
        snapshot = [l.rstrip() for l in screen.display if l.strip()]
        send("\t", 0.5)                     # accept
        accepted = [l.rstrip() for l in screen.display if l.strip()]
        send("\n", 0.8)                     # run it
        if record is not None:
            record["before Tab"] = snapshot; record["after Tab"] = accepted
        else:
            print("   after the typo:", " | ".join(snapshot[-4:]))
            print("   before Tab:", snapshot[-1] if snapshot else "")
            print("   after Tab: ", accepted[-1] if accepted else "")
    if panel:
        panel_steps(send, screen, "fish", record)
        os.write(fd, b"kintsu daemon stop\n"); drain(0.6)
        os.write(fd, b"exit\n"); drain(0.4)
        try: os.close(fd)
        except OSError: pass
        return screen, raw
    send("false\n", 0.8)
    if enter_first: send("true\n", 0.3)    # another command, so a new prompt precedes the answer
    if typed: send(typed, 0.3)          # typed, not executed
    drain(wait)
    if why:
        send("clear\n", 0.5)
        send("kintsu why\n", 0.8)
        drain(wait)
    send("\n" if typed else "", 0.3)
    os.write(fd, b"kintsu daemon stop\n"); drain(0.6)
    os.write(fd, b"exit\n"); drain(0.4)
    try: os.close(fd)
    except OSError: pass
    return screen, raw

def show(title, screen):
    print(f"===== {title}")
    try:
        print("   daemon.log: " + " | ".join(open(f"{ROOT}/state/daemon.log").read().splitlines()[-3:]))
    except FileNotFoundError:
        print("   daemon.log: none")
    for i, line in enumerate(screen.display):
        if line.strip(): print(f"{i:2}| {line.rstrip()}")
    print(f"   cursor at row {screen.cursor.y}, col {screen.cursor.x}")

variants = {
 "current hook": None,
 "newline first": '''function __kintsu_on_message --on-signal SIGUSR1
    echo
    command kintsu pending --session "$KINTSU_SESSION"
    commandline -f repaint
end''',
 "compensated": '''function __kintsu_on_message --on-signal SIGUSR1
    set -l text (command kintsu pending --session "$KINTSU_SESSION" 2>&1 | string collect)
    test -n "$text"; or return
    set -l height (fish_prompt 2>/dev/null | string collect | string split \\n | count)
    set -l up (math $height - 1)
    printf '\\e[%dA\\r\\e[J' $up
    printf '%s\\n' $text
    for i in (seq $up); printf '\\n'; end
    commandline -f repaint
end''',
}
def run_zsh(typed=None, wait=4.0, enter_first=False, why=False, ghost=False, panel=False, tee=False, record=None):
    fresh_start()
    zenv = dict(env); zenv.pop("ZDOTDIR", None)
    pid, fd = pty.fork()
    if pid == 0:
        os.execve(shutil.which("zsh") or "zsh", ["zsh", "-f", "-i"], zenv)
    import fcntl, termios, struct
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    screen = pyte.Screen(80, 24); stream = pyte.ByteStream(screen)
    def drain(t):
        end = time.time() + t * SLOW
        while time.time() < end:
            r, _, _ = select.select([fd], [], [], 0.05)
            if r:
                try: chunk = os.read(fd, 65536)
                except OSError: return
                stream.feed(chunk); respond(fd, chunk, screen)
    def send(s, t): os.write(fd, s.encode()); drain(t)
    drain(1.0)
    send(f"export PATH={SHELL_PATH}\n", 0.4)
    send("PROMPT=$'\\n~\\n❯ '\n", 0.4)
    send('eval "$(kintsu init zsh)"\n', 0.8)
    send("true\n", 1.5)
    send("clear\n", 0.5)
    if tee:
        send("touch /etc/kintsu-harness-denied\n", 1.5)   # stderr copied by the hook; the rule that reads it answers
        drain(2.5)
        send("kintsu privacy\n", 1.2)
        send("kintsu daemon stop\n", 0.6)
        send("exit\n", 0.4)
        try: os.close(fd)
        except OSError: pass
        return screen
    if ghost:
        send("gti status\n", 1.0)
        snapshot = [l.rstrip() for l in screen.display if l.strip()]
        send("\t", 0.5)
        accepted = [l.rstrip() for l in screen.display if l.strip()]
        send("\n", 0.8)
        if record is not None:
            record["before Tab"] = snapshot; record["after Tab"] = accepted
        else:
            print("   before Tab:", snapshot[-1] if snapshot else "")
            print("   after Tab: ", accepted[-1] if accepted else "")
    if panel:
        panel_steps(send, screen, "zsh", record)
        send("kintsu daemon stop\n", 0.6)
        send("exit\n", 0.4)
        try: os.close(fd)
        except OSError: pass
        return screen
    send("false\n", 0.8)
    if enter_first: send("true\n", 0.3)
    if typed: send(typed, 0.3)
    drain(wait)
    if why:
        send("clear\n", 0.5)
        send("kintsu why\n", 0.8)
        drain(wait)
    send("\n" if typed else "", 0.3)
    send("kintsu daemon stop\n", 0.6)
    send("exit\n", 0.4)
    try: os.close(fd)
    except OSError: pass
    return screen

def panel_steps(send, screen, shell, record=None):
    """^K on a typo (a rule fix), Enter takes it; ^K after a plain failure,
    w asks why, Esc closes; ^K again shows the same answer; ^K after a
    success opens nothing. Prints the screen at each step, or records it
    under the step's title when `record` is given."""
    def snap(title):
        if record is not None:
            record[title] = list(screen.display); record[title + " · model calls"] = CALLS["n"]
            return
        print(f"----- {shell}: {title}")
        for i, line in enumerate(screen.display):
            if line.strip(): print(f"{i:2}| {line.rstrip()}")
        print(f"   cursor at row {screen.cursor.y}, col {screen.cursor.x}")
    send("gti status\n", 1.2)
    snap("after the typo")
    send("\x0b", 1.5)
    snap("^K: the panel, on the fix")
    send("\r", 1.0)
    snap("Enter: the fix is in the line, the panel is gone")
    send("\x15", 0.4)
    send("false\n", 1.5)
    send("\x0b", 1.5)
    snap("^K after a plain failure: Why asks at once, the explanation streams in, the panel grows")
    send("w", 1.6)
    snap("w: why, from the model, whole")
    send("\x1b", 1.0)
    snap("Esc: closed")
    send("\x0b", 1.5)
    send("w", 0.8)
    snap("^K again, w: the explanation is remembered, nobody is asked")
    send("\x1b", 1.0)
    send("true\n", 1.0)
    send("\x0b", 1.5)
    snap("^K after a success: nothing opens")

def run_bash(tee=False, record=None):
    fresh_start()
    bash = os.environ.get("BASH_BIN") or shutil.which("bash") or "bash"
    pid, fd = pty.fork()
    if pid == 0:
        os.execve(bash, ["bash", "--norc", "--noprofile", "-i"], env)
    import fcntl, termios, struct
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    screen = pyte.Screen(80, 24); stream = pyte.ByteStream(screen)
    def drain(t):
        end = time.time() + t * SLOW
        while time.time() < end:
            r, _, _ = select.select([fd], [], [], 0.05)
            if r:
                try: chunk = os.read(fd, 65536)
                except OSError: return
                stream.feed(chunk); respond(fd, chunk, screen)
    def send(s, t): os.write(fd, s.encode()); drain(t)
    drain(1.0)
    send(f"export PATH={SHELL_PATH}\n", 0.4)
    send("PS1=$'\\n~\\n❯ '\n", 0.4)
    send('eval "$(kintsu init bash)"\n', 0.8)
    send("true\n", 1.5)
    send("clear\n", 0.5)
    if tee:
        send("touch /etc/kintsu-harness-denied\n", 1.5)   # the DEBUG trap started the tee before touch
        drain(2.0)
        send("true\n", 1.5)                                 # bash hears the message with its next decision
        send("kintsu privacy\n", 1.2)
        send("kintsu daemon stop\n", 0.6)
        send("exit\n", 0.4)
        try: os.close(fd)
        except OSError: pass
        return screen
    panel_steps(send, screen, f"bash ({bash})", record)
    send("kintsu daemon stop\n", 0.6)
    send("exit\n", 0.4)
    try: os.close(fd)
    except OSError: pass
    return screen

def expect(title, lines, present=(), absent=(), last=None):
    """Every `present` text is on the screen, no `absent` one is, and the
    last non-empty line is `last` when given; otherwise the screen is dumped
    and the run fails. `lines` is a screen's display or a recorded copy."""
    text = "\n".join(l.rstrip() for l in lines)
    tail = [l.rstrip() for l in lines if l.strip()]
    missing = [p for p in present if p not in text]
    unwanted = [a for a in absent if a in text]
    wrong_last = last is not None and (not tail or tail[-1] != last)
    if not (missing or unwanted or wrong_last):
        return
    print(f"FAIL: {title}")
    for m in missing: print(f"   expected on screen: {m!r}")
    for a in unwanted: print(f"   should not be on screen: {a!r}")
    if wrong_last: print(f"   expected last line {last!r}, saw {tail[-1] if tail else ''!r}")
    for i, line in enumerate(lines):
        if line.strip(): print(f"{i:2}| {line.rstrip()}")
    srv.shutdown(); sys.exit(1)

BUBBLE = ["▎ false exited 1.", "▎ kintsu fix · kintsu why · kintsu agent · kintsu ignore · ^K more"]
ANSWER = "Try nvm use 22 && npm run build? (local · not verified)"
LATE_ANSWER = "false: try nvm use 22 && npm run build? (local · not verified)"
QUIET = ["exited 0", "true exited"]   # a successful command never gets a bubble
TYPO = ["▎ Did you mean git status?", "Tab to fix · kintsu why · kintsu agent · kintsu ignore · ^K more"]
PANEL_HEADER = "▎ Why w   Fix f   Agent a   Ignore i   Privacy p"

def check_shell_messages(shell):
    """The bubble under a failure, then the model's answer: at once, while
    text is being typed (the typed text survives), and after another
    command (still shown under the failure it is about)."""
    go = run_zsh if shell == "zsh" else (lambda **kw: run(None, **kw)[0])
    expect(f"{shell}: bubble and answer", go().display, BUBBLE + [ANSWER], QUIET)
    screen = go(typed="echo hel")
    expect(f"{shell}: answer lands while typing", screen.display, BUBBLE + [ANSWER, "❯ echo hel", "\nhel\n"], QUIET)
    expect(f"{shell}: answer after another command", go(enter_first=True).display, BUBBLE + [ANSWER, "❯ true"], QUIET)

def check_why(shell):
    """`clear` ran between the failure and `kintsu why`, so the shell has
    moved on and the explanation names its command. Where exactly the
    answer lands relative to the `kintsu why` line depends on whether it
    arrives before or after the next prompt is drawn; only the answer is
    asserted."""
    go = run_zsh if shell == "zsh" else (lambda **kw: run(None, **kw)[0])
    expect(f"{shell}: kintsu why", go(why=True).display,
           ["▎ false: nvm use 22 && npm run build", "▎ — local"])

def check_late(shell):
    """A slow model: the answer lands after the next command and names the
    command it is about, so it never reads as being about the current one.
    The delay stretches with the waits, or the answer would beat `true`."""
    os.environ["DELAY"] = str(1.5 * SLOW)
    try:
        go = run_zsh if shell == "zsh" else (lambda **kw: run(None, **kw)[0])
        screen = go(enter_first=True)
    finally:
        os.environ.pop("DELAY", None)
    expect(f"{shell}: late answer is labelled", screen.display, BUBBLE + ["❯ true", LATE_ANSWER], [ANSWER] + QUIET)

def check_ghost(shell):
    """A typo's rule fix: zsh pre-types it as ghost text, fish inserts it on
    Tab; Enter runs it in both."""
    rec = {}
    screen = run_zsh(ghost=True, wait=1.0, record=rec) if shell == "zsh" else run(None, ghost=True, wait=1.0, record=rec)[0]
    if shell == "zsh":
        expect("zsh: ghost text is pre-typed", rec["before Tab"], TYPO, last="❯ git status")
    else:
        expect("fish: nothing pre-typed, the bubble says Tab", rec["before Tab"], TYPO, last="❯")
    expect(f"{shell}: Tab takes the fix", rec["after Tab"], last="❯ git status")
    expect(f"{shell}: Enter runs the fix", screen.display, ["❯ git status", "On branch main (fake git)"])

def check_panel(shell):
    rec = {}
    if shell == "fish": run(None, panel=True, record=rec)
    elif shell == "zsh": run_zsh(panel=True, record=rec)
    else: run_bash(record=rec)
    typo_line = TYPO if shell != "bash" else ["▎ Did you mean git status?", "kintsu why · kintsu agent · kintsu ignore · ^K more"]
    expect(f"{shell}: the typo's bubble", rec["after the typo"], typo_line, [PANEL_HEADER])
    expect(f"{shell}: ^K opens the panel on the fix", rec["^K: the panel, on the fix"],
           ["▎ gti status · exit 127", PANEL_HEADER, "▎ git status", "rule command typo", "Insert ⏎ · Copy c"])
    expect(f"{shell}: Enter puts the fix in the line and closes", rec["Enter: the fix is in the line, the panel is gone"],
           typo_line, [PANEL_HEADER], last="❯ git status")
    expect(f"{shell}: ^K after a plain failure", rec["^K after a plain failure"],
           ["▎ false · exit 1", PANEL_HEADER, "suggested by local"])
    expect(f"{shell}: w asks why", rec["w: why, from the model"], [PANEL_HEADER, "▎ — local"])
    expect(f"{shell}: Esc closes and puts the bubble back", rec["Esc: closed"], ["▎ false exited 1."], [PANEL_HEADER])
    asked_before = rec["Esc: closed · model calls"]
    expect(f"{shell}: ^K again shows the remembered answer", rec["^K again, w: the explanation is remembered, nobody is asked"],
           [PANEL_HEADER, "▎ — local"])
    if rec["^K again, w: the explanation is remembered, nobody is asked · model calls"] != asked_before:
        print(f"FAIL: {shell}: the remembered explanation asked the model again"); srv.shutdown(); sys.exit(1)
    expect(f"{shell}: ^K after a success opens nothing", rec["^K after a success: nothing opens"], ["❯ true"], [PANEL_HEADER], last="❯")

CHECKS = {
    "fish": lambda: check_shell_messages("fish"),
    "zsh": lambda: check_shell_messages("zsh"),
    "why": lambda: [check_why(s) for s in ("fish", "zsh")],
    "late": lambda: [check_late(s) for s in ("fish", "zsh")],
    "ghost": lambda: [check_ghost(s) for s in ("fish", "zsh")],
    "panel": lambda: [check_panel(s) for s in ("fish", "zsh", "bash")],
}

if "--check" in sys.argv:
    names = [a for a in sys.argv[1:] if a != "--check"] or list(CHECKS)
    for name in names:
        started = time.time()
        CHECKS[name]()
        print(f"ok   {name} ({time.time() - started:.0f} s)")
    srv.shutdown(); sys.exit(0)

which = sys.argv[1:] or list(variants)
if which == ["tee"]:
    show("zsh, stderr copied by the hook", run_zsh(tee=True))
    show("bash, stderr copied by the hook", run_bash(tee=True))
    srv.shutdown(); sys.exit(0)
if which == ["panel"]:
    run(None, panel=True)
    run_zsh(panel=True)
    run_bash()
    srv.shutdown(); sys.exit(0)
if which == ["ghost"]:
    screen, raw = run(None, ghost=True, wait=1.0)
    show("fish, Tab takes the rule fix", screen)
    show("zsh, ghost text then Tab", run_zsh(ghost=True, wait=1.0))
    srv.shutdown(); sys.exit(0)
if which == ["why"]:
    screen, raw = run(None, why=True)
    show("fish, kintsu why", screen)
    show("zsh, kintsu why", run_zsh(why=True))
    srv.shutdown(); sys.exit(0)
if which == ["late"]:
    screen, raw = run(None, enter_first=True)
    show("fish, answer after another command", screen)
    show("zsh, answer after another command", run_zsh(enter_first=True))
    srv.shutdown(); sys.exit(0)
if which == ["zsh"]:
    show("zsh", run_zsh())
    show("zsh (while typing 'echo hel')", run_zsh(typed="echo hel"))
    show("zsh (another command before the answer)", run_zsh(enter_first=True))
    srv.shutdown(); sys.exit(0)
for name in which:
    screen, raw = run(variants[name], typed=None)
    show(name, screen)
    screen, raw = run(variants[name], typed="echo hel")
    show(name + " (while typing 'echo hel')", screen)
    screen, raw = run(variants[name], enter_first=True)
    show(name + " (another command before the answer)", screen)
srv.shutdown()
