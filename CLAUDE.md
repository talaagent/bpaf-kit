# bpaf-kit

Batteries for Rust CLIs on bpaf: `crates/bpaf-kit` (library),
`crates/bpaf-kit-macros` (`#[derive(Cli)]`), `crates/bpaf-kit-build`
(zero-dependency `build.rs` helper), `examples/demo` (a CLI using every
module; its `tests/cli.rs` are the end-to-end tests). README.md is the
user documentation.

## Commands

Run everything inside the nix devShell (pinned rustc, see
`rust-toolchain.toml`):

```sh
nix develop -c cargo fmt
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo test
nix develop -c cargo clippy -p bpaf-kit --no-default-features --all-targets -- -D warnings
nix build .#demo          # checks the Nix version-file wrapper
```

Every commit must pass fmt, clippy and test. Commit straight to `main`,
small and focused; never force-push. Not published to crates.io.

## Layout

- `crates/bpaf-kit/src/lib.rs`: `App` (the startup sequence the derive
  calls: interrupt, `--man`, `--version`, parse, logging) and
  `color_enabled`.
- `cmd.rs`: `Cmd` builder, `Redirect`, `Output`, coded `Error`, global
  echo/dry-run flags (`BPAF_KIT_ECHO`, `BPAF_KIT_DRY_RUN`).
- `version.rs`: `BuildInfo`, `build_info!`, the runtime override file
  (`<NAME>_VERSION_FILE` or `../share/<name>/version.env`).
- `suggest.rs`, `interrupt.rs`, and feature-gated `table.rs`, `man.rs`,
  `log.rs`, `prompt.rs`.
- `nix/with-version-file.nix`: the wrapper; `flake.nix` exports it as
  `lib.withVersionFile` and builds `packages.demo` with it.

## Conventions

- Library errors use thiserror, carry a stable snake_case `code()`, and
  derive `miette::Diagnostic` with the same code behind the `miette`
  feature. Optional dependencies stay behind features; the core is bpaf,
  ctrlc, strsim and thiserror.
- Generated code (`bpaf-kit-macros`) refers to `::bpaf_kit::…` only, so
  users need no extra dependencies; anything feature-dependent goes through
  `App`, which compiles either way.
- Mind startup cost: nothing reads files or spawns processes on a normal
  start (the version file is read only for `--version`).
- No `unwrap` outside tests; `core` paths over `std` where they exist.
