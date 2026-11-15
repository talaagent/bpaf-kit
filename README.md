# bpaf-kit

Batteries for Rust CLIs built on [bpaf](https://docs.rs/bpaf): the startup
code and helpers every command-line tool ends up writing.

| crate             | what it is                                                      |
|-------------------|-----------------------------------------------------------------|
| `bpaf-kit`        | the library                                                     |
| `bpaf-kit-macros` | `#[derive(Cli)]` (re-exported as `bpaf_kit::Cli`)               |
| `bpaf-kit-build`  | dependency-free `build.rs` helper that bakes in git/build info  |

Not on crates.io; depend on it by git:

```toml
[dependencies]
bpaf = { version = "0.9", features = ["derive"] }
bpaf-kit = { git = "https://github.com/talaagent/bpaf-kit", features = ["log", "man"] }

[build-dependencies]
bpaf-kit-build = { git = "https://github.com/talaagent/bpaf-kit" }
```

Needs Rust 1.88 or newer.

## Quick start

```rust,no_run
# #[cfg(all(feature = "derive", feature = "log"))] mod quick_start {
use bpaf::Bpaf;
use bpaf_kit::{Cli, cmd::Cmd, log::LogArgs};

/// Deploys things
#[derive(Debug, Clone, Bpaf, Cli)]
#[bpaf(options)]
#[cli(log = "warn")]
struct Opts {
    #[bpaf(external(bpaf_kit::log::log_args))]
    #[cli(log_args)]
    log: LogArgs,
    /// Environment to deploy to
    #[bpaf(positional("ENV"))]
    env: String,
}

fn main() -> Result<(), bpaf_kit::cmd::Error> {
    let opts = Opts::cli();
    Cmd::new("git").args(["push", "origin", opts.env.as_str()]).run()?;
    Ok(())
}
# }
# fn main() {}
```

```rust,no_run
// build.rs
fn main() {
    bpaf_kit_build::emit();
}
```

`Opts::cli()` installs the Ctrl-C handler, answers `--man` and
`--version [--verbose]`, parses the command line, and sets up logging from
`-v`/`-q`/`RUST_LOG`. The derive also adds a `#[cfg(test)]` test that runs
bpaf's `check_invariants` on the parser, so a badly-shaped parser fails
`cargo test` instead of the first user. Without the derive, the same
sequence is `bpaf_kit::App::new(parser, bpaf_kit::build_info!()).run()`.

[`examples/demo`](examples/demo/src/main.rs) is a small CLI that uses every
module, and its [tests](examples/demo/tests/cli.rs) show the behaviour end
to end.

## Modules

### `cmd`: running subprocesses

```rust,no_run
use bpaf_kit::cmd::{Cmd, Redirect};
use std::time::Duration;
# fn main() -> Result<(), bpaf_kit::cmd::Error> {

Cmd::new("cargo").args(["build", "--release"]).run()?;            // + cargo build --release
let rev = Cmd::new("git").args(["rev-parse", "HEAD"]).capture_stdout()?;
let out = Cmd::new("make")
    .arg("check")
    .current_dir("vendor/lib")
    .env("CI", "1")
    .stdout(Redirect::Tee)          // show it and keep it
    .timeout(Duration::from_secs(60))
    .allow_failure()                // non-zero exit is an Output, not an Err
    .output()?;
# Ok(()) }
```

- Every command is echoed to stderr first, `set -x` style:
  `+ cd vendor/lib && CI=1 make check`. Off with `.echo(false)`,
  `cmd::set_echo(false)`, or `BPAF_KIT_ECHO=0`.
- Per stream: `Inherit`, `Capture`, `Tee` (both), `Null`, `Stderr` (a
  child's stdout onto our stderr, for tools whose stdout is `eval`'d).
  `run()` inherits, `output()` and `capture_stdout()` capture.
- A non-zero exit is `Error::Exit` with the last 4 KiB of the child's stderr
  (when captured or tee'd). Every error has a stable `code()`: `cmd_spawn`,
  `cmd_io`, `cmd_exit`, `cmd_timeout`, `cmd_utf8`.
- Dry run (`cmd::set_dry_run(true)`, `BPAF_KIT_DRY_RUN=1`, `.dry_run(true)`)
  echoes without running; `.dry_run(false)` exempts read-only commands.
- Timeouts kill the child; a grandchild holding the pipes open can't hang
  the caller.
- No extra thread unless both streams are piped (or there's a timeout).

### `interrupt`: Ctrl-C

`interrupt::install()` (idempotent) shows the cursor again and exits 130 on
Ctrl-C, so interrupting a dialoguer prompt doesn't leave the terminal
without a cursor. `cli()` and the `prompt` helpers install it for you.

### `version`: build info

`bpaf_kit::build_info!()` reads, at the caller's compile time, the package
version and what `bpaf-kit-build` baked in: git rev, dirty flag, build date
(UTC, honours `SOURCE_DATE_EPOCH`), target. Each can be forced through the
build environment (`BPAF_KIT_GIT_REV`, `BPAF_KIT_GIT_DIRTY`,
`BPAF_KIT_BUILD_DATE`, `BPAF_KIT_TARGET`).

```text
$ demo --version
demo 0.1.0 (c5d32b87e 2026-11-12)
$ demo --version --verbose
demo 0.1.0 (c5d32b87e 2026-11-12)
version:    0.1.0
commit:     c5d32b87ebfa84c6bfb6299a24b906083cfeff38
dirty:      no
build date: 2026-11-12
target:     x86_64-unknown-linux-gnu
info from:  /nix/store/…-demo-0.1.0/share/demo/version.env
```

#### Nix wrapper contract

Compiling the git rev into a binary makes every commit a cache miss. With
Nix, build the binary without it and let a tiny wrapper supply it at run
time. When the version is asked for (never on a normal start), bpaf-kit
looks for a version file:

1. at `$<NAME>_VERSION_FILE`, `<NAME>` being the binary name upper-cased
   with non-alphanumerics as `_` (`my-tool` → `MY_TOOL_VERSION_FILE`).
   The name is per binary so it doesn't leak into child processes that are
   bpaf-kit tools too;
2. else at `<dir of the executable>/../share/<name>/version.env`.

It holds `key=value` lines, all optional, overriding the compiled values:

```text
version=0.4.1
rev=c5d32b87ebfa84c6bfb6299a24b906083cfeff38
dirty=false
date=2026-11-12
target=x86_64-unknown-linux-gnu
```

This flake exports a wrapper ([`nix/with-version-file.nix`](nix/with-version-file.nix)):

```nix
{
  inputs.bpaf-kit.url = "github:talaagent/bpaf-kit";
  outputs = { self, nixpkgs, bpaf-kit, ... }: let
    pkgs = nixpkgs.legacyPackages.x86_64-linux;
    my-tool-unwrapped = pkgs.rustPlatform.buildRustPackage { /* ... */ };
  in {
    packages.x86_64-linux.default = bpaf-kit.lib.withVersionFile pkgs (
      { package = my-tool-unwrapped; } // bpaf-kit.lib.flakeVersionArgs self
    );
  };
}
```

or by hand:

```nix
let
  d = self.lastModifiedDate;
  versionFile = pkgs.writeText "my-tool-version.env" ''
    rev=${self.rev or "unknown"}
    dirty=${if self ? dirtyRev then "true" else "false"}
    date=${builtins.substring 0 4 d}-${builtins.substring 4 2 d}-${builtins.substring 6 2 d}
  '';
in
pkgs.runCommand "my-tool" { nativeBuildInputs = [ pkgs.makeWrapper ]; } ''
  install -Dm644 ${versionFile} $out/share/my-tool/version.env
  makeWrapper ${my-tool-unwrapped}/bin/my-tool $out/bin/my-tool \
    --set MY_TOOL_VERSION_FILE $out/share/my-tool/version.env
''
```

`nix build .#demo` here builds the demo that way. Under Nix, `.git` is
absent, so `bpaf-kit-build` leaves the rev unset, and Nix's placeholder
`SOURCE_DATE_EPOCH` (1980-01-01) leaves the date unset.

### `suggest`: did you mean

```rust
use bpaf_kit::suggest;

suggest::did_you_mean("stauts", ["status", "stash"]);          // Some("status")
suggest::parse_choice("format", "jsno", &[("json", 1), ("yaml", 2)]);
// Err: unknown format 'jsno', did you mean 'json'? (valid: json, yaml)
# assert_eq!(suggest::did_you_mean("stauts", ["status", "stash"]), Some("status"));
# assert_eq!(
#     suggest::parse_choice("format", "jsno", &[("json", 1), ("yaml", 2)]).unwrap_err().to_string(),
#     "unknown format 'jsno', did you mean 'json'? (valid: json, yaml)",
# );
```

With the `strum` feature, any `#[derive(EnumString, VariantNames)]` enum:

```rust
# #[cfg(feature = "strum")] {
# #[derive(Debug, Clone, strum::EnumString, strum::VariantNames)]
# #[strum(serialize_all = "kebab-case")]
# enum OutputFormat { Json, PrettyTable }
# #[derive(Debug, Clone, bpaf::Bpaf)]
# #[bpaf(options)]
# struct Opts {
#[bpaf(long, argument::<String>("FORMAT"), parse(bpaf_kit::suggest::bpaf_enum))]
format: OutputFormat,
# }
# let err = opts().run_inner(&["--format", "pretty-tabel"]).unwrap_err().unwrap_stderr();
# assert!(err.contains("did you mean 'pretty-table'?"), "{err}");
# }
// $ tool --format pretty-tabel
// Error: couldn't parse `pretty-tabel`: unknown output format 'pretty-tabel',
//        did you mean 'pretty-table'? (valid: json, pretty-table)
```

### `table` (feature `table`)

`table::new(headers)`: thin box borders, wraps to the terminal.
`table::plain(headers)`: no borders, for pipes. `table::auto` picks by
whether stdout is a terminal. `comfy_table` is re-exported.

### `man` (feature `man`)

`tool --man` prints a roff manpage rendered from the bpaf parser
(`man::render` to do it yourself): `tool --man > share/man/man1/tool.1`.

### `log` (feature `log`, `log-json`)

`log::log_args()` adds `-v`/`--verbose` and `-q`/`--quiet` (repeatable) and,
with `log-json`, `--log-json`. `log::init(default, &args)` installs a
tracing subscriber on stderr printing `level: message key=value`. The filter
is the default level moved one step per `-v`/`-q` when given, else
`RUST_LOG`, else the default (any `EnvFilter` directive).

### `prompt` (feature `prompt`)

`prompt::text`, `prompt::optional_text` (blank is `None`), `prompt::confirm`
on dialoguer, each installing the Ctrl-C handler first.

## Features

| feature    | default | adds                                              |
|------------|---------|---------------------------------------------------|
| `derive`   | yes     | `#[derive(Cli)]`                                  |
| `table`    |         | `table`, comfy-table                              |
| `man`      |         | `man`, `--man` (bpaf `docgen`)                    |
| `log`      |         | `log`, tracing-subscriber                         |
| `log-json` |         | `--log-json` (serde_json)                         |
| `strum`    |         | `suggest::parse_enum` / `bpaf_enum`               |
| `prompt`   |         | `prompt`, dialoguer                               |
| `miette`   |         | `miette::Diagnostic` on error types (same codes)  |

The core (`cmd`, `interrupt`, `version`, `suggest`) depends on bpaf, ctrlc,
strsim and thiserror only.

## Development

```sh
nix develop -c cargo fmt
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo test
nix build .#demo && ./result/bin/demo --version --verbose
```

## License

Apache-2.0. Copyright 2026 Ian Macalinao.
