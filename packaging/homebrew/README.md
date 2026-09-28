# Homebrew

`kintsu.rb` is the formula for the tap `nathan-poncet/homebrew-kintsu`. It
is rendered by `scripts/homebrew-formula.py` from a release's
`SHA256SUMS`: one archive per platform, `bin.install` of the single
binary, `brew test` running `kintsu --version`. Edit the script, not the
formula. `brew audit --strict --online` and `brew style` pass on it
inside a tap.

## What users run

```sh
brew install nathan-poncet/kintsu/kintsu
```

macOS on Apple silicon and Intel, Linux on x86_64 and arm64 with Homebrew
on Linux. The caveats printed after the install give the hook line for
zsh, bash and fish.

## What the maintainer does once

1. Create the empty public repository `nathan-poncet/homebrew-kintsu`
   (a tap must be named `homebrew-<name>`), then put the current formula
   in it:

   ```sh
   gh release download v0.2.0 --repo nathan-poncet/kintsu --pattern SHA256SUMS
   python3 scripts/homebrew-formula.py --version 0.2.0 --sums SHA256SUMS > /path/to/homebrew-kintsu/Formula/kintsu.rb
   ```

   Commit and push it there.
2. Create a fine-grained personal access token with *Contents: read and
   write* on `homebrew-kintsu` only, and add it to this repository's
   Actions secrets as `HOMEBREW_TAP_TOKEN`.

From then on the release workflow renders the formula for every tag and
pushes it to the tap. Without the secret, the `homebrew` job renders the
formula into its summary and says it was not pushed.
