//! Print an error the way command-line tools do and turn it into an exit
//! code, without a diagnostics framework.
//!
//! An error type implements [`Coded`] (a stable `snake_case` code, and an
//! exit code that defaults to 1); [`report`] prints
//! `<bin>: error[<code>]: <message>` to stderr, then one `  caused by:` line
//! per [`source`](core::error::Error::source) the message doesn't already
//! include, and returns the exit code:
//!
//! ```no_run
//! use bpaf_kit::report::{self, Coded};
//! use std::process::ExitCode;
//!
//! #[derive(Debug, thiserror::Error)]
//! enum Error {
//!     #[error(transparent)]
//!     Cmd(#[from] bpaf_kit::cmd::Error),
//!     #[error("no config at {0}")]
//!     NoConfig(String),
//! }
//!
//! impl Coded for Error {
//!     fn code(&self) -> &str {
//!         match self {
//!             Self::Cmd(e) => e.code(),
//!             Self::NoConfig(_) => "no_config",
//!         }
//!     }
//!     fn exit_code(&self) -> u8 {
//!         match self {
//!             Self::Cmd(e) => Coded::exit_code(e),
//!             Self::NoConfig(_) => 2,
//!         }
//!     }
//! }
//!
//! fn run() -> Result<(), Error> {
//!     Err(Error::NoConfig("tool.toml".into()))
//! }
//!
//! fn main() -> ExitCode {
//!     // tool: error[no_config]: no config at tool.toml
//!     run().map_or_else(|e| report::report(&e), |()| ExitCode::SUCCESS)
//! }
//! ```
//!
//! bpaf-kit's own errors implement [`Coded`] with their inherent `code()`.

use alloc::string::String;
use core::error::Error;
use core::fmt::Write as _;
use std::io::{self, Write};
use std::process::ExitCode;

/// An error with a stable identifier and an exit code, for [`report`].
pub trait Coded: Error {
    /// Stable `snake_case` identifier for the error kind, printed as
    /// `error[<code>]`.
    fn code(&self) -> &str;

    /// The process exit status for this error: 1 unless overridden.
    fn exit_code(&self) -> u8 {
        1
    }
}

/// Print `err` to stderr as `<bin>: error[<code>]: <message>` plus its
/// source chain and return its exit code. `<bin>` is the file name of the
/// running executable (`argv[0]`); use [`report_as`] to choose it (a cargo
/// subcommand would say `cargo foo`).
pub fn report(err: &dyn Coded) -> ExitCode {
    report_as(&program_name(), err)
}

/// [`report`] with an explicit program name.
pub fn report_as(program: &str, err: &dyn Coded) -> ExitCode {
    let _ = io::stderr()
        .lock()
        .write_all(render(program, err).as_bytes());
    ExitCode::from(err.exit_code())
}

/// The text [`report_as`] prints, ending in a newline: the message line,
/// then `  caused by: <source>` for each source in the chain whose message
/// isn't already part of the line before (as with `{source}` in a
/// thiserror message).
#[must_use]
pub fn render(program: &str, err: &dyn Coded) -> String {
    let mut out = String::new();
    let mut previous = err.to_string();
    let _ = writeln!(out, "{program}: error[{}]: {previous}", err.code());
    let mut source = err.source();
    while let Some(cause) = source {
        let message = cause.to_string();
        if !previous.contains(&message) {
            let _ = writeln!(out, "  caused by: {message}");
        }
        previous = message;
        source = cause.source();
    }
    out
}

fn program_name() -> String {
    std::env::args_os()
        .next()
        .as_deref()
        .map(std::path::Path::new)
        .and_then(std::path::Path::file_stem)
        .map_or_else(
            || String::from("error"),
            |name| name.to_string_lossy().into_owned(),
        )
}

macro_rules! coded {
    ($($ty:ty),* $(,)?) => {$(
        impl Coded for $ty {
            fn code(&self) -> &str {
                <$ty>::code(self)
            }
        }
    )*};
}

coded!(
    crate::cmd::Error,
    crate::interrupt::Error,
    crate::suggest::UnknownValue
);
#[cfg(feature = "log")]
coded!(crate::log::Error);

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, thiserror::Error)]
    enum Test {
        #[error("loading `{path}` failed")]
        Load {
            path: &'static str,
            #[source]
            source: io::Error,
        },
        #[error("wrapped: {0}")]
        Wrapped(#[source] io::Error),
    }

    impl Coded for Test {
        fn code(&self) -> &str {
            match self {
                Self::Load { .. } => "load",
                Self::Wrapped(_) => "wrapped",
            }
        }
        fn exit_code(&self) -> u8 {
            3
        }
    }

    #[test]
    fn prints_code_message_and_new_causes() {
        let err = Test::Load {
            path: "a.toml",
            source: io::Error::other("denied"),
        };
        assert_eq!(
            render("tool", &err),
            "tool: error[load]: loading `a.toml` failed\n  caused by: denied\n"
        );
        assert_eq!(report_as("tool", &err), ExitCode::from(3));
    }

    #[test]
    fn skips_causes_the_message_already_shows() {
        let err = Test::Wrapped(io::Error::other("denied"));
        assert_eq!(
            render("tool", &err),
            "tool: error[wrapped]: wrapped: denied\n"
        );
    }

    #[test]
    fn bpaf_kit_errors_are_coded() {
        let err = crate::cmd::Cmd::new("bpaf-kit-no-such-program")
            .run()
            .unwrap_err();
        let text = render("cargo tool", &err);
        assert!(
            text.starts_with(
                "cargo tool: error[cmd_spawn]: failed to start `bpaf-kit-no-such-program`: "
            ),
            "{text}"
        );
        assert_eq!(text.lines().count(), 1, "{text}");
        assert_eq!(Coded::exit_code(&err), 1);
    }
}
