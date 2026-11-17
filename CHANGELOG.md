# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `App::cargo_subcommand(name)`: `cargo-foo foo --man` and
  `cargo-foo foo --version [--verbose]` (how `cargo foo --version` runs
  it) are answered too. `#[derive(Cli)]` sets it from
  `#[bpaf(options("foo"))]` ([#4](https://github.com/talaagent/bpaf-kit/issues/4)).

### Fixed

- `man`: pages for parsers with subcommands no longer have a stray
  `.SH <NAME>\ ` section and a second SYNOPSIS. They read NAME, SYNOPSIS,
  DESCRIPTION (the top-level options), then a section per subcommand
  ([#8](https://github.com/talaagent/bpaf-kit/issues/8)).

## [0.2.0](https://github.com/talaagent/bpaf-kit/compare/v0.1.0...v0.2.0) - 2026-11-15

### Added

- `Cmd::max_output`: keep at most N bytes of each captured stream (the
  rest is drained and dropped), with `Output::stdout_truncated` and
  `Output::stderr_truncated`.

## [0.1.0](https://github.com/talaagent/bpaf-kit/releases/tag/v0.1.0) - 2026-11-15

First release. All crates share one version.

### Added

- `App`: the CLI startup sequence (Ctrl-C handler, `--man`,
  `--version [--verbose]`, parse, logging), and `#[derive(Cli)]`
  (`bpaf-kit-macros`) generating `parser()`, `cli()` and a parser
  invariant test.
- `cmd`: subprocess builder with `set -x`-style echo, per-stream
  `Inherit`/`Capture`/`Tee`/`Null`/`Stderr` redirects, timeouts, dry run,
  and coded errors quoting the child's stderr tail.
- `interrupt`: Ctrl-C handler that restores the cursor and exits 130.
- `version`: `BuildInfo` and `build_info!()`, with a runtime override file
  (`<NAME>_VERSION_FILE` or `../share/<name>/version.env`).
- `bpaf-kit-build`: dependency-free `build.rs` helper baking in git rev,
  dirty flag, build date (honours `SOURCE_DATE_EPOCH`) and target.
- `suggest`: "did you mean" for enum-like arguments, with strum support
  (feature `strum`).
- Feature-gated `table` (comfy-table), `man` (manpages from the parser),
  `log` / `log-json` (tracing-subscriber with `-v`/`-q`), `prompt`
  (dialoguer) and `miette` (`Diagnostic` on error types).
- Nix: `lib.withVersionFile` wrapper supplying build info at run time.
- `examples/demo`, a CLI exercising every module.
- MSRV per crate: Rust 1.88 for bpaf-kit; 1.85 for bpaf-kit-macros and
  bpaf-kit-build.

