/* Kintsu website: the tour.
   One terminal, one story in chapters. Each chapter is a command or an
   action; the player can pause, step chapter by chapter, jump to any
   chapter, and stop at each one when "step by step" is on. Beside the
   terminal, an explainer says what is happening and who is doing the work:
   a rule, a model on the machine, a model in the cloud, or the agent.
   Vanilla JS, no dependencies. Nothing here runs a command. */

(() => {
  document.documentElement.classList.add("js");
  const reduced = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  const esc = (s) => String(s).replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" }[c]));
  const el = (tag, cls, html) => { const n = document.createElement(tag); if (cls) n.className = cls; if (html != null) n.innerHTML = html; return n; };
  const timeout = (ms) => new Promise((r) => setTimeout(r, ms));

  // ── the scene DSL ────────────────────────────────────────────────────
  const CH = (spec) => ({ ch: spec });              // chapter start: {id,title,who,where,cost,keys,text}
  const P = () => ({ p: 1 });                       // new prompt
  const T = (text) => ({ t: text });                // type into the prompt
  const E = () => ({ e: 1 });                       // Enter: settle the prompt
  const O = (...lines) => ({ o: lines });           // output lines
  const err = (text) => ({ text, cls: "err" });
  const dim = (text) => ({ text, cls: "dimline" });
  const W = (ms) => ({ w: ms });                    // wait
  const S = (text) => ({ s: text });                // subtitle
  const TOAST = (spec) => ({ toast: spec });        // the bubble, collapsed
  const GHOST = (text) => ({ ghost: text });        // ghost text on the prompt
  const TAB = () => ({ tab: 1 });                   // accept the ghost text
  const OPEN = (tab, stream = true) => ({ open: tab, empty: !stream }); // expand into the panel
  const TABTO = (tab) => ({ tabto: tab });          // switch panel section
  const CLOSE = () => ({ close: 1 });               // collapse
  const INSERT = () => ({ insert: 1 });             // insert the suggestion in the line
  const IGNORE = () => ({ ignore: 1 });             // the Ignore action
  const MSG = (spec) => ({ msg: spec });            // a message arriving above the prompt
  const KEY = (label) => ({ key: label });        // flash the key the scene presses
  const LIVE = () => ({ live: 1 });                // everything before is drawn at once; from here, real time
  const END = () => ({ end: 1 });

  const ACTS = {
    tab: { label: "Tab to fix", key: true }, why: { label: "Why", act: "why" }, fix: { label: "Fix", act: "fix" },
    agent: { label: "Agent", act: "agent" }, ignore: { label: "Ignore", act: "ignore" }, more: { label: "^K more", act: "more" },
  };
  const sugg = (cmd, warn) => `<div class="sugg"><span class="cmd">${esc(cmd)}</span><span class="ops">${warn ? `<span class="warn">⚠ ${warn}</span> · ` : ""}<b data-act="insert">Insert ⏎</b> · Copy c</span></div>`;
  const agentLines = (...lines) => lines.map((l) => `<span class="d">claude-code ·</span> ${l}<br>`);

  // ── the story ────────────────────────────────────────────────────────
  const BUILD_TRACE = [
    "> acme@1.4.0 build", "> node scripts/build.js", "",
    err("node:internal/modules/cjs/loader:1228"), err("  throw err;"), err("  ^"),
    err("Error: Cannot find module 'node:sqlite'"),
    dim("    at Module._resolveFilename (node:internal/modules/cjs/loader:1225:15)"),
    dim("    at Module._load (node:internal/modules/cjs/loader:1051:27)"),
    dim("    … 6 more"),
  ];
  const TEST_OUTPUT = [
    "> acme@1.4.0 test", "> vitest run", "",
    dim(" ✓ src/auth/login.test.ts (12)"), err(" ✗ src/auth/refresh.test.ts (3)"),
    err("   × rotates the token"), err("   × sets the cookie"), err("   × rejects an expired token"), "",
    err("Error: fixture token expired: exp=2026-09-01 < now"), dim("   ❯ tests/fixtures/token.ts:14:9"), dim("   … 18 more lines"),
    err(" Tests  3 failed | 45 passed (48)"),
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
      agent: ["Handing this to <b>Claude Code</b>: command, output (redacted: 1 token), cwd, git state, last 10 commands.<br>", "<span class='d'>⏎ to start · p to review what is sent</span>"],
      ignore: "Quiet for <b>npm run build</b>: <span class='d'>this directory · this session · always</span>",
      privacy: `What left the machine, to <b>haiku</b> (Anthropic, cloud):<br><span class="ok">✓</span> command, exit status, duration<br><span class="ok">✓</span> 14 lines of output — <span class="redact">NPM_TOKEN=npm_••••••••</span> redacted<br><span class="ok">✓</span> cwd ~/dev/acme · branch main, clean · last 10 commands<br><span class="d">✗</span> environment variables · file contents · anything you did not see here`,
    },
  };
  const testToast = {
    s: "3 failures, one cause: the fixture token in <b>tests/fixtures/token.ts</b> expired on Sep 1. Hand it to Claude Code?", a: ["agent", "why", "fix", "ignore", "more"], fix: "npm test -- refresh",
    head: "npm test · exit 1 · 6.8 s", cmd: "npm test",
    sections: {
      agent: ["Handing to <b>Claude Code</b>: command, 48 lines of output, cwd, branch <b>feat/refresh</b> (dirty), last 10 commands, your CLAUDE.md. <span class='d'>⏎ to start · p to review</span><br><br>",
        ...agentLines("reading tests/fixtures/token.ts …", "the fixture hardcodes an <b>exp</b> claim; generating it from Date.now() + 1 h instead", "npm test → <span class='ok'>48 passed</span>", "diff ready: tests/fixtures/token.ts (+6 −2). Commit?")],
      why: ["The tiny local model read 48 lines and found one panic site shared by the three failures: <b>tests/fixtures/token.ts:14</b>. The date in the fixture is in the past."],
      fix: "No one-line fix: this is code. " + sugg("npm test -- refresh"),
      ignore: "Quiet for <b>npm test</b>: <span class='d'>this session · always</span>",
      privacy: "Summary by <b>tiny</b> (local). Hand-off to <b>Claude Code</b>: your CLI, your account, your terminal. Kintsu sent nothing to a cloud model.",
    },
  };
  const lintToast = {
    s: "lint failed on <b>src/legacy.c</b> again. Ask?", a: ["why", "agent", "ignore", "more"], fix: "",
    head: "make lint · exit 2 · 0.8 s", cmd: "make lint",
    sections: { why: ["<b>src/legacy.c</b> has an unused variable and the linter treats warnings as errors. You have seen this bubble twice this week."], fix: "No one-line fix: this is code.", agent: ["Hand it to your agent? <span class='d'>⏎ to start</span>"], ignore: "Quiet for <b>make lint</b>: <span class='d'>this command · this directory · this session · always</span>", privacy: "Nothing left the machine." },
  };
  const followUp = {
    html: "<span class='d'>haiku ·</span> Also: package.json says <b>\"engines\": { \"node\": \">=22\" }</b>. A <b>.nvmrc</b> would stop this from happening again. <span class='g'>^K</span> to add one.",
    fix: "echo 22 > .nvmrc", head: "npm run build · follow-up", cmd: "npm run build",
    sections: { why: ["Every shell that opens this directory picks the right Node with nvm, fnm or volta. One file, no surprises."], fix: sugg("echo 22 > .nvmrc"), agent: ["Hand the follow-up to <b>Claude Code</b>? <span class='d'>⏎ to start</span>"], ignore: "Quiet about follow-ups for <b>npm run build</b>.", privacy: "Sent to <b>haiku</b>: the previous case only. Nothing new left the machine." },
  };

  const SCENES = {
    landing: [
      P(), T("gti status"), E(), O(err("zsh: command not found: gti")),
      TOAST(typoToast), GHOST("git status"), TAB(), E(),
      O("On branch main", "nothing to commit, working tree clean"),
      P(), T("npm run build"), E(), O(...BUILD_TRACE.slice(0, 7), dim("    … 6 more")),
      TOAST(buildToast), P(),
      LIVE(), W(9000), MSG(followUp), END(),
    ],
    tour: [
      CH({ id: "typo", tag: "Tab", title: "A typo, fixed by a rule", who: "a rule", where: "local", cost: "under a millisecond · no model", keys: "Tab or → accepts · anything else ignores · Enter runs",
        text: "<b>gti</b> is not on your PATH. A rule ported from thefuck finds the nearest command and puts it on your next prompt as ghost text. Nothing left the machine, and nothing ran until you pressed Enter." }),
      S("A typo. A rule knows. The fix is already typed for you."),
      P(), W(600), T("gti status"), E(), O(err("zsh: command not found: gti")), W(400),
      TOAST(typoToast), GHOST("git status"), W(1600), KEY("Tab"), TAB(), W(700), KEY("Enter"), E(),
      O("On branch main", "Your branch is up to date with 'origin/main'.", "nothing to commit, working tree clean"), W(1500),

      CH({ id: "bubble", tag: "the bubble", title: "A real failure, a real bubble", who: "a rule and a tiny model", where: "local", cost: "a few hundred milliseconds · free", keys: "click a word · ^K for more",
        text: "The build fails after twelve seconds. A tiny local model turns fourteen lines of trace into one sentence. The bubble: a gold seam, the sentence, a line of words. The words are links. Nothing steals a keystroke from your prompt." }),
      S("A real failure. One sentence, a line of words. Click them, or ^K."),
      P(), T("npm run build"), E(), O(...BUILD_TRACE), W(500), TOAST(buildToast), P(), W(2400),

      CH({ id: "panel", tag: "^K", title: "The bubble becomes a panel", who: "Kintsu", where: "local", cost: "instant", keys: "w f a i p · Esc folds it back · mouse and wheel",
        text: "The same bubble, unfolded in place under your prompt, never full screen. The command and its status, five sections with one-letter keys, mouse welcome. It opened on Why, the first word of the bubble." }),
      S("^K unfolds the bubble in place. Five sections, one key each."),
      KEY("^K"), W(500), OPEN("why", false), W(2600),

      CH({ id: "why", tag: "w · Why", title: "Why it failed", who: "haiku · Anthropic", where: "cloud", cost: "about a tenth of a cent · or local, one line of config", keys: "w · click Why · kintsu why",
        text: "The explanation streams from the model you routed the <b>explain</b> task to, here Claude Haiku. Swap it for Ollama and it never leaves your machine. It reads the redacted case, never your files." }),
      S("Why: the explanation streams in from the model you chose."),
      KEY("w"), W(400), TABTO("why"), W(3400),

      CH({ id: "privacy", tag: "p · Privacy", title: "What left the machine", who: "Kintsu", where: "local", cost: "redaction on by default", keys: "p · click Privacy · kintsu privacy",
        text: "Not a promise, a tab. The exact payload that went to Haiku, redactions highlighted: an npm token sat in the output and the model never saw it. A case marked sensitive would have been answered locally, automatically." }),
      S("Privacy: the exact payload, secrets redacted."),
      KEY("p"), W(400), TABTO("privacy"), W(3800),

      CH({ id: "fix", tag: "f · Fix", title: "One command, two Enters", who: "a small model", where: "cloud or local", cost: "confidence 0.92 · not destructive", keys: "f · ⏎ inserts · Enter runs",
        text: "One corrected command with its confidence, and a red flag when it deserves one. ⏎ inserts it in your line editor and closes the panel; you press Enter again to run it. Kintsu never runs anything itself." }),
      S("Fix: ⏎ puts the command in your line. You press Enter."),
      KEY("f"), W(400), TABTO("fix"), W(2400), KEY("⏎"), INSERT(), W(1400), KEY("Enter"), E(),
      O(dim("Now using node v22.12.0"), "> acme@1.4.0 build", "> node scripts/build.js", "✓ built in 3.1 s"), W(1800),

      CH({ id: "agent", tag: "a · Agent", title: "Hand the case to your agent", who: "Claude Code · your account", where: "your terminal", cost: "no API key needed", keys: "a · click Agent · kintsu agent",
        text: "Tests fail: three failures, one cause, found by the tiny local model. Agent writes the brief, command, output, cwd, branch, last commands, your CLAUDE.md, all redacted, and launches Claude Code in your terminal on your subscription. Kintsu sent nothing to a cloud model." }),
      S("A wall of test output. One cause. Your agent, briefed."),
      P(), T("npm test"), E(), O(...TEST_OUTPUT), W(500), TOAST(testToast), P(), W(2000), KEY("a"), W(400), OPEN("agent"), W(7200), KEY("Esc"), CLOSE(), W(800),

      CH({ id: "later", tag: "messages", title: "Later, a message arrives", who: "the daemon, then haiku", where: "local + cloud", cost: "no polling in your shell", keys: "^K opens it · arrives at the next prompt in bash",
        text: "You moved on. The resident daemon kept the case and, when the follow-up was ready, delivered it above your prompt without touching the line you were typing. Messages arrive; they never interrupt." }),
      S("You move on. The follow-up arrives above your prompt."),
      T("vim scripts/build.js"), E(), P(), W(1800), MSG(followUp), W(3800),

      CH({ id: "ignore", tag: "i · Ignore", title: "Ignore, mute, forever", who: "Kintsu", where: "local", cost: "quiet by default", keys: "i · kintsu ignore · kintsu mute 1h",
        text: "A flaky linter you already know about. Ignore has scopes: this command, this directory, this session, forever. <b>kintsu mute 1h</b> buys an hour of nothing. The same failure twice is one bubble anyway." }),
      S("Ignore: this command, this directory, this session, or forever."),
      T("make lint"), E(), O(err("src/legacy.c:12: warning treated as error: unused variable 'tmp'"), err("make: *** [Makefile:31: lint] Error 1")), W(400),
      TOAST(lintToast), P(), W(1800), KEY("i"), IGNORE(), W(1400), T("kintsu mute 1h"), E(), O(dim("quiet until 17:42")), W(3200), END(),
    ],
  };

  // ── the player ───────────────────────────────────────────────────────
  class Player {
    constructor(root) {
      this.root = root;
      this.scene = SCENES[root.dataset.scene];
      this.chapters = [];
      this.scene.forEach((st, i) => { if (st.ch) this.chapters.push({ step: i, ...st.ch }); });
      this.run = 0; this.driving = false; this.paused = false; this.instant = false; this.playing = false;
      this.current = null; this.prompt = null; this.chapterIndex = -1; this.wake = null; this.keyTimer = null;
      this.render();
    }

    render() {
      const title = esc(this.root.dataset.title);
      if (!this.chapters.length) {
        this.root.innerHTML = `
          <div class="terminal" tabindex="0" aria-label="Kintsu, live. Click a word in the bubble, or use Control K, w, f, a, i, p, Escape, Enter.">
            <div class="titlebar"><span class="dots" aria-hidden="true"><i></i><i></i><i></i></span><span class="title">${title}</span><span class="badge static">live · click a word · ^K</span></div>
            <div class="screen"></div>
            <div class="keycap" aria-hidden="true"></div>
          </div>`;
        this.term = this.root.querySelector(".terminal"); this.screen = this.root.querySelector(".screen"); this.badge = this.root.querySelector(".badge"); this.keycap = this.root.querySelector(".keycap");
        this.items = []; this.ex = null;
        this.screen.addEventListener("click", (e) => {
          const a = e.target.closest("[data-act]"); if (a) { this.act(a.dataset.act); return; }
          const t = e.target.closest("[data-tab]"); if (t) { this.takeWheel(); this.switchTab(t.dataset.tab); }
        });
        this.term.addEventListener("keydown", (e) => this.onTerminalKey(e));
        return;
      }
      const steps = this.chapters.map((c, k) => `
        <li>
          <button type="button" class="chapter" data-chapter="${k}" aria-label="Chapter ${k + 1}: ${esc(c.title)}">
            <span class="n">${k + 1}</span>
            <span class="tag">${esc(c.tag)}</span>
            <span class="t">${esc(c.title)}</span>
            <i class="fill"></i>
          </button>
        </li>`).join("");
      this.root.innerHTML = `
        <aside class="ch-panel" aria-live="polite">
          <p class="ch-count"></p>
          <h3 class="ch-title"><span class="tag"></span><span class="t"></span></h3>
          <p class="ch-text"></p>
          <p class="ch-meta"></p>
          <p class="ch-keys"></p>
        </aside>
        <div class="player-main">
          <div class="terminal" tabindex="0" aria-label="Kintsu tour. Click a word in the bubble, or use Tab, Control K, w, f, a, i, p, Escape. Space pauses.">
            <div class="titlebar"><span class="dots" aria-hidden="true"><i></i><i></i><i></i></span><span class="title">${title}</span><button type="button" class="badge playpause" aria-label="Pause">⏸ playing</button></div>
            <div class="screen"></div>
            <div class="keycap" aria-hidden="true"></div>
          </div>
        </div>
        <ol class="chapters" aria-label="Chapters: pick one to play it">${steps}</ol>`;
      const $ = (s) => this.root.querySelector(s);
      this.term = $(".terminal"); this.screen = $(".screen"); this.badge = $(".playpause"); this.keycap = $(".keycap");
      this.items = [...this.root.querySelectorAll(".chapters > li")];
      this.ex = { count: $(".ch-count"), tag: $(".ch-title .tag"), title: $(".ch-title .t"), text: $(".ch-text"), meta: $(".ch-meta"), keys: $(".ch-keys") };
      this.badge.addEventListener("click", () => this.togglePause());
      this.root.querySelectorAll(".chapter").forEach((b) => b.addEventListener("click", () => this.play(Number(b.dataset.chapter))));
      this.screen.addEventListener("click", (e) => {
        const a = e.target.closest("[data-act]"); if (a) { this.act(a.dataset.act); return; }
        const t = e.target.closest("[data-tab]"); if (t) { this.takeWheel(); this.switchTab(t.dataset.tab); }
      });
      this.term.addEventListener("keydown", (e) => this.onTerminalKey(e));
    }

    // ── timing, pause, instant ──
    stale(id) { return id !== this.run; }
    async gate(id) { while (this.paused && !this.stale(id)) await new Promise((r) => (this.wake = r)); }
    async sleep(ms, id) { await this.gate(id); if (this.instant || reduced || this.stale(id)) return; await timeout(ms); await this.gate(id); }
    setPaused(v) {
      if (this.paused === v) return;
      this.paused = v;
      this.setBadge(v ? "▶ paused" : "⏸ playing", v ? "Play" : "Pause");
      if (!v && this.wake) { const w = this.wake; this.wake = null; w(); }
    }
    togglePause() { if (!this.playing) { this.play(Math.max(0, this.chapterIndex)); return; } this.setPaused(!this.paused); }
    setBadge(text, label) { if (this.badge.classList.contains("static")) return; this.badge.textContent = text; this.badge.setAttribute("aria-label", label || text); }
    scrollDown() { this.screen.scrollTop = this.screen.scrollHeight; }
    flashKey(label) {
      if (this.instant || reduced) return;
      this.keycap.textContent = label; this.keycap.classList.add("on");
      clearTimeout(this.keyTimer); this.keyTimer = setTimeout(() => this.keycap.classList.remove("on"), 1100);
    }

    // ── chapters ──
    showChapter(k) {
      this.chapterIndex = k;
      if (!this.ex) return;
      const c = this.chapters[k];
      this.ex.count.textContent = `Chapter ${k + 1} of ${this.chapters.length}`;
      this.ex.tag.textContent = c.tag; this.ex.title.textContent = c.title;
      this.ex.text.innerHTML = c.text;
      this.ex.meta.innerHTML = `<span class="where ${whereClass(c.where)}">${esc(c.where)}</span> ${esc(c.who)} <span class="d">· ${esc(c.cost)}</span>`;
      this.ex.keys.innerHTML = c.keys.split(" · ").map((x) => `<span>${esc(x)}</span>`).join(`<span class="sep"> · </span>`);
      this.items.forEach((li, i) => { li.classList.toggle("on", i === k); li.classList.toggle("done", i < k); li.querySelector(".fill").style.setProperty("--w", i < k ? "100%" : "0%"); });
    }
    progress(i) {
      const k = this.chapterIndex; if (k < 0 || !this.items.length) return;
      const start = this.chapters[k].step, end = this.chapters[k + 1]?.step ?? this.scene.length;
      this.items[k].querySelector(".fill").style.setProperty("--w", `${Math.round(((i - start) / Math.max(1, end - start)) * 100)}%`);
    }

    // ── lines ──
    newPrompt() {
      const line = el("div", "line", `<span class="prompt">$</span> <span class="typed"></span><span class="ghost"></span><span class="cursor"></span>`);
      this.screen.appendChild(line); this.scrollDown(); this.prompt = line; return line;
    }
    async type(text, id) {
      if (!this.prompt) this.newPrompt();
      const typed = this.prompt.querySelector(".typed");
      if (this.instant || reduced) { typed.textContent += text; return; }
      for (const ch of text) { await this.gate(id); if (this.stale(id)) return; typed.textContent += ch; await timeout(34 + Math.random() * 28); }
    }
    settle() { if (!this.prompt) return; this.prompt.querySelector(".cursor")?.remove(); this.prompt.querySelector(".ghost")?.remove(); this.prompt = null; }
    out(l) { const t = typeof l === "string" ? l : l.text; const cls = typeof l === "string" ? "out" : l.cls || "out"; this.screen.appendChild(el("div", "line " + cls, esc(t))); this.scrollDown(); }

    // ── bubble ──
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
    ghost(text) { if (!this.prompt) this.newPrompt(); this.prompt.querySelector(".ghost").textContent = text; if (this.current) this.current.ghostText = text; }
    acceptGhost() {
      if (!this.prompt || !this.current?.ghostText) return false;
      this.prompt.querySelector(".typed").textContent = this.current.ghostText;
      this.prompt.querySelector(".ghost").textContent = ""; this.current.ghostText = null; return true;
    }
    expand(section, empty = false) {
      const c = this.current; if (!c) return;
      if (c.panel) { if (!empty) this.switchTab(section); return; }
      c.toast.style.display = "none";
      const p = el("div", "panel", `
        <div class="head"><span>${esc(c.head || c.cmd)}</span><span class="esc" data-act="dismiss">esc</span></div>
        <div class="tabs">${["why", "fix", "agent", "ignore", "privacy"].map((s) => `<span class="tab" data-tab="${s}">${s[0].toUpperCase() + s.slice(1)}<span class="k">${s[0]}</span></span>`).join("")}</div>
        <div class="body"></div>`);
      c.toast.after(p); c.panel = p;
      if (empty) { p.querySelectorAll(".tab").forEach((t) => t.classList.toggle("on", t.dataset.tab === section)); p.querySelector(".body").innerHTML = `<span class="d">…</span>`; }
      else this.switchTab(section);
      this.scrollDown();
    }
    collapse() { const c = this.current; if (!c?.panel) return; c.panel.remove(); c.panel = null; c.toast.style.display = ""; this.scrollDown(); }
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
        await this.sleep(Math.min(1400, chunk.replace(/<[^>]+>/g, "").length * 9 + 60), id);
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

    // ── taking the wheel ──
    takeWheel() {
      if (this.driving) return;
      this.driving = true; this.run++; this.playing = false; this.paused = false;
      this.setBadge("✋ you're driving · pick a chapter to resume", "Resume the tour");
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
    onTerminalKey(e) {
      const k = e.key;
      if (k === " ") { e.preventDefault(); this.togglePause(); return; }
      if (k === "Tab") { if (this.acceptGhost()) { e.preventDefault(); this.takeWheel(); } return; }
      if ((e.ctrlKey && k.toLowerCase() === "k") || k === "k") { e.preventDefault(); this.act("more"); return; }
      if (k === "Escape") { e.preventDefault(); this.takeWheel(); this.collapse(); return; }
      if (k === "Enter" && this.current?.panel) { e.preventDefault(); this.act("insert"); return; }
      const map = { w: "why", f: "fix", a: "agent", i: "ignore", p: "privacy" };
      if (map[k] && !e.metaKey && !e.ctrlKey && !e.altKey) { e.preventDefault(); this.act(map[k]); }
    }

    // ── playback ──
    async play(fromChapter = 0) {
      const id = ++this.run;
      this.driving = false; this.paused = false; this.playing = true; this.wake = null;
      this.screen.innerHTML = ""; this.current = null; this.prompt = null;
      this.setBadge("⏸ playing", "Pause");
      const steps = this.scene;
      const liveAt = steps.findIndex((s) => s.live);
      const start = this.chapters.length ? (this.chapters[fromChapter]?.step ?? 0) : Math.max(0, liveAt);
      for (let i = 0; i < steps.length; i++) {
        if (this.stale(id)) return;
        this.instant = i < start;
        const st = steps[i];
        if (st.ch) { this.showChapter(this.chapters.findIndex((c) => c.step === i)); await this.gate(id); if (this.stale(id)) return; }
        else if (st.p) this.newPrompt();
        else if (st.t != null) await this.type(st.t, id);
        else if (st.e) { await this.sleep(260, id); this.settle(); }
        else if (st.o) { for (const l of st.o) { if (this.stale(id)) return; this.out(l); await this.sleep(28, id); } }
        else if (st.w) await this.sleep(st.w, id);
        else if (st.s != null) { /* subtitles retired: the chapter text carries the story */ }
        else if (st.live) { /* real time from here */ }
        else if (st.key) this.flashKey(st.key);
        else if (st.toast) this.toast(st.toast);
        else if (st.ghost != null) this.ghost(st.ghost);
        else if (st.tab) this.acceptGhost();
        else if (st.open) this.expand(st.open, st.empty);
        else if (st.tabto) await this.switchTab(st.tabto);
        else if (st.close) this.collapse();
        else if (st.insert) this.insert();
        else if (st.ignore) this.ignore();
        else if (st.msg) this.message(st.msg);
        else if (st.end) {
          this.playing = false; this.instant = false;
          this.items[this.chapterIndex]?.querySelector(".fill")?.style.setProperty("--w", "100%");
          this.setBadge("↻ replay", "Replay from the first chapter");
          return;
        }
        if (!this.instant) this.progress(i);
      }
    }
  }

  const whereClass = (where) => /cloud/.test(where) && !/local/.test(where) ? "cloud" : /your/.test(where) ? "you" : /cloud/.test(where) ? "mixed" : "local";

  const roots = [...document.querySelectorAll(".player[data-scene]")];
  const players = roots.map((root) => new Player(root));
  window.kintsuPlayers = players;
  window.kintsuTour = players.find((p) => p.chapters.length) || null;
  const fromHash = () => { const m = /^#ch-(\d+)$/.exec(location.hash); return m ? Math.max(0, Math.min(8, Number(m[1]) - 1)) : 0; };
  for (const p of players) {
    const first = p.chapters.length ? fromHash() : 0;
    if (typeof IntersectionObserver === "function") {
      const io = new IntersectionObserver((entries) => { for (const en of entries) if (en.isIntersecting && !p.playing && p.chapterIndex < 0 && !p.driving) { p.play(first); io.disconnect(); } }, { threshold: 0.2 });
      io.observe(p.root);
    } else {
      p.play(first);
    }
  }
  if (window.kintsuTour) window.addEventListener("hashchange", () => window.kintsuTour.play(fromHash()));

  // ── copy buttons ─────────────────────────────────────────────────────
  document.querySelectorAll(".copy[data-copy]").forEach((b) => b.addEventListener("click", async () => {
    try { await navigator.clipboard.writeText(b.dataset.copy); b.textContent = "copied"; } catch { b.textContent = "select it"; }
    setTimeout(() => (b.textContent = "copy"), 1400);
  }));

  // ── bars grow when they scroll into view ─────────────────────────────
  const vizzes = [...document.querySelectorAll(".viz")].filter((v) => v.querySelector(".bar"));
  if (vizzes.length && typeof IntersectionObserver === "function" && !reduced) {
    vizzes.forEach((v) => v.classList.add("anim"));
    const vio = new IntersectionObserver((entries) => { for (const en of entries) if (en.isIntersecting) { en.target.classList.add("in"); vio.unobserve(en.target); } }, { threshold: 0.4 });
    vizzes.forEach((v) => vio.observe(v));
  }

  // ── the router, live: a task finds its brain ─────────────────────────
  const router = document.getElementById("router");
  if (router) {
    const ROUTES = [
      { task: "classify", note: "is this failure worth a bubble?", to: 1, why: "A tiny local model reads the output and says whether it deserves your attention. Free, on your machine." },
      { task: "quick_fix", note: "one corrected command", to: 1, why: "The tiny model tries first; if it is not confident, the router moves to the next candidate, Haiku." },
      { task: "explain", note: "why did it fail?", to: 2, why: "Routed to Haiku in the cloud with the redacted case. One line of config makes it local instead." },
      { task: "investigate", note: "fix the code", to: 3, why: "Handed to Claude Code in your terminal, on your subscription. Kintsu wrote the brief; it sends nothing itself." },
      { task: "explain", note: "a token was in the output", to: 1, why: "Redaction found a secret: sensitive_output = local_only. Every cloud candidate is refused; the tiny model answers." },
      { task: "quick_fix", note: "a typo, a rule knows", to: 0, why: "Rules win before any model is asked. Under a millisecond, offline." },
    ];
    const nodes = [...router.querySelectorAll(".node")], paths = [...router.querySelectorAll(".edge")];
    const pulse = router.querySelector(".pulse"), taskEl = document.getElementById("router-task"), noteEl = document.getElementById("router-note"), whyEl = document.getElementById("router-why");
    let r = 0, running = false;
    const show = (route) => {
      taskEl.textContent = route.task; noteEl.textContent = route.note; whyEl.textContent = route.why;
      nodes.forEach((n, i) => n.classList.toggle("on", i === route.to));
      paths.forEach((p, i) => p.classList.toggle("on", i === route.to));
      const path = paths[route.to]; const len = path.getTotalLength?.() ?? 0;
      if (!len || reduced) { const end = path.getPointAtLength?.(len) ?? { x: 230, y: 22 + 35 * route.to }; pulse.setAttribute("cx", end.x); pulse.setAttribute("cy", end.y); return; }
      const t0 = performance.now(), dur = 900;
      const step = (now) => { const k = Math.min(1, (now - t0) / dur); const pt = path.getPointAtLength(len * k); pulse.setAttribute("cx", pt.x); pulse.setAttribute("cy", pt.y); if (k < 1) requestAnimationFrame(step); };
      requestAnimationFrame(step);
    };
    const tick = () => { show(ROUTES[r % ROUTES.length]); r++; };
    tick();
    const start = () => { if (running) return; running = true; if (!reduced) router.dataset.timer = setInterval(tick, 3200); };
    if (typeof IntersectionObserver === "function") new IntersectionObserver((en) => { if (en.some((e) => e.isIntersecting)) start(); }, { threshold: 0.3 }).observe(router);
    else start();
  }
})();
