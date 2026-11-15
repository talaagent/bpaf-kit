//! tracing-subscriber setup for CLIs (feature `log`).
//!
//! Output goes to stderr as `level: message key=value`, no timestamps or
//! targets, colored on a terminal (`NO_COLOR` turns that off). With the
//! `log-json` feature, `--log-json` switches to one JSON object per line.
//!
//! The filter comes from, in order:
//! 1. `-v`/`-q` when given: the default level moved up or down one step per
//!    flag (`-vv` from `warn` is `debug`; `-q` from `warn` is `error`,
//!    `-qq` is off);
//! 2. `RUST_LOG`;
//! 3. the default filter the program passes in (any `EnvFilter` directive,
//!    e.g. `"warn"` or `"my_tool=info,warn"`).
//!
//! ```no_run
//! use bpaf::{Bpaf, Parser};
//! use bpaf_kit::log::{LogArgs, log_args};
//!
//! #[derive(Debug, Clone, Bpaf)]
//! #[bpaf(options)]
//! struct Opts {
//!     #[bpaf(external(log_args))]
//!     log: LogArgs,
//! }
//!
//! let opts = opts().run();
//! bpaf_kit::log::init("warn", &opts.log).expect("first init");
//! ```

use core::fmt;

use bpaf::{Parser, short};
use tracing::Level;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;

/// Logging flags: `-v`/`--verbose` and `-q`/`--quiet` (repeatable), and
/// `--log-json` with the `log-json` feature.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LogArgs {
    /// Times `-v` was given.
    pub verbose: usize,
    /// Times `-q` was given.
    pub quiet: usize,
    /// `--log-json` was given.
    pub json: bool,
}

/// The bpaf parser for [`LogArgs`]; use with `#[bpaf(external(log_args))]`.
pub fn log_args() -> impl Parser<LogArgs> {
    let verbose = short('v')
        .long("verbose")
        .help("More log output (repeat for more: -vv, -vvv)")
        .req_flag(())
        .count();
    let quiet = short('q')
        .long("quiet")
        .help("Less log output (-qq for none)")
        .req_flag(())
        .count();
    #[cfg(feature = "log-json")]
    let json = bpaf::long("log-json").help("Log as JSON lines").switch();
    #[cfg(not(feature = "log-json"))]
    let json = bpaf::pure(false);
    bpaf::construct!(LogArgs {
        verbose,
        quiet,
        json
    })
}

/// Logging could not be set up.
#[derive(Debug, thiserror::Error)]
#[cfg_attr(feature = "miette", derive(miette::Diagnostic))]
pub enum Error {
    /// The default filter is not a valid `EnvFilter` directive.
    #[error("invalid log filter '{filter}': {message}")]
    #[cfg_attr(feature = "miette", diagnostic(code(log_filter)))]
    Filter {
        /// The filter.
        filter: String,
        /// The parse error.
        message: String,
    },
    /// A global subscriber was already installed.
    #[error("logging was already initialized")]
    #[cfg_attr(feature = "miette", diagnostic(code(log_init)))]
    AlreadyInitialized,
}

impl Error {
    /// Stable `snake_case` identifier.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Filter { .. } => "log_filter",
            Self::AlreadyInitialized => "log_init",
        }
    }
}

const LEVELS: [LevelFilter; 6] = [
    LevelFilter::OFF,
    LevelFilter::ERROR,
    LevelFilter::WARN,
    LevelFilter::INFO,
    LevelFilter::DEBUG,
    LevelFilter::TRACE,
];

/// The level `-v`/`-q` select, starting from `base`; `None` when neither
/// was given.
#[must_use]
pub fn shifted_level(base: LevelFilter, args: &LogArgs) -> Option<LevelFilter> {
    if args.verbose == 0 && args.quiet == 0 {
        return None;
    }
    let at = LEVELS.iter().position(|l| *l == base).unwrap_or(2);
    let idx = (at + args.verbose)
        .saturating_sub(args.quiet)
        .min(LEVELS.len() - 1);
    Some(LEVELS[idx])
}

/// Build the filter per the module docs' precedence.
///
/// # Errors
///
/// [`Error::Filter`] when `default` doesn't parse.
pub fn filter(default: &str, args: &LogArgs) -> Result<EnvFilter, Error> {
    let parsed = EnvFilter::try_new(default).map_err(|e| Error::Filter {
        filter: default.to_owned(),
        message: e.to_string(),
    })?;
    // The base for -v/-q is the default's most verbose level.
    let base = parsed.max_level_hint().unwrap_or(LevelFilter::WARN);
    if let Some(level) = shifted_level(base, args) {
        return Ok(EnvFilter::default().add_directive(level.into()));
    }
    Ok(EnvFilter::try_from_default_env().unwrap_or(parsed))
}

/// Install the global subscriber.
///
/// # Errors
///
/// [`Error::Filter`] for a bad `default`, [`Error::AlreadyInitialized`] when
/// a subscriber is already set.
pub fn init(default: &str, args: &LogArgs) -> Result<(), Error> {
    let filter = filter(default, args)?;
    let ansi = crate::color_enabled(&std::io::stderr());
    let builder = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .with_ansi(ansi);
    #[cfg(feature = "log-json")]
    if args.json {
        return builder
            .json()
            .try_init()
            .map_err(|_| Error::AlreadyInitialized);
    }
    builder
        .event_format(CliFormat)
        .try_init()
        .map_err(|_| Error::AlreadyInitialized)
}

/// The `level: message fields` event format.
#[derive(Debug, Clone, Copy, Default)]
pub struct CliFormat;

impl<S, N> FormatEvent<S, N> for CliFormat
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> fmt::Result {
        let (label, style) = match *event.metadata().level() {
            Level::ERROR => ("error", "\x1b[1;31m"),
            Level::WARN => ("warning", "\x1b[1;33m"),
            Level::INFO => ("info", "\x1b[2m"),
            Level::DEBUG => ("debug", "\x1b[2m"),
            Level::TRACE => ("trace", "\x1b[2m"),
        };
        if writer.has_ansi_escapes() {
            write!(writer, "{style}{label}\x1b[0m: ")?;
        } else {
            write!(writer, "{label}: ")?;
        }
        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: usize, q: usize) -> LogArgs {
        LogArgs {
            verbose: v,
            quiet: q,
            json: false,
        }
    }

    #[test]
    fn shifting() {
        let warn = LevelFilter::WARN;
        assert_eq!(shifted_level(warn, &args(0, 0)), None);
        assert_eq!(shifted_level(warn, &args(1, 0)), Some(LevelFilter::INFO));
        assert_eq!(shifted_level(warn, &args(2, 0)), Some(LevelFilter::DEBUG));
        assert_eq!(shifted_level(warn, &args(9, 0)), Some(LevelFilter::TRACE));
        assert_eq!(shifted_level(warn, &args(0, 1)), Some(LevelFilter::ERROR));
        assert_eq!(shifted_level(warn, &args(0, 5)), Some(LevelFilter::OFF));
        assert_eq!(shifted_level(warn, &args(1, 1)), Some(LevelFilter::WARN));
        assert_eq!(
            shifted_level(LevelFilter::INFO, &args(1, 0)),
            Some(LevelFilter::DEBUG)
        );
    }

    #[test]
    fn flags_parse() {
        let p = log_args().to_options();
        assert_eq!(p.run_inner(&["-vv", "-q"][..]).unwrap(), args(2, 1));
        assert_eq!(
            p.run_inner(&["--verbose", "--quiet", "--quiet"][..])
                .unwrap(),
            args(1, 2)
        );
        assert_eq!(p.run_inner(&[] as &[&str]).unwrap(), LogArgs::default());
        #[cfg(feature = "log-json")]
        assert!(p.run_inner(&["--log-json"][..]).unwrap().json);
    }

    #[test]
    fn filters() {
        let f = filter("my_tool=info,warn", &args(1, 0)).unwrap();
        assert_eq!(f.max_level_hint(), Some(LevelFilter::DEBUG));
        let err = filter("[[[", &args(0, 0)).unwrap_err();
        assert_eq!(err.code(), "log_filter");
    }

    #[test]
    fn format_and_init() {
        use alloc::sync::Arc;
        use std::sync::Mutex;
        use tracing_subscriber::fmt::MakeWriter;

        #[derive(Clone, Default)]
        struct Buf(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Buf {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        impl<'a> MakeWriter<'a> for Buf {
            type Writer = Self;
            fn make_writer(&'a self) -> Self {
                self.clone()
            }
        }

        let buf = Buf::default();
        let sub = tracing_subscriber::fmt()
            .with_writer(buf.clone())
            .with_ansi(false)
            .event_format(CliFormat)
            .finish();
        tracing::subscriber::with_default(sub, || {
            tracing::warn!(path = "/tmp/x", "disk almost full");
            tracing::error!("gone");
        });
        let text = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert_eq!(
            text,
            "warning: disk almost full path=\"/tmp/x\"\nerror: gone\n"
        );

        init("warn", &LogArgs::default()).unwrap();
        assert_eq!(
            init("warn", &LogArgs::default()).unwrap_err().code(),
            "log_init"
        );
    }
}
