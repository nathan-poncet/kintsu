#!/usr/bin/env python3
"""Render the Homebrew formula of one release from its published checksums.

    python3 scripts/homebrew-formula.py --version 0.2.0 --sums SHA256SUMS > packaging/homebrew/kintsu.rb

The release workflow runs it once SHA256SUMS is on the release and pushes
the result to the tap as Formula/kintsu.rb; by hand it regenerates the
copy kept in packaging/homebrew/. Standard library only.
"""
from __future__ import annotations
import argparse, sys
from pathlib import Path

REPO = "nathan-poncet/kintsu"
# (Homebrew system block, architecture block, archive name): the four
# targets release.yml builds.
TARGETS = [
    ("on_macos", "on_arm", "kintsu-aarch64-apple-darwin.tar.gz"),
    ("on_macos", "on_intel", "kintsu-x86_64-apple-darwin.tar.gz"),
    ("on_linux", "on_arm", "kintsu-aarch64-unknown-linux-musl.tar.gz"),
    ("on_linux", "on_intel", "kintsu-x86_64-unknown-linux-musl.tar.gz"),
]


def checksums(text: str) -> dict[str, str]:
    """`sha256sum` output: one `<hex>  <name>` per line."""
    found = {}
    for line in text.splitlines():
        parts = line.split()
        if len(parts) == 2 and len(parts[0]) == 64:
            found[parts[1].lstrip("*")] = parts[0]
    return found


def render(version: str, sums: dict[str, str]) -> str:
    missing = [name for _, _, name in TARGETS if name not in sums]
    if missing:
        raise SystemExit(f"no checksum for: {', '.join(missing)}")
    blocks = []
    for system in ("on_macos", "on_linux"):
        arches = []
        for block, arch, name in TARGETS:
            if block != system:
                continue
            url = f"https://github.com/{REPO}/releases/download/v{version}/{name}"
            arches.append(
                f"    {arch} do\n"
                f'      url "{url}"\n'
                f'      sha256 "{sums[name]}"\n'
                f"    end\n"
            )
        blocks.append(f"  {system} do\n" + "\n".join(arches) + "  end\n")
    return (
        "# Rendered by scripts/homebrew-formula.py from the release's SHA256SUMS;\n"
        "# edit the script, not this file.\n"
        "class Kintsu < Formula\n"
        '  desc "When a command fails, offers to hand it to your AI agent in the terminal"\n'
        '  homepage "https://nathan-poncet.github.io/kintsu/"\n'
        '  license "MIT"\n'
        "\n"
        "  livecheck do\n"
        "    url :stable\n"
        "    strategy :github_latest\n"
        "  end\n"
        "\n" + "\n".join(blocks) + "\n"
        "  def install\n"
        '    bin.install "kintsu"\n'
        "  end\n"
        "\n"
        "  def caveats\n"
        "    <<~EOS\n"
        "      Add the hook to your shell, then open a new one:\n"
        '        zsh   eval "$(kintsu init zsh)"    in ~/.zshrc\n'
        '        bash  eval "$(kintsu init bash)"   in ~/.bashrc\n'
        "        fish  kintsu init fish | source    in ~/.config/fish/config.fish\n"
        "      Then `kintsu setup` picks a local model and your keys.\n"
        "    EOS\n"
        "  end\n"
        "\n"
        "  test do\n"
        '    assert_match version.to_s, shell_output("#{bin}/kintsu --version")\n'
        "  end\n"
        "end\n"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--version", required=True, help="the release, with or without its v")
    parser.add_argument("--sums", required=True, type=Path, help="the release's SHA256SUMS file")
    args = parser.parse_args()
    sys.stdout.write(render(args.version.lstrip("v"), checksums(args.sums.read_text())))


if __name__ == "__main__":
    main()
