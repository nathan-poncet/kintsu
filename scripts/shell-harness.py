#!/usr/bin/env python3
"""Drives real zsh and fish sessions in a pseudo-terminal, against a fake
Ollama server, and prints what a screen emulator shows afterwards. This is
how the hooks' drawing is checked: the bubble, the "asking…" line, and the
message that replaces it above the prompt while text is being typed.

Needs `pyte` (pip install pyte), zsh and fish on the PATH, and a built
binary (cargo build). Not part of CI: run it after touching shell/*.

    python3 scripts/shell-harness.py            # fish scenarios
    python3 scripts/shell-harness.py zsh        # zsh scenarios
    python3 scripts/shell-harness.py why        # kintsu why in both shells
    python3 scripts/shell-harness.py late       # the answer lands after another command
    python3 scripts/shell-harness.py ghost      # a typo, the pre-typed fix, Tab, Enter
    python3 scripts/shell-harness.py panel      # ^K, the panel, Enter, w, Esc (fish, zsh, bash)
    DELAY=1.5 …                                 # slow the fake model down

Environment: BIN (directory of the kintsu binary, default target/debug),
ROOT (scratch directory, default /tmp/kintsu-harness; keep it short, it
holds a Unix socket), PYLIB (extra sys.path entry for pyte).
"""
import os, pty, select, time, re, json, threading, socketserver, http.server, sys, shutil
if os.environ.get("PYLIB"):
    sys.path.insert(0, os.environ["PYLIB"])
import pyte

ROOT = os.environ.get("ROOT", "/tmp/kintsu-harness")
BIN = os.path.abspath(os.environ.get("BIN", os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "target", "debug")))
class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        n = int(self.headers.get("content-length", 0)); self.rfile.read(n)
        time.sleep(float(os.environ.get("DELAY", "0")))
        body = json.dumps({"message": {"role": "assistant", "content": "nvm use 22 && npm run build"}, "done": True}).encode()
        self.send_response(200); self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(body))); self.end_headers(); self.wfile.write(body)
    def log_message(self, *a): pass
srv = socketserver.TCPServer(("127.0.0.1", 0), H); port = srv.server_address[1]
threading.Thread(target=srv.serve_forever, daemon=True).start()
os.makedirs(ROOT, exist_ok=True)
open(f"{ROOT}/config.toml", "w").write(f'[models.local]\nprovider = "ollama"\nmodel = "m"\nbase_url = "http://127.0.0.1:{port}"\n[routing]\nquick_fix = ["local"]\nexplain = ["local"]\n[ui]\neager_fix = true\n')
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

def run(variant_fn, typed=None, wait=4.0, enter_first=False, why=False, ghost=False, panel=False):
    import shutil, subprocess
    subprocess.run(["pkill", "-f", "kintsu daemon run"], capture_output=True); time.sleep(0.3)
    shutil.rmtree(f"{ROOT}/state", ignore_errors=True)
    for f in ("d.sock", "d.spawn"):
        try: os.remove(f"{ROOT}/{f}")
        except FileNotFoundError: pass
    pid, fd = pty.fork()
    if pid == 0:
        os.execve(shutil.which("fish") or "fish", ["fish", "-N", "-i"], env)
    import fcntl, termios, struct
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    screen = pyte.Screen(80, 24); stream = pyte.ByteStream(screen)
    raw = b""
    def drain(t):
        nonlocal raw
        end = time.time() + t
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
        print("   after the typo:", " | ".join(snapshot[-4:]))
        send("\t", 0.5)                     # accept
        accepted = [l.rstrip() for l in screen.display if l.strip()]
        send("\n", 0.8)                     # run it
        print("   before Tab:", snapshot[-1] if snapshot else "")
        print("   after Tab: ", accepted[-1] if accepted else "")
    if panel:
        panel_steps(send, screen, "fish")
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
def run_zsh(typed=None, wait=4.0, enter_first=False, why=False, ghost=False, panel=False):
    import shutil, subprocess
    subprocess.run(["pkill", "-f", "kintsu daemon run"], capture_output=True); time.sleep(0.3)
    shutil.rmtree(f"{ROOT}/state", ignore_errors=True)
    for f in ("d.sock", "d.spawn"):
        try: os.remove(f"{ROOT}/{f}")
        except FileNotFoundError: pass
    zenv = dict(env); zenv.pop("ZDOTDIR", None)
    pid, fd = pty.fork()
    if pid == 0:
        os.execve(shutil.which("zsh") or "zsh", ["zsh", "-f", "-i"], zenv)
    import fcntl, termios, struct
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    screen = pyte.Screen(80, 24); stream = pyte.ByteStream(screen)
    def drain(t):
        end = time.time() + t
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
    if ghost:
        send("gti status\n", 1.0)
        snapshot = [l.rstrip() for l in screen.display if l.strip()]
        send("\t", 0.5)
        accepted = [l.rstrip() for l in screen.display if l.strip()]
        send("\n", 0.8)
        print("   before Tab:", snapshot[-1] if snapshot else "")
        print("   after Tab: ", accepted[-1] if accepted else "")
    if panel:
        panel_steps(send, screen, "zsh")
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

def panel_steps(send, screen, shell):
    """^K on a typo (a rule fix), Enter takes it; ^K after a plain failure,
    w asks why, Esc closes. Prints the screen at each step."""
    def snap(title):
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
    snap("^K after a plain failure")
    send("w", 2.5)
    snap("w: why, from the model")
    send("\x1b", 1.0)
    snap("Esc: closed")

def run_bash():
    import shutil, subprocess
    subprocess.run(["pkill", "-f", "kintsu daemon run"], capture_output=True); time.sleep(0.3)
    shutil.rmtree(f"{ROOT}/state", ignore_errors=True)
    for f in ("d.sock", "d.spawn"):
        try: os.remove(f"{ROOT}/{f}")
        except FileNotFoundError: pass
    bash = os.environ.get("BASH_BIN") or shutil.which("bash") or "bash"
    pid, fd = pty.fork()
    if pid == 0:
        os.execve(bash, ["bash", "--norc", "--noprofile", "-i"], env)
    import fcntl, termios, struct
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    screen = pyte.Screen(80, 24); stream = pyte.ByteStream(screen)
    def drain(t):
        end = time.time() + t
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
    panel_steps(send, screen, f"bash ({bash})")
    send("kintsu daemon stop\n", 0.6)
    send("exit\n", 0.4)
    try: os.close(fd)
    except OSError: pass
    return screen

which = sys.argv[1:] or list(variants)
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
