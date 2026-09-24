#!/usr/bin/env python3
"""Keep the documentation pages in one shell: header with search, sidebar,
"On this page" column, previous/next links, and the search index.

    python3 scripts/docs-shell.py          # rewrite the pages and docs/search-index.js
    python3 scripts/docs-shell.py --check  # exit 1 if anything would change (CI)

Static output only: nothing runs at page load except the site's own script.
"""
from __future__ import annotations
import html, json, re, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "docs"

GROUPS = [
    ("Start here", [("docs.html", "Overview"), ("install.html", "Install"), ("keys.html", "Keys and commands")]),
    ("Reference", [("configuration.html", "Configuration")]),
    ("Learn", [("faq.html", "FAQ")]),
    ("Project", [
        ("roadmap.html", "Roadmap"),
        ("https://github.com/nathan-poncet/kintsu/tree/main/docs", "Design documents"),
        ("https://github.com/nathan-poncet/kintsu/blob/main/CONTRIBUTING.md", "Contributing"),
    ]),
]
PAGES = [href for _, items in GROUPS for href, _ in items if href.endswith(".html")]
ORDER = PAGES  # previous / next follow the sidebar

def slug(text: str) -> str:
    text = re.sub(r"<[^>]+>", "", text)
    text = html.unescape(text).lower()
    text = re.sub(r"[^a-z0-9]+", "-", text).strip("-")
    return text[:60] or "section"

def strip_tags(s: str) -> str:
    return re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", " ", s))).strip()

def header(current: str) -> str:
    def nav(href, label):
        cur = ' aria-current="page"' if (href == "docs.html" and current != "roadmap.html") or href == current else ""
        return f'<a href="{href}"{cur}>{label}</a>'
    return f'''<header class="top docs-top">
  <button type="button" class="menu" aria-expanded="false" aria-controls="sidebar">Menu</button>
  <a class="brand" href="./"><img src="media/mark.svg" alt="" width="26" height="26"><span>Kintsu</span></a>
  <div class="search" role="search">
    <input type="search" id="docs-search" placeholder="Search the docs" aria-label="Search the documentation" autocomplete="off" spellcheck="false">
    <kbd aria-hidden="true">/</kbd>
    <div class="search-results" id="search-results" role="listbox" hidden></div>
  </div>
  <nav aria-label="Site">
    {nav("docs.html", "Docs")}
    {nav("roadmap.html", "Roadmap")}
    <a class="gh" href="https://github.com/nathan-poncet/kintsu" target="_blank" rel="noopener">GitHub</a>
  </nav>
</header>'''

def sidebar(current: str) -> str:
    out = ['<nav class="sidebar" id="sidebar" aria-label="Documentation">']
    for title, items in GROUPS:
        out.append(f'  <details open>\n    <summary>{title}</summary>\n    <ul>')
        for href, label in items:
            ext = href.startswith("http")
            cur = ' aria-current="page"' if href == current else ""
            extra = ' target="_blank" rel="noopener"' if ext else ""
            arrow = ' <span class="ext" aria-hidden="true">↗</span>' if ext else ""
            out.append(f'      <li><a href="{href}"{cur}{extra}>{label}{arrow}</a></li>')
        out.append('    </ul>\n  </details>')
    out.append('</nav>')
    return "\n".join(out)

def prev_next(current: str) -> str:
    labels = {href: label for _, items in GROUPS for href, label in items}
    i = ORDER.index(current)
    parts = []
    if i > 0:
        parts.append(f'<a class="prev" href="{ORDER[i-1]}"><span>Previous</span><b>{labels[ORDER[i-1]]}</b></a>')
    if i < len(ORDER) - 1:
        parts.append(f'<a class="next" href="{ORDER[i+1]}"><span>Next</span><b>{labels[ORDER[i+1]]}</b></a>')
    return '<footer class="pager">' + "".join(parts) + '</footer>'

def ensure_ids(main_html: str) -> str:
    """A section with an id keeps it and its h2 carries none (no duplicate ids);
    a section without one gets a slug id on its first h2."""
    used = set(re.findall(r'id="([^"]+)"', main_html))
    def fix_section(m):
        sec_open, body = m.group(1), m.group(2)
        sec_id = re.search(r'id="([^"]+)"', sec_open)
        def fix_h2(h):
            attrs, text = h.group(1), h.group(2)
            if sec_id:
                attrs = re.sub(r'\s+id="[^"]*"', "", attrs) if f'id="{sec_id.group(1)}"' in attrs else attrs
                return f"<h2{attrs}>{text}</h2>"
            if 'id="' in attrs: return h.group(0)
            new, base, n = slug(text), slug(text), 2
            while new in used: new = f"{base}-{n}"; n += 1
            used.add(new)
            return f'<h2{attrs} id="{new}">{text}</h2>'
        return f'<section{sec_open}>{re.sub(r"<h2([^>]*)>(.*?)</h2>", fix_h2, body, count=1, flags=re.S)}</section>'
    return re.sub(r'<section([^>]*)>(.*?)</section>', fix_section, main_html, flags=re.S)

def index_page(href: str, main_html: str) -> list[dict]:
    title = strip_tags(re.search(r'<h1[^>]*>(.*?)</h1>', main_html, re.S).group(1))
    lede_m = re.search(r'<p class="lede">(.*?)</p>', main_html, re.S)
    entries = [{"page": href, "title": title, "heading": "", "id": "", "text": strip_tags(lede_m.group(1)) if lede_m else ""}]
    for sec in re.finditer(r'<section([^>]*)>(.*?)</section>', main_html, re.S):
        attrs, body = sec.group(1), sec.group(2)
        h = re.search(r'<h2([^>]*)>(.*?)</h2>', body, re.S)
        if not h: continue
        hid = re.search(r'id="([^"]+)"', h.group(1)) or re.search(r'id="([^"]+)"', attrs)
        if not hid: continue
        hid = hid.group(1)
        text = strip_tags(re.sub(r'<pre>.*?</pre>', ' ', body, flags=re.S))
        text = text.replace(strip_tags(h.group(2)), "", 1).strip()
        entries.append({"page": href, "title": title, "heading": strip_tags(h.group(2)), "id": hid, "text": text[:320]})
        # FAQ questions and reference rows deserve their own hits
        for q in re.finditer(r'<summary>(.*?)</summary>\s*<p>(.*?)</p>', body, re.S):
            entries.append({"page": href, "title": title, "heading": strip_tags(q.group(1)), "id": hid, "text": strip_tags(q.group(2))[:240]})
    return entries

def shell(page: Path) -> tuple[str, list[dict]]:
    src = page.read_text()
    current = page.name
    # the inner <main>, whether already wrapped or not
    m = re.search(r'<main id="main"[^>]*>.*?</main>', src, re.S)
    assert m, page
    main_html = ensure_ids(m.group(0))
    main_html = re.sub(r'\s*<footer class="pager">.*?</footer>\s*(?=</main>)', "", main_html, flags=re.S)
    main_html = re.sub(r'\s*</main>\s*$', "</main>", main_html)
    main_html = main_html.replace("</main>", f"\n  {prev_next(current)}\n</main>")
    layout = f'<!-- docs-layout -->\n<div class="docs-layout">\n{sidebar(current)}\n{main_html}\n<aside class="toc" aria-label="On this page"><p class="toc-title">On this page</p><ol id="toc"></ol></aside>\n</div>\n<!-- /docs-layout -->'
    # replace an existing layout, or the bare main
    if "<!-- docs-layout -->" in src:
        out = re.sub(r'<!-- docs-layout -->.*?<!-- /docs-layout -->', lambda _: layout, src, flags=re.S)
    else:
        out = src.replace(m.group(0), layout)
    out = re.sub(r'<header class="top[^"]*">.*?</header>', lambda _: header(current), out, flags=re.S)
    if 'body class="page"' in out: out = out.replace('body class="page"', 'body class="page docs"')
    if "search-index.js" not in out:
        tags = '<script src="search-index.js" defer></script>\n<script src="script.js" defer></script>'
        out = out.replace('<script src="script.js" defer></script>', tags) if 'src="script.js"' in out else out.replace("</body>", tags + "\n</body>")
    return out, index_page(current, main_html)

def main(check: bool) -> int:
    changed = []
    entries: list[dict] = []
    for href in PAGES:
        page = ROOT / href
        out, idx = shell(page)
        entries += idx
        if out != page.read_text():
            changed.append(href)
            if not check: page.write_text(out)
    index_js = "window.KINTSU_DOCS_INDEX = " + json.dumps(entries, ensure_ascii=False, separators=(",", ":")) + ";\n"
    idx_path = ROOT / "search-index.js"
    if not idx_path.exists() or idx_path.read_text() != index_js:
        changed.append("search-index.js")
        if not check: idx_path.write_text(index_js)
    if check and changed:
        print("docs shell is stale; run scripts/docs-shell.py:", ", ".join(changed)); return 1
    print(("would change: " if check else "updated: ") + (", ".join(changed) or "nothing") + f" · {len(entries)} search entries")
    return 0

if __name__ == "__main__":
    sys.exit(main("--check" in sys.argv))
