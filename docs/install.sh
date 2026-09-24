#!/bin/sh
# Kintsu installer — https://github.com/nathan-poncet/kintsu
#
#   curl -fsSL https://nathan-poncet.github.io/kintsu/install.sh | sh
#
# Puts one static binary in ~/.local/bin (or --dir), from the latest GitHub
# release when there is one, otherwise from source with cargo; offers to add
# the one-line hook to your shell and to set up a local model (Ollama with
# qwen2.5-coder:7b, about 4.7 GB) so fixes and explanations never leave your
# machine; writes a default configuration if you have none. Nothing else.
#
# Options
#   --dir DIR        install directory            (default ~/.local/bin, or $KINTSU_INSTALL_DIR)
#   --version vX.Y.Z a specific release            (default latest, or $KINTSU_VERSION)
#   --from-source    skip the release, build with cargo
#   --no-hook        do not touch your shell configuration
#   --no-model       do not install Ollama or pull the local model   (or KINTSU_NO_MODEL=1)
#   --yes            answer yes to every question  (or KINTSU_YES=1)
#   --dry-run        print what would happen
#   --uninstall      remove the binary and the hook line
#   --help
set -eu

REPO="nathan-poncet/kintsu"
DIR="${KINTSU_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${KINTSU_VERSION:-latest}"
FROM_SOURCE=0; HOOK=1; MODEL=1; YES="${KINTSU_YES:-}"; DRY=0; UNINSTALL=0
[ -n "${KINTSU_NO_MODEL:-}" ] && MODEL=0
LOCAL_MODEL="qwen2.5-coder:7b"

if [ -t 2 ]; then GOLD="$(printf '\033[33m')"; DIM="$(printf '\033[2m')"; RST="$(printf '\033[0m')"; else GOLD=""; DIM=""; RST=""; fi
say()  { printf '%s▎%s %s\n' "$GOLD" "$RST" "$*" >&2; }
note() { printf '%s▎ %s%s\n' "$GOLD" "$DIM$*" "$RST" >&2; }
die()  { printf '%s▎%s %s\n' "$GOLD" "$RST" "$*" >&2; exit 1; }
run()  { if [ "$DRY" = 1 ]; then note "would run: $*"; else "$@"; fi; }

usage() { sed -n '2,23p' "$0" | sed 's/^# \{0,1\}//'; exit 0; }

while [ $# -gt 0 ]; do
  case "$1" in
    --dir) DIR="$2"; shift ;;
    --dir=*) DIR="${1#--dir=}" ;;
    --version) VERSION="$2"; shift ;;
    --version=*) VERSION="${1#--version=}" ;;
    --from-source) FROM_SOURCE=1 ;;
    --no-hook) HOOK=0 ;;
    --no-model) MODEL=0 ;;
    --yes|-y) YES=1 ;;
    --dry-run) DRY=1 ;;
    --uninstall) UNINSTALL=1 ;;
    -h|--help) usage ;;
    *) die "unknown option: $1 (try --help)" ;;
  esac
  shift
done

# ── platform ──────────────────────────────────────────────────────────────────
os="$(uname -s)"; arch="$(uname -m)"
case "$arch" in arm64|aarch64) arch=aarch64 ;; x86_64|amd64) arch=x86_64 ;; *) die "unsupported architecture: $arch" ;; esac
case "$os" in
  Darwin) target="$arch-apple-darwin" ;;
  Linux)  target="$arch-unknown-linux-musl" ;;
  *) die "unsupported system: $os (Linux and macOS today; see the roadmap for the rest)" ;;
esac

# ── shell hook ────────────────────────────────────────────────────────────────
shell_name="$(basename "${SHELL:-sh}")"
# shellcheck disable=SC2016  # the hook lines must reach the rc file unexpanded
case "$shell_name" in
  zsh)  rc="${ZDOTDIR:-$HOME}/.zshrc"; hook_line='eval "$(kintsu init zsh)"' ;;
  bash) rc="$HOME/.bashrc"; hook_line='eval "$(kintsu init bash)"' ;;
  fish) rc="${XDG_CONFIG_HOME:-$HOME/.config}/fish/config.fish"; hook_line='kintsu init fish | source' ;;
  *)    rc=""; hook_line="" ;;
esac

# ── uninstall ─────────────────────────────────────────────────────────────────
if [ "$UNINSTALL" = 1 ]; then
  if [ -f "$DIR/kintsu" ]; then run rm -f "$DIR/kintsu"; say "removed $DIR/kintsu"; else note "no binary at $DIR/kintsu"; fi
  if [ "$HOOK" = 1 ] && [ -n "$rc" ] && [ -f "$rc" ] && grep -q 'kintsu init' "$rc"; then
    if [ "$DRY" = 1 ]; then note "would remove the kintsu line from $rc"; else
      tmp="$(mktemp)"; grep -v 'kintsu init' "$rc" > "$tmp" || true; cat "$tmp" > "$rc"; rm -f "$tmp"; say "removed the hook from $rc"
    fi
  fi
  say "Kintsu is gone. Nothing else was touched."
  exit 0
fi

# ── install ───────────────────────────────────────────────────────────────────
have() { command -v "$1" >/dev/null 2>&1; }
have curl || die "curl is needed"
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT INT TERM
run mkdir -p "$DIR"

installed=""
if [ "$FROM_SOURCE" = 0 ]; then
  if [ "$VERSION" = latest ]; then base="https://github.com/$REPO/releases/latest/download"; else base="https://github.com/$REPO/releases/download/$VERSION"; fi
  asset="kintsu-$target.tar.gz"
  say "looking for a release for ${target}..."
  if curl -fsSL -o "$tmp/$asset" "$base/$asset" 2>/dev/null; then
    if curl -fsSL -o "$tmp/SHA256SUMS" "$base/SHA256SUMS" 2>/dev/null; then
      expected="$(grep " $asset\$" "$tmp/SHA256SUMS" | cut -d' ' -f1)"
      if have sha256sum; then actual="$(sha256sum "$tmp/$asset" | cut -d' ' -f1)"; else actual="$(shasum -a 256 "$tmp/$asset" | cut -d' ' -f1)"; fi
      if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then die "checksum mismatch for $asset; refusing to install"; fi
      note "checksum verified"
    else
      note "no SHA256SUMS published for this release; installing unverified"
    fi
    tar xzf "$tmp/$asset" -C "$tmp"
    run install -m 755 "$tmp/kintsu" "$DIR/kintsu"
    installed="release"
  else
    note "no prebuilt release yet (Kintsu is pre-alpha): building from source"
  fi
fi

if [ -z "$installed" ]; then
  if ! have cargo; then
    say "building from source needs a Rust toolchain, and none was found."
    note "install one with:  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    note "then run this installer again."
    exit 1
  fi
  say "building Kintsu with cargo (a few seconds)..."
  if [ "$DRY" = 1 ]; then note "would run: cargo install --git https://github.com/$REPO --locked --root $tmp/root"; else
    cargo install --git "https://github.com/$REPO" --locked --root "$tmp/root" --quiet
    install -m 755 "$tmp/root/bin/kintsu" "$DIR/kintsu"
  fi
  installed="source"
fi

if [ "$DRY" = 1 ]; then version="(dry run)"; else version="$("$DIR/kintsu" --version 2>/dev/null || echo kintsu)"; fi
say "$version installed to $DIR/kintsu ($installed)"

case ":$PATH:" in
  *":$DIR:"*) ;;
  *) note "$DIR is not on your PATH yet; add:  export PATH=\"$DIR:\$PATH\"" ;;
esac

# ── hook ──────────────────────────────────────────────────────────────────────
if [ "$HOOK" = 1 ]; then
  if [ -z "$rc" ]; then
    note "shell '$shell_name' is not supported yet (zsh, bash, fish); see https://github.com/$REPO#install"
  elif [ -f "$rc" ] && grep -q 'kintsu init' "$rc"; then
    note "the hook is already in $rc"
  else
    add=""
    if [ -n "$YES" ]; then add=1
    elif ( : </dev/tty ) 2>/dev/null; then   # a real terminal, not just a /dev/tty node
      printf '%s▎%s add  %s  to %s? [Y/n] ' "$GOLD" "$RST" "$hook_line" "$rc" >/dev/tty 2>/dev/null || true
      ans=""; read -r ans </dev/tty 2>/dev/null || ans=n
      case "$ans" in ""|y|Y|yes|YES) add=1 ;; esac
    fi
    if [ -n "$add" ]; then
      if [ "$DRY" = 1 ]; then note "would append the hook to $rc"; else
        mkdir -p "$(dirname "$rc")"
        printf '\n# kintsu: a bubble under failed commands\n%s\n' "$hook_line" >> "$rc"
        say "hook added to $rc"
      fi
    else
      say "to hook your shell, add this line to $rc:"
      note "$hook_line"
    fi
  fi
fi

# ── local model ───────────────────────────────────────────────────────────────
# Asks once. A yes installs Ollama when it is missing and pulls the model the
# default configuration routes for fixes and explanations.
ask() {  # ask "question"  → 0 when yes (or --yes), 1 otherwise; no terminal means no
  if [ -n "$YES" ]; then return 0; fi
  if ( : </dev/tty ) 2>/dev/null; then
    printf '%s▎%s %s [Y/n] ' "$GOLD" "$RST" "$1" >/dev/tty 2>/dev/null || true
    ans=""; read -r ans </dev/tty 2>/dev/null || ans=n
    case "$ans" in ""|y|Y|yes|YES) return 0 ;; esac
  fi
  return 1
}
if [ "$MODEL" = 1 ]; then
  if have ollama; then
    say "Ollama is installed."
  elif ask "install Ollama and pull $LOCAL_MODEL (about 4.7 GB) so fixes and explanations run on this machine?"; then
    case "$os" in
      Darwin)
        if have brew; then run brew install ollama; run brew services start ollama
        else note "Homebrew is missing: install Ollama from https://ollama.com/download, then run:  ollama pull $LOCAL_MODEL"; MODEL=0; fi ;;
      Linux)
        if [ "$DRY" = 1 ]; then note "would run: curl -fsSL https://ollama.com/install.sh | sh"; else curl -fsSL https://ollama.com/install.sh | sh; fi ;;
    esac
  else
    note "skipping the local model; later:  ollama pull $LOCAL_MODEL"; MODEL=0
  fi
  if [ "$MODEL" = 1 ] && { have ollama || [ "$DRY" = 1 ]; }; then
    if [ "$DRY" = 1 ]; then note "would run: ollama pull $LOCAL_MODEL"
    elif ollama list 2>/dev/null | grep -q "^$LOCAL_MODEL"; then note "$LOCAL_MODEL is already pulled"
    else
      say "pulling $LOCAL_MODEL (about 4.7 GB, once)..."
      ollama pull "$LOCAL_MODEL" || note "the pull failed; later:  ollama pull $LOCAL_MODEL"
    fi
  fi
fi

# ── configuration ─────────────────────────────────────────────────────────────
config="${KINTSU_CONFIG:-${XDG_CONFIG_HOME:-$HOME/.config}/kintsu/config.toml}"
if [ -f "$config" ]; then
  note "keeping your configuration at $config"
elif [ "$DRY" = 1 ]; then
  note "would write the default configuration to $config"
else
  mkdir -p "$(dirname "$config")" && "$DIR/kintsu" default-config > "$config" && say "default configuration written to $config"
fi

say "done. Open a new shell; the next failure gets a bubble."
note "remove everything later with:  sh install.sh --uninstall   (or the same curl | sh -s -- --uninstall)"
