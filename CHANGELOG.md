# Changelog

All notable changes to this project are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-11-15

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
- MSRV 1.88.

[Unreleased]: https://github.com/talaagent/bpaf-kit/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/talaagent/bpaf-kit/releases/tag/v0.1.0
