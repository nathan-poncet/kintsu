/* Kintsu website: scene players.
   Each `.player[data-scene]` becomes a small terminal that plays a scripted
   scene, subtitles included, starts when it scrolls into view, and lets the
   visitor take the wheel: click a word in the bubble, or focus the terminal
   and use the keys. Vanilla JS, no dependencies. Nothing here runs a
   command, and nothing will in the real thing either. */

(() => {
  const reduced = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  const esc = (s) => String(s).replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" }[c]));
  const el = (tag, cls, html) => { const n = document.createElement(tag); if (cls) n.className = cls; if (html != null) n.innerHTML = html; return n; };

  // ── step helpers: the scene DSL ──────────────────────────────────────
  const P = () => ({ p: 1 });                       // new prompt
  const T = (text) => ({ t: text });                // type into the prompt
  const E = () => ({ e: 1 });                       // Enter: settle the prompt
  const O = (...lines) => ({ o: lines });           // output lines ("text" or {text, cls})
  const err = (text) => ({ text, cls: "err" });
  const dim = (text) => ({ text, cls: "dimline" });
  const W = (ms) => ({ w: ms });                    // wait
  const S = (text) => ({ s: text });                // subtitle
  const TOAST = (spec) => ({ toast: spec });        // the bubble, collapsed
  const GHOST = (text) => ({ ghost: text });        // ghost text on the prompt
  const TAB = () => ({ tab: 1 });                   // accept the ghost text
  const OPEN = (tab) => ({ open: tab });            // expand into the panel
  const TABTO = (tab) => ({ tabto: tab });          // switch panel section
  const CLOSE = () => ({ close: 1 });               // collapse
  const INSERT = () => ({ insert: 1 });             // insert the suggestion in the line
  const MSG = (spec) => ({ msg: spec });            // a message arriving above the prompt
  const END = () => ({ end: 1 });

  const ACTS = {
    tab: { label: "Tab to fix", key: true }, why: { label: "Why", act: "why" }, fix: { label: "Fix", act: "fix" },
    agent: { label: "Agent", act: "agent" }, ignore: { label: "Ignore", act: "ignore" }, more: { label: "^K more", act: "more" },
  };
  const sugg = (cmd, warn) => `<div class="sugg"><span class="cmd">${esc(cmd)}</span><span class="ops">${warn ? `<span class="warn">⚠ ${warn}</span> · ` : ""}<b data-act="insert">Insert ⏎</b> · Copy c</span></div>`;
  const agentLines = (...lines) => lines.map((l) => `<span class="d">claude-code ·</span> ${l}<br>`);

  // ── the scenes ───────────────────────────────────────────────────────
  const BUILD_TRACE = [
    "> acme@1.4.0 build", "> node scripts/build.js", "",
    err("node:internal/modules/cjs/loader:1228"), err("  throw err;"), err("  ^"),
    err("Error: Cannot find module 'node:sqlite'"),
    dim("    at Module._resolveFilename (node:internal/modules/cjs/loader:1225:15)"),
    dim("    at Module._load (node:internal/modules/cjs/loader:1051:27)"),
    dim("    … 6 more"),
  ];
  const typoToast = {
    s: "Did you mean <b>git status</b>?", a: ["tab", "why", "agent", "ignore", "more"], fix: "git status",
    head: "gti status · exit 127 · 0.01 s", cmd: "gti status",
    sections: {
      why: ["<b>gti</b> is not on your PATH; <b>git</b> is one keystroke away. Rule: command-name typo. No model involved."],
      fix: "Rule · command-name typo · not destructive" + sugg("git status"),
      agent: ["This one does not need an agent, but you can still hand it off. <span class='d'>⏎ to start · p to review what is sent</span>"],
      ignore: "Quiet for <b>gti</b>: <span class='d'>this session · always</span>",
      privacy: "Nothing left the machine. Rules run locally.",
    },
  };
  const buildToast = {
    s: "Build failed after 12 s. Want a look?", a: ["why", "fix", "agent", "ignore", "more"], fix: "nvm use 22 && npm run build",
    head: "npm run build · exit 1 · 12 s", cmd: "npm run build",
    sections: {
      why: ["Module not found: <b>node:sqlite</b> ships with Node 22.5 or later; ", "this shell runs 20.11. ", "nvm has 22.12 installed.", sugg("nvm use 22 && npm run build")],
      fix: "Small model · confidence 0.92 · not destructive" + sugg("nvm use 22 && npm run build"),
      agent: ["Handing this to <b>Claude Code</b>: command, output (redacted: 1 token), cwd, git state, last 10 commands.<br>", "<span class='d'>⏎ to start · p to review what is sent</span><br><br>",
        ...agentLines("reading scripts/build.js …", "package.json says <b>\"engines\": { \"node\": \">=22\" }</b>", "proposing: add <b>.nvmrc</b> with 22, and a preinstall check.")],
      ignore: "Quiet for <b>npm run build</b>: <span class='d'>this directory · this session · always</span>",
      privacy: `What leaves the machine, to <b>haiku</b> (Anthropic):<br><span class="ok">✓</span> command, exit status, duration<br><span class="ok">✓</span> 14 lines of output — <span class="redact">NPM_TOKEN=npm_••••••••</span> redacted<br><span class="ok">✓</span> cwd ~/dev/acme · branch main, clean · last 10 commands<br><span class="d">✗</span> environment variables · file contents · anything you did not see here`,
    },
  };

  const SCENES = {
    hero: [
      S("A typo. A rule knows. The fix is already typed for you."),
      P(), W(600), T("gti status"), E(), O(err("zsh: command not found: gti")), W(400),
      TOAST(typoToast), GHOST("git status"), W(1500), TAB(), W(500), E(),
      O("On branch main", "Your branch is up to date with 'origin/main'.", "nothing to commit, working tree clean"), W(1200),
      S("A real failure. ^K expands the bubble; the explanation streams in."),
      P(), T("npm run build"), E(), O(...BUILD_TRACE), W(400),
      TOAST(buildToast), P(), W(1600), OPEN("why"), W(4200),
      S("Privacy is a tab, not a promise: the exact payload, secrets redacted."),
      TABTO("privacy"), W(3000), CLOSE(), W(500),
      S("You move on. The follow-up arrives above your prompt, when it is ready."),
      T("vim scripts/build.js"), E(), P(), W(1500),
      MSG({ html: "<span class='d'>haiku ·</span> Also: package.json says <b>\"engines\": { \"node\": \">=22\" }</b>. A <b>.nvmrc</b> would stop this from happening again. <span class='g'>^K</span> to add one.",
        fix: "echo 22 > .nvmrc", head: "npm run build · follow-up", cmd: "npm run build",
        sections: { why: ["Every shell that opens this directory picks the right Node with nvm, fnm or volta. One file, no surprises."], fix: sugg("echo 22 > .nvmrc"), agent: ["Hand the follow-up to <b>Claude Code</b>? <span class='d'>⏎ to start</span>"], ignore: "Quiet about follow-ups for <b>npm run build</b>.", privacy: "Sent to <b>haiku</b>: the previous case only. Nothing new left the machine." } }),
      W(5000), END(),
    ],

    typo: [
      S("A command-name typo. A rule, no model, under a millisecond."),
      P(), W(500), T("gti status"), E(), O(err("zsh: command not found: gti")), W(400),
      TOAST(typoToast), GHOST("git status"), W(1600), TAB(), W(500), E(),
      O("On branch main", "nothing to commit, working tree clean"), W(1200),
      S("Not a directory? The rule knows your tree."),
      P(), T("cd api"), E(), O(err("cd: not a directory: api")), W(400),
      TOAST({ s: "<b>apps/api</b> is two levels down.", a: ["tab", "why", "ignore", "more"], fix: "cd apps/api", head: "cd api · exit 1", cmd: "cd api",
        sections: { why: ["There is a file named <b>api</b> here and a directory <b>apps/api</b> below. Rule: cd into a file, unique match in the tree."], fix: sugg("cd apps/api"), agent: ["No agent needed."], ignore: "Quiet for <b>cd</b>.", privacy: "Nothing left the machine." } }),
      GHOST("cd apps/api"), W(1500), TAB(), W(500), E(),
      S("Nothing left the machine. Nothing ran until you pressed Enter."),
      P(), W(2500), END(),
    ],

    port: [
      S("The dev server cannot bind. A forgotten process holds the port."),
      P(), W(500), T("npm run dev"), E(),
      O("> acme@1.4.0 dev", "> next dev", "", err("Error: listen EADDRINUSE: address already in use :::3000"), dim("    at Server.setupListenHandle [as _listen2] (node:net:1908:16)"), dim("    … 4 more")), W(400),
      TOAST({ s: "Something already listens on <b>:3000</b>. Find out what?", a: ["why", "fix", "agent", "ignore", "more"], fix: "kill 4821", head: "npm run dev · exit 1 · 0.4 s", cmd: "npm run dev",
        sections: {
          why: ["<span class='d'>lsof -i :3000 ·</span> <b>node</b>, pid 4821, started 2 h ago from <b>~/dev/acme-old</b>. ", "A dev server you forgot in another project.", sugg("kill 4821", "ends a process")],
          fix: "Rule: port in use · <span class='warn'>⚠ ends a process</span> · you press Enter" + sugg("kill 4821", "ends a process"),
          agent: ["Hand it to <b>Claude Code</b>? Probably overkill for this one. <span class='d'>⏎ to start</span>"],
          ignore: "Quiet for <b>npm run dev</b>: <span class='d'>this directory · this session · always</span>",
          privacy: "Read-only probe <b>lsof -i :3000</b> ran locally when you asked Why. Nothing left the machine.",
        } }),
      P(), W(1500), OPEN("why"), W(3800),
      S("Ending a process is flagged in red. Kintsu inserts the command; you press Enter."),
      W(1800), INSERT(), W(1000), E(), W(300),
      P(), T("npm run dev"), E(), O("▲ Next.js 15.1", "- Local: http://localhost:3000", "✓ Ready in 1.2 s"),
      S("You knew what you killed, and where it came from."), W(2500), END(),
    ],

    docker: [
      S("Docker Desktop is closed. The error does not say so."),
      P(), W(500), T("docker compose up -d"), E(),
      O(err("Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?")), W(400),
      TOAST({ s: "Docker Desktop is not running. Start it?", a: ["tab", "why", "ignore", "more"], fix: "open -a Docker", head: "docker compose up -d · exit 1", cmd: "docker compose up -d",
        sections: { why: ["The socket is missing, so no daemon is listening. On this Mac, Docker Desktop owns it. On Linux the fix would be <b>sudo systemctl start docker</b>, and the sudo would be flagged."], fix: sugg("open -a Docker"), agent: ["No agent needed."], ignore: "Quiet for <b>docker</b> while Docker is down.", privacy: "Nothing left the machine." } }),
      GHOST("open -a Docker"), W(1500), TAB(), W(500), E(),
      S("Twenty seconds later, a message arrives. Your prompt never waited."),
      P(), W(2400),
      MSG({ html: "Docker is up. Run <b>docker compose up -d</b> again? <span class='g'>Tab</span>.", fix: "docker compose up -d", head: "docker · follow-up", cmd: "docker compose up -d",
        sections: { why: ["The daemon noticed the socket appear and remembered what you were doing."], fix: sugg("docker compose up -d"), agent: ["No agent needed."], ignore: "Quiet about Docker follow-ups.", privacy: "Nothing left the machine." } }),
      GHOST("docker compose up -d"), W(1600), TAB(), W(500), E(),
      O("[+] Running 3/3", " ✔ Network acme_default   Created", " ✔ Container acme-db-1    Started", " ✔ Container acme-api-1   Started"),
      S("No polling loop in your shell. The resident daemon watched."), W(2500), END(),
    ],

    git: [
      S("The remote moved on. git's advice is buried in eight lines of hint."),
      P(), W(500), T("git push"), E(),
      O("To github.com:acme/acme.git", err(" ! [rejected]        main -> main (fetch first)"), err("error: failed to push some refs to 'github.com:acme/acme.git'"),
        dim("hint: Updates were rejected because the remote contains work that you do not"), dim("hint: have locally. This is usually caused by another repository pushing to"), dim("hint: the same ref. Integrate the remote changes (e.g. 'git pull ...') before"), dim("hint: pushing again.")), W(400),
      TOAST({ s: "Remote <b>main</b> has 2 commits you don't. Rebase yours on top?", a: ["tab", "why", "agent", "ignore", "more"], fix: "git pull --rebase && git push", head: "git push · exit 1 · 0.9 s", cmd: "git push",
        sections: {
          why: ["<b>origin/main</b> is 2 ahead: ", "<i>fix rate limit</i> (Marie, 40 min ago) and <i>bump lockfile</i> (CI, 38 min ago). ", "Yours is 1 ahead. A rebase keeps history linear.", sugg("git pull --rebase && git push")],
          fix: "Rule: non-fast-forward push · not destructive" + sugg("git pull --rebase && git push") + "<br><span class='warn'>⚠ git push --force</span> <span class='d'>would also work, and would erase their two commits. Kintsu never proposes it first.</span>",
          agent: ["Hand it to <b>Claude Code</b> to resolve conflicts if the rebase stops? <span class='d'>⏎ to start</span>"],
          ignore: "Quiet for <b>git push</b>.",
          privacy: "Read locally: <b>git log origin/main</b>. Nothing left the machine.",
        } }),
      GHOST("git pull --rebase && git push"), W(1400), OPEN("why"), W(3600),
      S("The force push exists. Kintsu will not hand it to you first."),
      TABTO("fix"), W(2800), CLOSE(), W(600), TAB(), W(500), E(),
      O("Successfully rebased and updated refs/heads/main.", "To github.com:acme/acme.git", "   3f1c2a9..8be7d10  main -> main"),
      S("Linear history, nobody's work erased."), W(2500), END(),
    ],

    wall: [
      S("Forty-eight lines of test output. One cause. Your agent, briefed."),
      P(), W(500), T("cargo test"), E(),
      O(dim("   Compiling acme v0.9.2"), dim("    Finished test profile in 4.2 s"), "running 48 tests", dim("....F..F........F......................."), "",
        err("---- auth::tests::refresh_rotates_token stdout ----"), err("thread panicked at tests/fixtures/token.rs:14: token expired: 2026-09-01 < now"),
        err("---- auth::tests::login_sets_cookie stdout ----"), err("thread panicked at tests/fixtures/token.rs:14: token expired: 2026-09-01 < now"),
        dim("… 22 more lines"), err("test result: FAILED. 45 passed; 3 failed")), W(500),
      TOAST({ s: "3 failures, one cause: the fixture token in <b>tests/fixtures/token.rs</b> expired on Sep 1. Hand it to Claude Code?", a: ["agent", "why", "fix", "ignore", "more"], fix: "cargo test auth::", head: "cargo test · exit 101 · 6.8 s", cmd: "cargo test",
        sections: {
          agent: ["Handing to <b>Claude Code</b>: command, 48 lines of output, cwd, branch <b>feat/refresh</b> (dirty), last 10 commands, your CLAUDE.md. <span class='d'>⏎ to start · p to review</span><br><br>",
            ...agentLines("reading tests/fixtures/token.rs …", "the fixture hardcodes an <b>exp</b> claim; generating it from now() + 1 h instead", "cargo test auth:: → <span class='ok'>48 passed</span>", "diff ready: tests/fixtures/token.rs (+6 −2). Commit?")],
          why: ["The tiny local model read 48 lines and found one panic site shared by the three failures: <b>tests/fixtures/token.rs:14</b>. The date in the fixture is in the past."],
          fix: "No one-line fix: this is code. " + sugg("cargo test auth::"),
          ignore: "Quiet for <b>cargo test</b>: <span class='d'>this session · always</span>",
          privacy: "Summary by <b>tiny</b> (local). Hand-off to <b>Claude Code</b>: your CLI, your account, your terminal. Kintsu sent nothing to a cloud model.",
        } }),
      P(), W(1800), OPEN("agent"), W(7500),
      S("The agent worked in your terminal, on your account. Kintsu wrote the brief."), W(3000), END(),
    ],

    secret: [
      S("There is a token in that command. Watch what leaves the machine."),
      P(), W(500), T('curl -H "Authorization: Bearer sk-live-8f3a…c21e" https://api.acme.dev/v1/users'), E(),
      O('{"error":"invalid_token","hint":"token expired"}', dim("curl: (22) The requested URL returned error: 401")), W(400),
      TOAST({ s: "401 from <b>api.acme.dev</b>: the token expired. Rotate it?", a: ["why", "fix", "ignore", "more"], fix: "acme auth login && export ACME_TOKEN=$(acme auth token)", head: "curl … · exit 22 · 0.3 s", cmd: "curl",
        sections: {
          privacy: `Redaction found a secret in the command: <b>Bearer <span class="redact">sk-live-••••••••••••</span></b><br><span class="ok">✓</span> routed to <b>tiny</b> (local) — <span class="d">sensitive_output = local_only</span><br><span class="d">✗</span> nothing was sent to a cloud model, and nothing will be for this case`,
          why: ["The API says <i>token expired</i>. ", "Your <b>acme</b> CLI can mint a new one. The token itself never appears in the brief."],
          fix: "Local model · confidence 0.81 · not destructive" + sugg("acme auth login && export ACME_TOKEN=$(acme auth token)"),
          agent: ["Hand-off would go to your CLI agent with the <b>redacted</b> case. <span class='d'>⏎ to start</span>"],
          ignore: "Quiet for <b>curl</b> against api.acme.dev.",
        } }),
      P(), W(1500), OPEN("privacy"), W(4000),
      S("A secret in the output or the command forces local-only routing. Automatically."),
      TABTO("why"), W(3000), CLOSE(),
      S("The token stayed on this machine."), W(2500), END(),
    ],

    quiet: [
      S("Most commands succeed. Some you stop yourself. Kintsu says nothing."),
      P(), W(500), T("make"), E(), O(dim("cc -O2 -o bin/acme src/*.c"), dim("ok")), W(700),
      P(), T("sleep 30"), E(), O(dim("^C")), W(700),
      P(), T("rg TODO src | head -2"), E(), O(dim("src/auth.c:41: // TODO rotate"), dim("src/db.c:9: // TODO pool")), W(700),
      S("One real failure, one bubble."),
      P(), T("make test"), E(), O(err("test_auth: FAILED (assert token != NULL)"), err("make: *** [Makefile:22: test] Error 1")), W(400),
      TOAST({ s: "1 test failed: <b>test_auth</b>, null token. Ask?", a: ["why", "agent", "ignore", "more"], fix: "", head: "make test · exit 2 · 1.1 s", cmd: "make test",
        sections: { why: ["<b>test_auth</b> asserts a token that the fixture no longer provides since the last commit touched <b>tests/fixtures.c</b>."], fix: "No one-line fix: this is code.", agent: ["Hand it to your agent? <span class='d'>⏎ to start</span>"], ignore: "Quiet for <b>make test</b>: <span class='d'>this directory · this session · always</span>", privacy: "Nothing left the machine yet." } }),
      P(), W(1800),
      S("The same failure again, while you fix it: no second bubble."),
      T("make test"), E(), O(err("test_auth: FAILED (assert token != NULL)"), err("make: *** [Makefile:22: test] Error 1")), W(1200),
      P(), W(600),
      S("Silence is a feature. Dismiss, mute for an hour, ignore per command, directory, or forever."),
      T("kintsu mute 1h"), E(), O(dim("quiet until 17:42")), W(2500), END(),
    ],
  };

  // ── the player ───────────────────────────────────────────────────────
  class Player {
    constructor(root) {
      this.root = root;
      this.scene = SCENES[root.dataset.scene];
      this.loop = root.dataset.loop === "true";
      this.run = 0; this.driving = false; this.played = false; this.playing = false;
      this.current = null; this.prompt = null;
      root.innerHTML = `
        <div class="terminal" tabindex="0" aria-label="Scene: ${esc(root.dataset.title)}. Click a word in the bubble, or use Tab, Control K, w, f, a, i, p, Escape.">
          <div class="titlebar"><span class="dots" aria-hidden="true"><i></i><i></i><i></i></span><span class="title">${esc(root.dataset.title)}</span><span class="badge">scene</span></div>
          <div class="screen"></div>
        </div>
        <div class="player-bar">
          <button type="button" class="play" aria-label="Play"><span aria-hidden="true">▶</span></button>
          <div class="progress" aria-hidden="true"><i></i></div>
          <p class="subtitle" aria-live="polite"></p>
        </div>`;
      this.term = root.querySelector(".terminal"); this.screen = root.querySelector(".screen");
      this.badge = root.querySelector(".badge"); this.bar = root.querySelector(".progress i");
      this.subtitle = root.querySelector(".subtitle"); this.playBtn = root.querySelector(".play");
      this.playBtn.addEventListener("click", () => this.replay());
      this.screen.addEventListener("click", (e) => {
        const a = e.target.closest("[data-act]"); if (a) { this.act(a.dataset.act); return; }
        const t = e.target.closest("[data-tab]"); if (t) { this.takeWheel(); this.switchTab(t.dataset.tab); }
      });
      this.term.addEventListener("keydown", (e) => this.onKey(e));
    }

    // timing
    sleep(ms) { return new Promise((r) => setTimeout(r, reduced ? 0 : ms)); }
    stale(id) { return id !== this.run; }
    scrollDown() { this.screen.scrollTop = this.screen.scrollHeight; }
    setState(text, live) { this.badge.textContent = text; this.badge.classList.toggle("live", !!live); }

    // lines
    newPrompt() {
      const line = el("div", "line", `<span class="prompt">$</span> <span class="typed"></span><span class="ghost"></span><span class="cursor"></span>`);
      this.screen.appendChild(line); this.scrollDown(); this.prompt = line; return line;
    }
    async type(text, id) {
      if (!this.prompt) this.newPrompt();
      const typed = this.prompt.querySelector(".typed");
      for (const ch of text) { if (this.stale(id)) return; typed.textContent += ch; await this.sleep(34 + Math.random() * 28); }
    }
    settle() { if (!this.prompt) return; this.prompt.querySelector(".cursor")?.remove(); this.prompt.querySelector(".ghost")?.remove(); this.prompt = null; }
    out(l) { const t = typeof l === "string" ? l : l.text; const cls = typeof l === "string" ? "out" : l.cls || "out"; this.screen.appendChild(el("div", "line " + cls, esc(t))); this.scrollDown(); }

    // bubble
    actions(list) {
      return `<span class="actions">` + list.map((k, i) => { const a = ACTS[k];
        return (i ? `<span class="sep"> · </span>` : "") + (a.key ? `<span class="act key">${a.label}</span>` : `<span class="act" data-act="${a.act}">${a.label}</span>`); }).join("") + `</span>`;
    }
    toast(spec) {
      const t = el("div", "toast", `<div class="line"><span class="seam"></span>${spec.s}</div><div class="line"><span class="seam"></span>${this.actions(spec.a)}</div>`);
      this.screen.appendChild(t); this.scrollDown();
      this.current = { ...spec, toast: t, panel: null, ghostText: null };
    }
    message(spec) {
      const m = el("div", "toast arriving", `<div class="line"><span class="seam"></span>${spec.html}</div>`);
      if (this.prompt) this.prompt.before(m); else this.screen.appendChild(m);
      this.scrollDown();
      this.current = { ...spec, a: ["why", "fix", "ignore", "more"], toast: m, panel: null, ghostText: null };
    }
    ghost(text) {
      if (!this.prompt) this.newPrompt();
      this.prompt.querySelector(".ghost").textContent = text;
      if (this.current) this.current.ghostText = text;
    }
    acceptGhost() {
      if (!this.prompt || !this.current?.ghostText) return false;
      this.prompt.querySelector(".typed").textContent = this.current.ghostText;
      this.prompt.querySelector(".ghost").textContent = "";
      this.current.ghostText = null; return true;
    }
    expand(section) {
      const c = this.current; if (!c) return;
      if (c.panel) { this.switchTab(section); return; }
      c.toast.style.display = "none";
      const p = el("div", "panel", `
        <div class="head"><span>${esc(c.head || c.cmd)}</span><span class="esc" data-act="dismiss">esc</span></div>
        <div class="tabs">${["why", "fix", "agent", "ignore", "privacy"].map((s) => `<span class="tab" data-tab="${s}">${s[0].toUpperCase() + s.slice(1)}<span class="k">${s[0]}</span></span>`).join("")}</div>
        <div class="body"></div>`);
      c.toast.after(p); c.panel = p; this.setState("panel", true);
      this.switchTab(section); this.scrollDown();
    }
    collapse() {
      const c = this.current; if (!c?.panel) return;
      c.panel.remove(); c.panel = null; c.toast.style.display = "";
      this.setState(this.driving ? "you're driving" : "scene", this.driving); this.scrollDown();
    }
    async switchTab(name) {
      const c = this.current; if (!c?.panel) return;
      const id = this.run;
      c.panel.querySelectorAll(".tab").forEach((t) => t.classList.toggle("on", t.dataset.tab === name));
      const body = c.panel.querySelector(".body"); body.innerHTML = "";
      const content = c.sections?.[name] ?? "…";
      if (typeof content === "string") { body.innerHTML = content; this.scrollDown(); return; }
      for (const chunk of content) {
        if (this.stale(id) || !c.panel) return;
        body.insertAdjacentHTML("beforeend", chunk); this.scrollDown();
        await this.sleep(Math.min(1400, chunk.replace(/<[^>]+>/g, "").length * 9 + 60));
      }
    }
    ignore() {
      const c = this.current; if (!c) return;
      if (c.panel) this.collapse();
      c.toast.innerHTML = `<div class="line dimline"><span class="seam"></span>Quiet for <b>${esc(c.cmd)}</b> in this directory for 10 minutes.</div>`;
    }
    insert() {
      const c = this.current; if (!c?.fix) return;
      this.collapse();
      if (!this.prompt) this.newPrompt();
      this.prompt.querySelector(".typed").textContent = c.fix;
      this.prompt.querySelector(".ghost").textContent = ""; c.ghostText = null;
    }

    // interaction
    takeWheel() {
      if (this.driving) return;
      this.driving = true; this.run++; this.playing = false;
      this.setState("you're driving", true); this.playBtn.innerHTML = `<span aria-hidden="true">↻</span>`; this.playBtn.setAttribute("aria-label", "Replay");
      this.subtitle.textContent = "You're driving. Tab accepts ghost text · ^K toggles the panel · w f a i p switch sections · Esc collapses · ⏎ inserts.";
    }
    act(name) {
      this.takeWheel();
      switch (name) {
        case "dismiss": this.collapse(); break;
        case "ignore": this.ignore(); break;
        case "more": this.current?.panel ? this.collapse() : this.expand("why"); break;
        case "insert": this.insert(); break;
        default: this.expand(name);
      }
    }
    onKey(e) {
      const k = e.key;
      if (k === "Tab") { if (this.acceptGhost()) { e.preventDefault(); this.takeWheel(); } return; }
      if ((e.ctrlKey && k.toLowerCase() === "k") || k === "k") { e.preventDefault(); this.act("more"); return; }
      if (k === "Escape") { e.preventDefault(); this.takeWheel(); this.collapse(); return; }
      if (k === "Enter" && this.current?.panel) { e.preventDefault(); this.act("insert"); return; }
      const map = { w: "why", f: "fix", a: "agent", i: "ignore", p: "privacy" };
      if (map[k] && !e.metaKey && !e.ctrlKey && !e.altKey) { e.preventDefault(); this.act(map[k]); }
    }

    // playback
    replay() { this.driving = false; this.play(); }
    async play() {
      const id = ++this.run;
      this.played = true; this.playing = true;
      this.screen.innerHTML = ""; this.current = null; this.prompt = null;
      this.setState("scene", false); this.bar.style.width = "0%";
      this.playBtn.innerHTML = `<span aria-hidden="true">↻</span>`; this.playBtn.setAttribute("aria-label", "Replay");
      const steps = this.scene; const total = steps.length;
      for (let i = 0; i < total; i++) {
        if (this.stale(id)) return;
        const st = steps[i];
        this.bar.style.width = `${Math.round(((i + 1) / total) * 100)}%`;
        if (st.p) this.newPrompt();
        else if (st.t != null) await this.type(st.t, id);
        else if (st.e) { await this.sleep(260); this.settle(); }
        else if (st.o) { for (const l of st.o) { if (this.stale(id)) return; this.out(l); await this.sleep(28); } }
        else if (st.w) await this.sleep(st.w);
        else if (st.s != null) this.subtitle.textContent = st.s;
        else if (st.toast) this.toast(st.toast);
        else if (st.ghost != null) this.ghost(st.ghost);
        else if (st.tab) this.acceptGhost();
        else if (st.open) this.expand(st.open);
        else if (st.tabto) await this.switchTab(st.tabto);
        else if (st.close) this.collapse();
        else if (st.insert) this.insert();
        else if (st.msg) this.message(st.msg);
        else if (st.end) {
          this.playing = false; this.setState("scene · replay ↻", false);
          if (this.loop && !reduced) { await this.sleep(2500); if (!this.stale(id)) this.play(); }
          return;
        }
      }
    }
  }

  const players = [...document.querySelectorAll(".player[data-scene]")].map((root) => new Player(root));
  if (typeof IntersectionObserver === "function") {
    const io = new IntersectionObserver((entries) => {
      for (const en of entries) {
        const p = players.find((pl) => pl.root === en.target);
        if (p && en.isIntersecting && !p.played) p.play();
      }
    }, { threshold: 0.45 });
    players.forEach((p) => io.observe(p.root));
  } else {
    players.forEach((p) => p.play());
  }
  window.kintsuPlayers = players;
})();
