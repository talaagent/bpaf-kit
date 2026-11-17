//! Ctrl-C handling that leaves the terminal usable.
//!
//! Interactive prompts (dialoguer and friends) hide the cursor while they
//! run. If the user presses Ctrl-C mid-prompt, the default SIGINT behaviour
//! kills the process before the prompt can show the cursor again, and the
//! shell is left without one. [`install`] registers a handler that shows
//! the cursor, runs the cleanups registered with [`on_interrupt`], then
//! exits with the conventional status 130.
//!
//! ```no_run
//! bpaf_kit::interrupt::install().expect("no other Ctrl-C handler");
//!
//! let dir = std::env::temp_dir().join(format!("my-tool-{}", std::process::id()));
//! std::fs::create_dir_all(&dir)?;
//! // Removed on Ctrl-C while `cleanup` is alive.
//! let cleanup = bpaf_kit::interrupt::on_interrupt({
//!     let dir = dir.clone();
//!     move || drop(std::fs::remove_dir_all(dir))
//! });
//! // ... work in `dir` ...
//! std::fs::remove_dir_all(&dir)?;
//! drop(cleanup);
//! # Ok::<(), std::io::Error>(())
//! ```

use alloc::boxed::Box;
use core::sync::atomic::{AtomicU64, Ordering};
use std::io::{self, IsTerminal, Write};
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

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
    /// Stable `snake_case` identifier.
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
                run_cleanups();
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

type Hook = Box<dyn FnOnce() + Send>;

static HOOKS: Mutex<Vec<(u64, Hook)>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn hooks() -> MutexGuard<'static, Vec<(u64, Hook)>> {
    HOOKS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Run `cleanup` on Ctrl-C, before the handler exits 130: remove temporary
/// files, kill a child's process group, restore a terminal mode. Installs
/// the handler if needed ([`install`]; a foreign handler is left alone and
/// then nothing runs).
///
/// The cleanup stays registered while the returned [`Cleanup`] is alive;
/// dropping it unregisters it without running it, [`Cleanup::keep`] keeps
/// it for the rest of the process. On Ctrl-C, the registered cleanups run
/// once each, newest first, on the handler's thread (not in a signal
/// handler, so any code is fine); one that panics doesn't stop the rest.
/// They run while the main thread carries on, and a second Ctrl-C doesn't
/// interrupt them, so keep them short. They don't run on `SIGKILL` or
/// [`std::process::exit`].
pub fn on_interrupt(cleanup: impl FnOnce() + Send + 'static) -> Cleanup {
    let _ = install();
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    hooks().push((id, Box::new(cleanup)));
    Cleanup { id }
}

/// Keeps a cleanup from [`on_interrupt`] registered; dropping it
/// unregisters the cleanup.
#[derive(Debug)]
#[must_use = "dropping the guard unregisters the cleanup at once"]
pub struct Cleanup {
    id: u64,
}

impl Cleanup {
    /// Keep the cleanup registered for the rest of the process.
    pub fn keep(self) {
        core::mem::forget(self);
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        hooks().retain(|(id, _)| *id != self.id);
    }
}

/// Take the registered cleanups and run them, newest first. Panics are
/// caught (the panic hook still prints them).
fn run_cleanups() {
    // Taken out first, so each runs once and a cleanup can use
    // `on_interrupt` or drop a `Cleanup` without deadlocking.
    let taken = core::mem::take(&mut *hooks());
    for (_, hook) in taken.into_iter().rev() {
        let _ = panic::catch_unwind(AssertUnwindSafe(hook));
    }
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

    // The only test touching the registry: tests share the process.
    #[test]
    fn cleanups_run_once_newest_first_past_panics() {
        use std::sync::Arc;
        let log = Arc::new(Mutex::new(Vec::new()));
        let push = |name: &'static str| {
            let log = Arc::clone(&log);
            move || log.lock().unwrap().push(name)
        };
        on_interrupt(push("first")).keep();
        let dropped = on_interrupt(push("dropped"));
        let _alive = on_interrupt(push("alive"));
        on_interrupt(|| panic!("cleanup panics")).keep();
        on_interrupt(push("last")).keep();
        drop(dropped);

        run_cleanups();
        assert_eq!(*log.lock().unwrap(), ["last", "alive", "first"]);
        run_cleanups();
        assert_eq!(log.lock().unwrap().len(), 3, "each runs once");
        assert!(is_installed());
    }
}
