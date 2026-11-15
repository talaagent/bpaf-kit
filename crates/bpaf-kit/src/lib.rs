#![doc = include_str!("../README.md")]
#![allow(
    clippy::needless_doctest_main,
    reason = "the README shows build.rs, whose fn main is the point"
)]

extern crate alloc;

pub mod cmd;
pub mod interrupt;
#[cfg(feature = "log")]
pub mod log;
#[cfg(feature = "man")]
pub mod man;
#[cfg(feature = "prompt")]
pub mod prompt;
pub mod suggest;
#[cfg(feature = "table")]
pub mod table;
pub mod version;

use std::ffi::OsString;
use std::io::{IsTerminal, Write};

pub use bpaf;
use bpaf::OptionParser;
#[cfg(feature = "derive")]
pub use bpaf_kit_macros::Cli;

pub use crate::version::BuildInfo;

/// Whether to write ANSI colors to `stream`: it is a terminal and `NO_COLOR`
/// is unset or empty.
pub fn color_enabled(stream: &impl IsTerminal) -> bool {
    std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()) && stream.is_terminal()
}

/// The startup sequence `#[derive(Cli)]` generates, usable without the
/// derive:
///
/// ```no_run
/// use bpaf::Parser;
///
/// let parser = bpaf::long("name").argument::<String>("NAME").to_options();
/// let name: String = bpaf_kit::App::new(parser, bpaf_kit::build_info!()).run();
/// ```
///
/// [`App::run`]:
/// 1. installs the [`interrupt`] handler (ignoring a foreign one);
/// 2. answers `--man` with the manpage (feature `man`);
/// 3. answers `--version` / `--version --verbose` from [`BuildInfo`], with
///    the runtime override file applied;
/// 4. parses the command line (bpaf prints errors and help and exits);
/// 5. initializes logging (feature `log`), unless turned off.
pub struct App<T> {
    parser: OptionParser<T>,
    info: BuildInfo,
    log: Option<&'static str>,
}

impl<T> App<T> {
    /// Wrap `parser`; its `--version` text becomes the compiled-in version.
    #[must_use]
    pub fn new(parser: OptionParser<T>, info: BuildInfo) -> Self {
        let short = info.short().to_string();
        Self {
            parser: parser.version(short.as_str()),
            info,
            log: Some("warn"),
        }
    }

    /// The default log filter (`"warn"` unless set), or `None` to leave
    /// logging alone. Without the `log` feature this is ignored.
    #[must_use]
    pub fn log(mut self, default: Option<&'static str>) -> Self {
        self.log = default;
        self
    }

    /// The wrapped parser, with the version attached.
    #[must_use]
    pub fn parser(&self) -> &OptionParser<T> {
        &self.parser
    }

    /// The wrapped parser, with the version attached.
    #[must_use]
    pub fn into_parser(self) -> OptionParser<T> {
        self.parser
    }

    /// The build info.
    #[must_use]
    pub fn info(&self) -> &BuildInfo {
        &self.info
    }

    /// Run the startup sequence and return the parsed options.
    #[must_use]
    pub fn run(self) -> T {
        #[cfg(feature = "log")]
        {
            self.run_with_log_args(|_| &DEFAULT_LOG_ARGS)
        }
        #[cfg(not(feature = "log"))]
        {
            self.before_parse();
            self.parser.run()
        }
    }

    /// [`App::run`], with `-v`/`-q`/`--log-json` taken from the parsed
    /// options.
    #[cfg(feature = "log")]
    #[must_use]
    pub fn run_with_log_args(self, log_args: impl FnOnce(&T) -> &log::LogArgs) -> T {
        self.before_parse();
        let opts = self.parser.run();
        // A bad compiled-in filter is a programming error worth seeing; an
        // already-set subscriber is the program's own choice.
        if let Some(default) = self.log
            && let Err(err @ log::Error::Filter { .. }) = log::init(default, log_args(&opts))
        {
            eprintln!("warning: {err}");
        }
        opts
    }

    fn before_parse(&self) {
        let _ = interrupt::install();
        let args: Vec<OsString> = std::env::args_os().skip(1).take(3).collect();
        #[cfg(feature = "man")]
        if args.len() == 1 && args[0] == man::FLAG {
            exit_with(&man::render(&self.parser, &self.info));
        }
        if let Some(verbose) = version::version_request(&args) {
            let info = self.info.clone().with_override();
            if verbose {
                exit_with(&info.verbose().to_string());
            }
            exit_with(&format!("{} {}\n", info.name, info.short()));
        }
    }
}

#[cfg(feature = "log")]
static DEFAULT_LOG_ARGS: log::LogArgs = log::LogArgs {
    verbose: 0,
    quiet: 0,
    json: false,
};

fn exit_with(text: &str) -> ! {
    let mut out = std::io::stdout().lock();
    let code = i32::from(
        out.write_all(text.as_bytes())
            .and_then(|()| out.flush())
            .is_err(),
    );
    std::process::exit(code)
}
