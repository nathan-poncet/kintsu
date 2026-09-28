# Rendered by scripts/homebrew-formula.py from the release's SHA256SUMS;
# edit the script, not this file.
class Kintsu < Formula
  desc "When a command fails, offers to hand it to your AI agent in the terminal"
  homepage "https://nathan-poncet.github.io/kintsu/"
  license "MIT"

  livecheck do
    url :stable
    strategy :github_latest
  end

  on_macos do
    on_arm do
      url "https://github.com/nathan-poncet/kintsu/releases/download/v0.2.0/kintsu-aarch64-apple-darwin.tar.gz"
      sha256 "1d529c26a293767e95c84dfbd181092ad427172656fa82aa11c5b73f6896909a"
    end

    on_intel do
      url "https://github.com/nathan-poncet/kintsu/releases/download/v0.2.0/kintsu-x86_64-apple-darwin.tar.gz"
      sha256 "832058feef2cf3e3a306a28b762c38a92ac93878090f98650d6a4ec73ec02b70"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/nathan-poncet/kintsu/releases/download/v0.2.0/kintsu-aarch64-unknown-linux-musl.tar.gz"
      sha256 "f6739c61aeadcb48d23657622bdf1d41ce7349c9b2a5d505a6ccbf16f48ad968"
    end

    on_intel do
      url "https://github.com/nathan-poncet/kintsu/releases/download/v0.2.0/kintsu-x86_64-unknown-linux-musl.tar.gz"
      sha256 "2a8567acb147a7c13249073bf1d3c59e4dd00ac921b949a073bf32dbcf575e3b"
    end
  end

  def install
    bin.install "kintsu"
  end

  def caveats
    <<~EOS
      Add the hook to your shell, then open a new one:
        zsh   eval "$(kintsu init zsh)"    in ~/.zshrc
        bash  eval "$(kintsu init bash)"   in ~/.bashrc
        fish  kintsu init fish | source    in ~/.config/fish/config.fish
      Then `kintsu setup` picks a local model and your keys.
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/kintsu --version")
  end
end
