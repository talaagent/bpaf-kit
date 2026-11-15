# bpaf-kit-build

[![CI][ci-badge]][ci] [![crates.io][version-badge]][crate] [![downloads][downloads-badge]][crate] [![docs.rs][docs-badge]][docs] [![lib.rs][librs-badge]][librs] [![Rust 1.85+][msrv-badge]][msrv] [![License: Apache-2.0][license-badge]][license]

Dependency-free `build.rs` helper for Rust CLIs built on
[bpaf-kit](https://crates.io/crates/bpaf-kit): bakes the git rev, dirty
flag, build date and target into the binary, so `--version --verbose` can
report them through `bpaf_kit::build_info!()`.

```toml
[build-dependencies]
bpaf-kit-build = "0.1"
```

```rust,no_run
// build.rs
fn main() {
    bpaf_kit_build::emit();
}
```

`emit` sets these compile-time env vars, which `bpaf_kit::build_info!()`
reads with `option_env!`:

| Var | Value |
| --- | --- |
| `BPAF_KIT_GIT_REV` | full commit hash (`git rev-parse HEAD`) |
| `BPAF_KIT_GIT_DIRTY` | `true` / `false` (tracked files modified) |
| `BPAF_KIT_BUILD_DATE` | `YYYY-MM-DD` (UTC) |
| `BPAF_KIT_TARGET` | the target triple |

Each value can be forced by setting the same variable in the build
environment, which is how sandboxed builds without a `.git` directory (Nix,
vendored tarballs) pass them in. `SOURCE_DATE_EPOCH` sets the build date so
reproducible builds stay reproducible; Nix's placeholder value (1980-01-01)
leaves the date unset. `collect` returns the same values without printing.

The helper has no dependencies and runs at most three short `git`
commands, re-running only when HEAD, the index or the branch ref changes.
If `git` or the repository is missing, the value is left unset and the
binary reports it as unknown.

## License

Licensed under the [Apache License, Version 2.0][license].

[ci-badge]: https://github.com/talaagent/bpaf-kit/actions/workflows/ci.yml/badge.svg
[ci]: https://github.com/talaagent/bpaf-kit/actions/workflows/ci.yml
[version-badge]: https://img.shields.io/crates/v/bpaf-kit-build.svg
[downloads-badge]: https://img.shields.io/crates/d/bpaf-kit-build.svg
[crate]: https://crates.io/crates/bpaf-kit-build
[docs-badge]: https://docs.rs/bpaf-kit-build/badge.svg
[docs]: https://docs.rs/bpaf-kit-build
[librs-badge]: https://img.shields.io/badge/lib.rs-bpaf--kit--build-purple.svg
[librs]: https://lib.rs/crates/bpaf-kit-build
[msrv-badge]: https://img.shields.io/badge/rust-1.85%2B-orange.svg
[msrv]: https://github.com/talaagent/bpaf-kit#msrv
[license-badge]: https://img.shields.io/badge/license-Apache--2.0-blue.svg
[license]: https://github.com/talaagent/bpaf-kit/blob/main/LICENSE
