# bpaf-kit-macros

[![CI][ci-badge]][ci] [![crates.io][version-badge]][crate] [![downloads][downloads-badge]][crate] [![docs.rs][docs-badge]][docs] [![lib.rs][librs-badge]][librs] [![Rust 1.85+][msrv-badge]][msrv] [![License: Apache-2.0][license-badge]][license]

`#[derive(Cli)]` for [bpaf-kit](https://crates.io/crates/bpaf-kit), on top
of bpaf's own `#[derive(Bpaf)]`. Use it through `bpaf_kit::Cli` (feature
`derive`, on by default) rather than depending on this crate directly.

```rust,ignore
#[derive(Debug, Clone, bpaf::Bpaf, bpaf_kit::Cli)]
#[bpaf(options)]
#[cli(log = "info")]
struct Opts {
    #[bpaf(external(bpaf_kit::log::log_args))]
    #[cli(log_args)]
    log: bpaf_kit::log::LogArgs,
}

fn main() {
    let opts = Opts::cli();
}
```

It adds `Opts::parser()` (the bpaf parser with the compiled-in version)
and `Opts::cli()` (Ctrl-C handler, `--man`, `--version [--verbose]`,
parse, logging), plus a `#[cfg(test)]` test that runs bpaf's
`check_invariants` on the parser. See the
[`Cli` docs](https://docs.rs/bpaf-kit-macros/latest/bpaf_kit_macros/derive.Cli.html)
for the `#[cli(...)]` attributes.

## License

Licensed under the [Apache License, Version 2.0][license].

[ci-badge]: https://github.com/talaagent/bpaf-kit/actions/workflows/ci.yml/badge.svg
[ci]: https://github.com/talaagent/bpaf-kit/actions/workflows/ci.yml
[version-badge]: https://img.shields.io/crates/v/bpaf-kit-macros.svg
[downloads-badge]: https://img.shields.io/crates/d/bpaf-kit-macros.svg
[crate]: https://crates.io/crates/bpaf-kit-macros
[docs-badge]: https://docs.rs/bpaf-kit-macros/badge.svg
[docs]: https://docs.rs/bpaf-kit-macros
[librs-badge]: https://img.shields.io/badge/lib.rs-bpaf--kit--macros-purple.svg
[librs]: https://lib.rs/crates/bpaf-kit-macros
[msrv-badge]: https://img.shields.io/badge/rust-1.85%2B-orange.svg
[msrv]: https://github.com/talaagent/bpaf-kit#msrv
[license-badge]: https://img.shields.io/badge/license-Apache--2.0-blue.svg
[license]: https://github.com/talaagent/bpaf-kit/blob/main/LICENSE
