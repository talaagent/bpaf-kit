//! Ctrl-C handling that leaves the terminal usable.
//!
//! Interactive prompts (dialoguer and friends) hide the cursor while they
//! run. If the user presses Ctrl-C mid-prompt, the default SIGINT behaviour
//! kills the process before the prompt can show the cursor again, and the
//! shell is left without one. [`install`] registers a handler that shows
//! the cursor, then exits with the conventional status 130.
//!
//! ```no_run
//! bpaf_kit::interrupt::install().expect("no other Ctrl-C handler");
//! ```

use std::io::{self, IsTerminal, Write};
use std::sync::OnceLock;

/// Exit status after Ctrl-C (128 + SIGINT).
pub const EXIT_CODE: i32 = 130;

/// The Ctrl-C handler could not be installed.
#[derive(Debug, Clone, thiserror::Error)]
#[cfg_attr(feature = "miette", derive(miette::Diagnostic))]
#[cfg_attr(feature = "miette", diagnostic(code(interrupt_install)))]
#[error("failed to install the Ctrl-C handler: {message}")]
pub struct Error {
    message: String,
}

impl Error {
    /// Stable snake_case identifier.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        "interrupt_install"
    }
}

static INSTALLED: OnceLock<Result<(), Error>> = OnceLock::new();

/// Install the handler. Safe to call more than once: later calls return the
/// first call's result.
///
/// # Errors
///
/// When another Ctrl-C handler (one not installed through this function) is
/// already registered, or the OS refuses.
pub fn install() -> Result<(), Error> {
    INSTALLED
        .get_or_init(|| {
            ctrlc::set_handler(|| {
                restore_terminal();
                std::process::exit(EXIT_CODE);
            })
            .map_err(|e| Error {
                message: e.to_string(),
            })
        })
        .clone()
}

/// Whether [`install`] has succeeded in this process.
#[must_use]
pub fn is_installed() -> bool {
    matches!(INSTALLED.get(), Some(Ok(())))
}

/// Show the cursor and reset text attributes on whichever of stderr/stdout is
/// a terminal. Writes nothing when neither is, so pipes stay clean.
pub fn restore_terminal() {
    const RESET: &[u8] = b"\x1b[?25h\x1b[0m";
    if io::stderr().is_terminal() {
        let mut err = io::stderr().lock();
        let _ = err.write_all(RESET);
        let _ = err.write_all(b"\n");
    } else if io::stdout().is_terminal() {
        let mut out = io::stdout().lock();
        let _ = out.write_all(RESET);
        let _ = out.write_all(b"\n");
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_idempotent() {
        install().unwrap();
        install().unwrap();
        assert!(is_installed());
        // A second, foreign handler is refused by ctrlc; ours stays.
        assert!(ctrlc::set_handler(|| {}).is_err());
    }
}
