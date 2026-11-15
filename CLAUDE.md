@CONTRIBUTING.md

# Maintainer rules

- Run everything inside the nix devShell (`nix develop -c …`).
- Every commit must pass fmt, clippy and test. Commit straight to `main`,
  small and focused; never force-push.
- Not published to crates.io (`publish = false`).
