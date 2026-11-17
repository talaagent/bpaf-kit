//! A subprocess runner on top of [`std::process::Command`].
//!
//! ```no_run
//! use bpaf_kit::cmd::{Cmd, Redirect};
//!
//! # fn main() -> Result<(), bpaf_kit::cmd::Error> {
//! // `+ git fetch --prune` on stderr, then git's own output streams through.
//! Cmd::new("git").args(["fetch", "--prune"]).run()?;
//!
//! // stdout returned; stderr captured, and quoted in the error on failure.
//! let head = Cmd::new("git").args(["rev-parse", "HEAD"]).capture_stdout()?;
//!
//! // Capture both streams quietly, give up after 5s.
//! let out = Cmd::new("make")
//!     .arg("check")
//!     .echo(false)
//!     .stderr(Redirect::Capture)
//!     .timeout(core::time::Duration::from_secs(5))
//!     .output()?;
//! # let _ = (head, out);
//! # Ok(())
//! # }
//! ```
//!
//! Every command is echoed to stderr before it runs, `set -x` style
//! (`+ program args…`), so a CLI documents what it does. Turn that off per
//! command with [`Cmd::echo`], process-wide with [`set_echo`], or from the
//! environment with `BPAF_KIT_ECHO=0`. Dry-run mode ([`set_dry_run`],
//! `BPAF_KIT_DRY_RUN=1`, or [`Cmd::dry_run`]) echoes commands without running
//! them. A long command line can be shortened with [`Cmd::echo_width`], or
//! replaced by a description with [`Cmd::echo_label`].
//!
//! A non-zero exit is an [`Error::Exit`] carrying the last few KiB of the
//! child's stderr whenever stderr was captured or tee'd; use
//! [`Cmd::allow_failure`] to get the [`Output`] back instead.

use alloc::borrow::Cow;
use alloc::string::FromUtf8Error;
use core::fmt;
use core::sync::atomic::{AtomicU8, Ordering};
use core::time::Duration;
use std::ffi::OsStr;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::thread::{self, JoinHandle};
use std::time::Instant;

/// Env var that sets the default for echoing commands (`0`/`false`/`off` to
/// disable). Read once, on first use.
pub const ECHO_ENV: &str = "BPAF_KIT_ECHO";

/// Env var that turns on dry-run mode (`1`/`true`/`on`). Read once, on first
/// use.
pub const DRY_RUN_ENV: &str = "BPAF_KIT_DRY_RUN";

/// How much of the child's stderr an [`Error`] keeps.
pub const STDERR_TAIL_BYTES: usize = 4096;

/// Where one of the child's output streams goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Redirect {
    /// Share the parent's file descriptor; nothing is captured.
    Inherit,
    /// Collect into [`Output`]; nothing reaches the terminal.
    Capture,
    /// Collect into [`Output`] and forward to the parent's stream as it
    /// arrives.
    Tee,
    /// Discard (`/dev/null`).
    Null,
    /// Write to the parent's stderr. For a child's stdout when this
    /// program's own stdout is reserved (output meant for `eval`, say).
    Stderr,
}

impl Redirect {
    fn stdio(self) -> Stdio {
        match self {
            Self::Inherit => Stdio::inherit(),
            Self::Capture | Self::Tee => Stdio::piped(),
            Self::Null => Stdio::null(),
            Self::Stderr => io::stderr().into(),
        }
    }
}

/// A finished (or dry-run) command.
#[derive(Debug, Clone, Default)]
pub struct Output {
    /// Exit status. Success for a dry run.
    pub status: ExitStatus,
    /// Captured stdout; empty unless stdout was [`Redirect::Capture`] or
    /// [`Redirect::Tee`].
    pub stdout: Vec<u8>,
    /// Captured stderr; empty unless stderr was [`Redirect::Capture`] or
    /// [`Redirect::Tee`].
    pub stderr: Vec<u8>,
    /// The command was only echoed, not run.
    pub dry_run: bool,
    /// Stdout went past [`Cmd::max_output`]: `stdout` holds its first bytes.
    pub stdout_truncated: bool,
    /// Stderr went past [`Cmd::max_output`]: `stderr` holds its first bytes.
    pub stderr_truncated: bool,
}

impl Output {
    /// Whether the command exited successfully.
    #[must_use]
    pub fn success(&self) -> bool {
        self.status.success()
    }

    /// Captured stdout as text, replacing invalid UTF-8.
    #[must_use]
    pub fn stdout_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.stdout)
    }

    /// Captured stderr as text, replacing invalid UTF-8.
    #[must_use]
    pub fn stderr_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.stderr)
    }
}

/// Errors from running a command. [`Error::code`] gives a stable identifier.
#[derive(Debug, thiserror::Error)]
#[cfg_attr(feature = "miette", derive(miette::Diagnostic))]
pub enum Error {
    /// The program could not be started (not found, not executable, …).
    #[error("failed to start `{program}`: {source}")]
    #[cfg_attr(feature = "miette", diagnostic(code(cmd_spawn)))]
    Spawn {
        /// The program.
        program: String,
        /// The OS error.
        #[source]
        source: io::Error,
    },
    /// Reading the child's output or waiting for it failed.
    #[error("I/O error while running `{program}`: {source}")]
    #[cfg_attr(feature = "miette", diagnostic(code(cmd_io)))]
    Io {
        /// The program.
        program: String,
        /// The OS error.
        #[source]
        source: io::Error,
    },
    /// The program exited unsuccessfully.
    #[error("`{program}` failed ({status}){}", tail_suffix(stderr_tail))]
    #[cfg_attr(feature = "miette", diagnostic(code(cmd_exit)))]
    Exit {
        /// The program.
        program: String,
        /// Its exit status.
        status: ExitStatus,
        /// The end of its stderr, if stderr was captured or tee'd.
        stderr_tail: String,
    },
    /// The program ran past [`Cmd::timeout`] and was killed.
    #[error("`{program}` timed out after {timeout:?}{}", tail_suffix(stderr_tail))]
    #[cfg_attr(feature = "miette", diagnostic(code(cmd_timeout)))]
    Timeout {
        /// The program.
        program: String,
        /// The timeout that elapsed.
        timeout: Duration,
        /// The end of its stderr, if stderr was captured or tee'd.
        stderr_tail: String,
    },
    /// [`Cmd::capture_stdout`] got bytes that are not UTF-8.
    #[error("`{program}` printed non-UTF-8 output")]
    #[cfg_attr(feature = "miette", diagnostic(code(cmd_utf8)))]
    Utf8 {
        /// The program.
        program: String,
        /// The decoding error.
        #[source]
        source: FromUtf8Error,
    },
}

impl Error {
    /// Stable `snake_case` identifier for the error kind.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Spawn { .. } => "cmd_spawn",
            Self::Io { .. } => "cmd_io",
            Self::Exit { .. } => "cmd_exit",
            Self::Timeout { .. } => "cmd_timeout",
            Self::Utf8 { .. } => "cmd_utf8",
        }
    }

    /// The program the error is about.
    #[must_use]
    pub fn program(&self) -> &str {
        match self {
            Self::Spawn { program, .. }
            | Self::Io { program, .. }
            | Self::Exit { program, .. }
            | Self::Timeout { program, .. }
            | Self::Utf8 { program, .. } => program,
        }
    }

    /// The child's exit code, for [`Error::Exit`] when it exited normally.
    #[must_use]
    pub fn exit_code(&self) -> Option<i32> {
        match self {
            Self::Exit { status, .. } => status.code(),
            _ => None,
        }
    }

    /// The end of the child's stderr, when it was captured.
    #[must_use]
    pub fn stderr_tail(&self) -> Option<&str> {
        match self {
            Self::Exit { stderr_tail, .. } | Self::Timeout { stderr_tail, .. }
                if !stderr_tail.is_empty() =>
            {
                Some(stderr_tail)
            }
            _ => None,
        }
    }
}

fn tail_suffix(tail: &str) -> String {
    if tail.is_empty() {
        String::new()
    } else {
        format!(":\n{tail}")
    }
}

/// A process-wide on/off setting with an env var default.
struct Flag {
    state: AtomicU8,
    env: &'static str,
    default: bool,
}

const UNSET: u8 = 0;
const OFF: u8 = 1;
const ON: u8 = 2;

impl Flag {
    const fn new(env: &'static str, default: bool) -> Self {
        Self {
            state: AtomicU8::new(UNSET),
            env,
            default,
        }
    }

    fn get(&self) -> bool {
        match self.state.load(Ordering::Relaxed) {
            ON => true,
            OFF => false,
            _ => {
                let on = std::env::var(self.env)
                    .ok()
                    .and_then(|v| parse_bool(&v))
                    .unwrap_or(self.default);
                // A concurrent `set` wins over the env default.
                let _ = self.state.compare_exchange(
                    UNSET,
                    if on { ON } else { OFF },
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                );
                self.state.load(Ordering::Relaxed) == ON
            }
        }
    }

    fn set(&self, on: bool) {
        self.state
            .store(if on { ON } else { OFF }, Ordering::Relaxed);
    }
}

static ECHO: Flag = Flag::new(ECHO_ENV, true);
static DRY_RUN: Flag = Flag::new(DRY_RUN_ENV, false);

pub(crate) fn parse_bool(s: &str) -> Option<bool> {
    let s = s.trim();
    ["1", "true", "yes", "on"]
        .iter()
        .any(|t| s.eq_ignore_ascii_case(t))
        .then_some(true)
        .or_else(|| {
            ["0", "false", "no", "off", ""]
                .iter()
                .any(|f| s.eq_ignore_ascii_case(f))
                .then_some(false)
        })
}

/// Set whether commands are echoed by default (overrides [`ECHO_ENV`]).
pub fn set_echo(on: bool) {
    ECHO.set(on);
}

/// Whether commands are echoed by default.
#[must_use]
pub fn echo_enabled() -> bool {
    ECHO.get()
}

/// Set process-wide dry-run mode (overrides [`DRY_RUN_ENV`]).
pub fn set_dry_run(on: bool) {
    DRY_RUN.set(on);
}

/// Whether dry-run mode is on by default.
#[must_use]
pub fn dry_run_enabled() -> bool {
    DRY_RUN.get()
}

/// Print a line to stderr with a `+ ` prefix (dim on a color terminal), the
/// way commands are echoed. Use it to narrate other side effects too.
pub fn echo_line(line: impl fmt::Display) {
    let mut err = io::stderr().lock();
    let _ = if crate::color_enabled(&io::stderr()) {
        writeln!(err, "\x1b[2m+\x1b[0m {line}")
    } else {
        writeln!(err, "+ {line}")
    };
}

#[derive(Clone, Copy)]
enum Mode {
    Run,
    Output,
    CaptureStdout,
}

/// A command to run. Builder methods mirror [`Command`]'s.
#[derive(Debug)]
pub struct Cmd {
    command: Command,
    stdout: Option<Redirect>,
    stderr: Option<Redirect>,
    echo: Option<bool>,
    echo_label: Option<String>,
    echo_width: Option<usize>,
    dry_run: Option<bool>,
    timeout: Option<Duration>,
    max_output: Option<usize>,
    check: bool,
}

impl From<Command> for Cmd {
    fn from(command: Command) -> Self {
        Self {
            command,
            stdout: None,
            stderr: None,
            echo: None,
            echo_label: None,
            echo_width: None,
            dry_run: None,
            timeout: None,
            max_output: None,
            check: true,
        }
    }
}

impl Cmd {
    /// A command for `program`.
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Command::new(program).into()
    }

    /// Add an argument.
    pub fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut Self {
        self.command.arg(arg);
        self
    }

    /// Add arguments.
    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.command.args(args);
        self
    }

    /// Set an environment variable for the child.
    pub fn env(&mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut Self {
        self.command.env(key, value);
        self
    }

    /// Set environment variables for the child.
    pub fn envs<I, K, V>(&mut self, vars: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.command.envs(vars);
        self
    }

    /// Remove an environment variable from the child's environment.
    pub fn env_remove(&mut self, key: impl AsRef<OsStr>) -> &mut Self {
        self.command.env_remove(key);
        self
    }

    /// Start the child with an empty environment (plus [`Cmd::env`] vars).
    pub fn env_clear(&mut self) -> &mut Self {
        self.command.env_clear();
        self
    }

    /// Run the child in `dir`.
    pub fn current_dir(&mut self, dir: impl AsRef<Path>) -> &mut Self {
        self.command.current_dir(dir);
        self
    }

    /// The child's stdin (inherited by default).
    pub fn stdin(&mut self, stdin: impl Into<Stdio>) -> &mut Self {
        self.command.stdin(stdin);
        self
    }

    /// Where the child's stdout goes. Defaults: [`Redirect::Inherit`] for
    /// [`Cmd::run`], [`Redirect::Capture`] for [`Cmd::output`] and
    /// [`Cmd::capture_stdout`] (which also upgrades `Inherit`/`Null` to
    /// `Capture`).
    pub fn stdout(&mut self, redirect: Redirect) -> &mut Self {
        self.stdout = Some(redirect);
        self
    }

    /// Where the child's stderr goes. Defaults: [`Redirect::Inherit`] for
    /// [`Cmd::run`], [`Redirect::Capture`] for [`Cmd::output`] and
    /// [`Cmd::capture_stdout`]. [`Redirect::Tee`] shows it live as well,
    /// which suits long-running commands that report progress.
    pub fn stderr(&mut self, redirect: Redirect) -> &mut Self {
        self.stderr = Some(redirect);
        self
    }

    /// Echo this command (overrides [`set_echo`] and [`ECHO_ENV`]).
    pub fn echo(&mut self, on: bool) -> &mut Self {
        self.echo = Some(on);
        self
    }

    /// Echo `+ label` instead of the command line, for commands whose line
    /// says less than a description would (a replayed `rustc` call with
    /// hundreds of arguments, say). Dry-run mode echoes the label too.
    pub fn echo_label(&mut self, label: impl Into<String>) -> &mut Self {
        self.echo_label = Some(label.into());
        self
    }

    /// Shorten the echoed command line (and [`Cmd::display`]) to about
    /// `chars` characters. The `cd DIR &&` prefix and the program are always
    /// shown. Environment variables go first, collapsed to `…`, then
    /// arguments from the middle, replaced by `…(N more)`, keeping as
    /// many from the start and the end as fit. A line within `chars` is
    /// shown as is.
    ///
    /// ```
    /// let mut cmd = bpaf_kit::cmd::Cmd::new("rustc");
    /// cmd.args(["--crate-name", "foo", "--edition=2024", "src/lib.rs"])
    ///     .args((0..50).map(|i| format!("-Cmetadata={i}")))
    ///     .args(["--out-dir", "target/debug/deps"])
    ///     .echo_width(60);
    /// assert_eq!(
    ///     cmd.display().to_string(),
    ///     "rustc --crate-name foo …(53 more) target/debug/deps",
    /// );
    /// ```
    pub fn echo_width(&mut self, chars: usize) -> &mut Self {
        self.echo_width = Some(chars);
        self
    }

    /// Only echo this command, don't run it (overrides [`set_dry_run`]).
    /// Use `dry_run(false)` for read-only commands whose output the program
    /// needs even in dry-run mode.
    pub fn dry_run(&mut self, on: bool) -> &mut Self {
        self.dry_run = Some(on);
        self
    }

    /// Kill the child if it runs longer than `timeout`.
    pub fn timeout(&mut self, timeout: Duration) -> &mut Self {
        self.timeout = Some(timeout);
        self
    }

    /// Keep at most `bytes` of each captured stream. The rest is still read
    /// (and still tee'd), so the child never blocks on a full pipe, but
    /// dropped; [`Output::stdout_truncated`] and
    /// [`Output::stderr_truncated`] say whether anything was. For children
    /// whose output size isn't yours to trust; pair it with a
    /// [`Cmd::timeout`].
    pub fn max_output(&mut self, bytes: usize) -> &mut Self {
        self.max_output = Some(bytes);
        self
    }

    /// Return a non-zero exit as an [`Output`] instead of [`Error::Exit`].
    pub fn allow_failure(&mut self) -> &mut Self {
        self.check = false;
        self
    }

    /// The underlying [`Command`].
    #[must_use]
    pub fn as_command(&self) -> &Command {
        &self.command
    }

    /// The underlying [`Command`], for settings this builder doesn't wrap
    /// (`CommandExt` and the like).
    pub fn as_command_mut(&mut self) -> &mut Command {
        &mut self.command
    }

    /// The command line as it is echoed: `cd DIR && K=V program args…`, with
    /// shell quoting where needed, shortened to [`Cmd::echo_width`] if set.
    /// For people, not for feeding to a shell.
    #[must_use]
    pub fn display(&self) -> CommandLine<'_> {
        CommandLine {
            command: &self.command,
            width: self.echo_width,
        }
    }

    /// What is echoed: the label, or the command line.
    fn echo_text(&self) -> String {
        self.echo_label
            .clone()
            .unwrap_or_else(|| self.display().to_string())
    }

    /// Run with stdout and stderr going to the terminal unless redirected.
    ///
    /// # Errors
    ///
    /// [`Error::Spawn`], [`Error::Io`], [`Error::Exit`] (unless
    /// [`Cmd::allow_failure`]), [`Error::Timeout`].
    pub fn run(&mut self) -> Result<Output, Error> {
        self.exec(Mode::Run)
    }

    /// Run capturing stdout and stderr unless redirected.
    ///
    /// # Errors
    ///
    /// As [`Cmd::run`].
    pub fn output(&mut self) -> Result<Output, Error> {
        self.exec(Mode::Output)
    }

    /// Run and return stdout as a string. Stderr is captured for the error
    /// message unless redirected. A dry run returns an empty string.
    ///
    /// # Errors
    ///
    /// As [`Cmd::run`], plus [`Error::Utf8`].
    pub fn capture_stdout(&mut self) -> Result<String, Error> {
        let out = self.exec(Mode::CaptureStdout)?;
        String::from_utf8(out.stdout).map_err(|source| Error::Utf8 {
            program: self.program(),
            source,
        })
    }

    fn program(&self) -> String {
        self.command.get_program().to_string_lossy().into_owned()
    }

    fn exec(&mut self, mode: Mode) -> Result<Output, Error> {
        let (stdout, stderr) = match mode {
            Mode::Run => (
                self.stdout.unwrap_or(Redirect::Inherit),
                self.stderr.unwrap_or(Redirect::Inherit),
            ),
            Mode::Output => (
                self.stdout.unwrap_or(Redirect::Capture),
                self.stderr.unwrap_or(Redirect::Capture),
            ),
            Mode::CaptureStdout => (
                match self.stdout {
                    Some(Redirect::Tee) => Redirect::Tee,
                    _ => Redirect::Capture,
                },
                self.stderr.unwrap_or(Redirect::Capture),
            ),
        };

        let dry_run = self.dry_run.unwrap_or_else(dry_run_enabled);
        if dry_run || self.echo.unwrap_or_else(echo_enabled) {
            echo_line(self.echo_text());
        }
        if dry_run {
            return Ok(Output {
                dry_run: true,
                ..Output::default()
            });
        }

        self.command.stdout(stdout.stdio());
        self.command.stderr(stderr.stdio());
        let mut child = self.command.spawn().map_err(|source| Error::Spawn {
            program: self.program(),
            source,
        })?;
        let pipes = Pipes {
            stdout: child.stdout.take(),
            stderr: child.stderr.take(),
            tee_stdout: stdout == Redirect::Tee,
            tee_stderr: stderr == Redirect::Tee,
            limit: self.max_output.unwrap_or(usize::MAX),
        };

        let io_err = |source| Error::Io {
            program: self.program(),
            source,
        };
        let (status, stdout, stderr) = match self.timeout {
            None => pipes.wait(&mut child).map_err(io_err)?,
            Some(timeout) => match pipes.wait_timeout(&mut child, timeout).map_err(io_err)? {
                Waited::Exited(status, stdout, stderr) => (status, stdout, stderr),
                Waited::TimedOut(stderr) => {
                    return Err(Error::Timeout {
                        program: self.program(),
                        timeout,
                        stderr_tail: tail(&stderr.bytes),
                    });
                }
            },
        };

        if self.check && !status.success() {
            return Err(Error::Exit {
                program: self.program(),
                status,
                stderr_tail: tail(&stderr.bytes),
            });
        }
        Ok(Output {
            status,
            stdout: stdout.bytes,
            stderr: stderr.bytes,
            dry_run: false,
            stdout_truncated: stdout.truncated,
            stderr_truncated: stderr.truncated,
        })
    }
}

struct Pipes {
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    tee_stdout: bool,
    tee_stderr: bool,
    /// Bytes kept per stream.
    limit: usize,
}

/// What was read from one stream.
#[derive(Default)]
struct Drained {
    bytes: Vec<u8>,
    truncated: bool,
}

enum Waited {
    Exited(ExitStatus, Drained, Drained),
    TimedOut(Drained),
}

type Drain = JoinHandle<io::Result<Drained>>;

impl Pipes {
    /// Drain both pipes while the child runs (a full pipe would block it),
    /// using at most one extra thread.
    fn wait(self, child: &mut Child) -> io::Result<(ExitStatus, Drained, Drained)> {
        let limit = self.limit;
        let (stdout, stderr) = match (self.stdout, self.stderr) {
            (Some(out), Some(err)) => {
                let tee = self.tee_stderr;
                let err = thread::spawn(move || drain(err, tee, Sink::Stderr, limit));
                let out = drain(out, self.tee_stdout, Sink::Stdout, limit);
                (out?, join(err)?)
            }
            (Some(out), None) => (
                drain(out, self.tee_stdout, Sink::Stdout, limit)?,
                Drained::default(),
            ),
            (None, Some(err)) => (
                Drained::default(),
                drain(err, self.tee_stderr, Sink::Stderr, limit)?,
            ),
            (None, None) => (Drained::default(), Drained::default()),
        };
        Ok((child.wait()?, stdout, stderr))
    }

    fn wait_timeout(self, child: &mut Child, timeout: Duration) -> io::Result<Waited> {
        let deadline = Instant::now() + timeout;
        let (tee_out, tee_err, limit) = (self.tee_stdout, self.tee_stderr, self.limit);
        let out = self
            .stdout
            .map(|p| thread::spawn(move || drain(p, tee_out, Sink::Stdout, limit)));
        let err = self
            .stderr
            .map(|p| thread::spawn(move || drain(p, tee_err, Sink::Stderr, limit)));

        let mut nap = Duration::from_millis(1);
        loop {
            if let Some(status) = child.try_wait()? {
                let stdout = out.map_or_else(|| Ok(Drained::default()), join)?;
                let stderr = err.map_or_else(|| Ok(Drained::default()), join)?;
                return Ok(Waited::Exited(status, stdout, stderr));
            }
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            thread::sleep(nap.min(deadline - now));
            nap = (nap * 2).min(Duration::from_millis(50));
        }

        // Killing the child closes its ends of the pipes, but a grandchild may
        // still hold them open: give the drains a moment, then abandon them.
        let _ = child.kill();
        child.wait()?;
        let grace = Instant::now() + Duration::from_millis(200);
        let stderr = match err {
            Some(handle) => {
                while !handle.is_finished() && Instant::now() < grace {
                    thread::sleep(Duration::from_millis(5));
                }
                if handle.is_finished() {
                    join(handle).unwrap_or_default()
                } else {
                    Drained::default()
                }
            }
            None => Drained::default(),
        };
        Ok(Waited::TimedOut(stderr))
    }
}

#[derive(Clone, Copy)]
enum Sink {
    Stdout,
    Stderr,
}

/// Reads `src` to its end, keeping its first `limit` bytes.
fn drain(mut src: impl Read, mut tee: bool, sink: Sink, limit: usize) -> io::Result<Drained> {
    let mut out = Drained::default();
    let mut chunk = [0u8; 8192];
    loop {
        let n = match src.read(&mut chunk) {
            Ok(0) => return Ok(out),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        let keep = n.min(limit - out.bytes.len());
        out.bytes.extend_from_slice(&chunk[..keep]);
        out.truncated |= keep < n;
        // Stop forwarding if the parent's stream is gone, but keep draining so
        // the child never blocks on a full pipe.
        if tee {
            let forwarded = match sink {
                Sink::Stdout => {
                    let mut out = io::stdout().lock();
                    out.write_all(&chunk[..n]).and_then(|()| out.flush())
                }
                Sink::Stderr => io::stderr().lock().write_all(&chunk[..n]),
            };
            tee = forwarded.is_ok();
        }
    }
}

fn join(handle: Drain) -> io::Result<Drained> {
    handle
        .join()
        .unwrap_or_else(|_| Err(io::Error::other("output reader thread panicked")))
}

/// The last [`STDERR_TAIL_BYTES`] of `stderr`, starting at a line boundary
/// when truncated, trimmed.
fn tail(stderr: &[u8]) -> String {
    let start = stderr.len().saturating_sub(STDERR_TAIL_BYTES);
    let mut bytes = &stderr[start..];
    if start > 0
        && let Some(nl) = bytes.iter().position(|&b| b == b'\n')
    {
        bytes = &bytes[nl + 1..];
    }
    String::from_utf8_lossy(bytes).trim().to_owned()
}

/// Display form of a command line; see [`Cmd::display`].
pub struct CommandLine<'a> {
    command: &'a Command,
    width: Option<usize>,
}

impl fmt::Display for CommandLine<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cmd = self.command;
        let quoted = |s: &OsStr| {
            let mut out = String::new();
            let _ = quote(&mut out, &s.to_string_lossy());
            out
        };
        if let Some(dir) = cmd.get_current_dir() {
            write!(f, "cd {} && ", quoted(dir.as_os_str()))?;
        }
        let envs: Vec<String> = cmd
            .get_envs()
            .filter_map(|(key, value)| Some(format!("{}={}", quoted(key), quoted(value?))))
            .collect();
        let program = quoted(cmd.get_program());
        let args: Vec<String> = cmd.get_args().map(quoted).collect();
        let Some(width) = self.width else {
            return write_words(f, &envs, &program, &args);
        };
        // Lengths in chars, each with its separating space.
        let len = |words: &[String]| -> usize { words.iter().map(|w| w.chars().count() + 1).sum() };
        let fixed = cmd.get_current_dir().map_or(0, |dir| {
            quoted(dir.as_os_str()).chars().count() + "cd  && ".len()
        }) + program.chars().count();
        if fixed + len(&envs) + len(&args) <= width {
            return write_words(f, &envs, &program, &args);
        }
        let envs = if envs.is_empty() {
            envs
        } else {
            vec![String::from("…")]
        };
        let envs = envs.as_slice();
        let room = width.saturating_sub(fixed + len(envs));
        if len(&args) <= room {
            return write_words(f, envs, &program, &args);
        }
        // Take arguments from the front and the back in turn while they
        // fit next to the `…(N more)` marker.
        let (mut head, mut tail, mut used) = (0, 0, 0);
        loop {
            let marker = format!("…({} more)", args.len() - head - tail)
                .chars()
                .count()
                + 1;
            let next = if head <= tail {
                head
            } else {
                args.len() - 1 - tail
            };
            let cost = args[next].chars().count() + 1;
            if head + tail == args.len() || used + cost + marker > room {
                break;
            }
            used += cost;
            if head <= tail {
                head += 1;
            } else {
                tail += 1;
            }
        }
        write_words(f, envs, &program, &args[..head])?;
        write!(f, " …({} more)", args.len() - head - tail)?;
        for arg in &args[args.len() - tail..] {
            write!(f, " {arg}")?;
        }
        Ok(())
    }
}

/// `envs… program args…`.
fn write_words(
    f: &mut fmt::Formatter<'_>,
    envs: &[String],
    program: &str,
    args: &[String],
) -> fmt::Result {
    for env in envs {
        write!(f, "{env} ")?;
    }
    f.write_str(program)?;
    for arg in args {
        write!(f, " {arg}")?;
    }
    Ok(())
}

fn quote(f: &mut impl fmt::Write, s: &str) -> fmt::Result {
    let safe = !s.is_empty()
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'_' | b'-' | b'.' | b'/' | b':' | b'=' | b',' | b'+' | b'@' | b'%'
                )
        });
    if safe {
        return f.write_str(s);
    }
    f.write_str("'")?;
    for (i, part) in s.split('\'').enumerate() {
        if i > 0 {
            f.write_str("'\\''")?;
        }
        f.write_str(part)?;
    }
    f.write_str("'")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> Cmd {
        let mut cmd = Cmd::new("sh");
        cmd.args(["-c", script]).echo(false).dry_run(false);
        cmd
    }

    #[test]
    fn display_quotes_only_when_needed() {
        let mut cmd = Cmd::new("git");
        cmd.args(["log", "--format=%H %s", "it's", "", "a/b.c:d"]);
        assert_eq!(
            cmd.display().to_string(),
            r"git log '--format=%H %s' 'it'\''s' '' a/b.c:d"
        );
    }

    #[test]
    fn display_shows_cwd_and_env() {
        let mut cmd = Cmd::new("migrate");
        cmd.current_dir("/srv/my app")
            .env("DATABASE_URL", "postgres://u@h/db?x=1")
            .env_remove("PGPASSWORD")
            .arg("up");
        assert_eq!(
            cmd.display().to_string(),
            "cd '/srv/my app' && DATABASE_URL='postgres://u@h/db?x=1' migrate up"
        );
    }

    #[test]
    fn echo_width_keeps_short_lines() {
        let mut cmd = Cmd::new("ls");
        cmd.env("A", "1").arg("-l").echo_width(12);
        assert_eq!(cmd.display().to_string(), "A=1 ls -l");
    }

    #[test]
    fn echo_width_collapses_envs_then_middle_args() {
        let mut cmd = Cmd::new("rustc");
        cmd.current_dir("/w")
            .env("CARGO_PKG_NAME", "foo")
            .env("CARGO_PKG_VERSION", "1.0.0")
            .args(["--crate-name", "foo", "src/lib.rs"]);
        let line = |cmd: &mut Cmd, width| cmd.echo_width(width).display().to_string();
        assert_eq!(
            line(&mut cmd, 44),
            "cd /w && … rustc --crate-name foo src/lib.rs"
        );
        assert_eq!(
            line(&mut cmd, 43),
            "cd /w && … rustc --crate-name …(2 more)"
        );
        // The program and the directory stay, however narrow.
        assert_eq!(line(&mut cmd, 5), "cd /w && … rustc …(3 more)");
        for width in 0..80 {
            let shown = line(&mut cmd, width);
            assert!(
                shown.chars().count() <= width.max("cd /w && … rustc …(3 more)".chars().count()),
                "{width}: {shown}"
            );
        }
    }

    #[test]
    fn echo_label_replaces_the_line_in_dry_run() {
        let out = Cmd::new("rm")
            .args(["-rf", "/"])
            .echo_label("cleaning up")
            .dry_run(true)
            .run()
            .unwrap();
        assert!(out.dry_run);
        let mut cmd = Cmd::new("rm");
        assert_eq!(cmd.arg("x").echo_text(), "rm x");
        assert_eq!(cmd.echo_label("cleaning up").echo_text(), "cleaning up");
    }

    #[test]
    fn stdout_to_stderr_is_not_captured() {
        let out = sh("printf out").stdout(Redirect::Stderr).output().unwrap();
        assert!(out.success());
        assert!(out.stdout.is_empty());
    }

    #[test]
    fn output_captures_both_streams() {
        let out = sh("printf out; printf err >&2").output().unwrap();
        assert!(out.success());
        assert_eq!(out.stdout, b"out");
        assert_eq!(out.stderr, b"err");
        assert!(!out.dry_run);
    }

    #[test]
    fn run_inherits_by_default() {
        let out = sh("true").run().unwrap();
        assert!(out.stdout.is_empty() && out.stderr.is_empty());
    }

    #[test]
    fn null_discards() {
        let out = sh("echo hi; echo there >&2")
            .stdout(Redirect::Null)
            .stderr(Redirect::Null)
            .output()
            .unwrap();
        assert!(out.stdout.is_empty() && out.stderr.is_empty());
    }

    #[test]
    fn capture_stdout_returns_text_and_upgrades_redirect() {
        let s = sh("printf 'a\\nb\\n'")
            .stdout(Redirect::Null)
            .stderr(Redirect::Capture)
            .capture_stdout()
            .unwrap();
        assert_eq!(s, "a\nb\n");
    }

    #[test]
    fn tee_still_captures() {
        let out = sh("printf one; printf two >&2")
            .stdout(Redirect::Tee)
            .stderr(Redirect::Tee)
            .run()
            .unwrap();
        assert_eq!(out.stdout, b"one");
        assert_eq!(out.stderr, b"two");
    }

    #[test]
    fn exit_error_carries_code_and_stderr_tail() {
        let err = sh("echo progress; echo 'fatal: boom' >&2; exit 3")
            .stderr(Redirect::Capture)
            .capture_stdout()
            .unwrap_err();
        assert_eq!(err.code(), "cmd_exit");
        assert_eq!(err.exit_code(), Some(3));
        assert_eq!(err.stderr_tail(), Some("fatal: boom"));
        assert_eq!(err.program(), "sh");
        let msg = err.to_string();
        assert!(msg.starts_with("`sh` failed (exit status: 3):\n"), "{msg}");
        assert!(msg.ends_with("fatal: boom"), "{msg}");
    }

    #[test]
    fn exit_error_without_capture_has_no_tail() {
        let err = sh("exit 1").run().unwrap_err();
        assert_eq!(err.stderr_tail(), None);
        assert_eq!(err.to_string(), "`sh` failed (exit status: 1)");
    }

    #[test]
    fn allow_failure_returns_output() {
        let out = sh("printf x; exit 4").allow_failure().output().unwrap();
        assert_eq!(out.status.code(), Some(4));
        assert_eq!(out.stdout, b"x");
    }

    #[test]
    fn tail_is_bounded_and_starts_on_a_line() {
        let mut big = Vec::new();
        for i in 0..2000 {
            big.extend_from_slice(format!("line {i}\n").as_bytes());
        }
        let t = tail(&big);
        assert!(t.len() <= STDERR_TAIL_BYTES);
        assert!(t.starts_with("line "), "{}", &t[..20]);
        assert!(t.ends_with("line 1999"));
    }

    #[test]
    fn spawn_error() {
        let err = Cmd::new("definitely-not-a-program-7f3a")
            .echo(false)
            .dry_run(false)
            .run()
            .unwrap_err();
        assert_eq!(err.code(), "cmd_spawn");
        assert!(err.to_string().contains("definitely-not-a-program-7f3a"));
    }

    #[test]
    fn utf8_error() {
        let err = sh("printf '\\377'").capture_stdout().unwrap_err();
        assert_eq!(err.code(), "cmd_utf8");
    }

    #[test]
    fn large_output_on_both_streams_does_not_deadlock() {
        let out = sh("head -c 300000 /dev/zero; head -c 300000 /dev/zero >&2")
            .output()
            .unwrap();
        assert_eq!(out.stdout.len(), 300_000);
        assert_eq!(out.stderr.len(), 300_000);
    }

    #[test]
    fn timeout_kills_the_child() {
        let start = Instant::now();
        let err = sh("echo started >&2; exec sleep 5")
            .timeout(Duration::from_millis(200))
            .output()
            .unwrap_err();
        assert!(start.elapsed() < Duration::from_secs(3));
        assert_eq!(err.code(), "cmd_timeout");
        assert_eq!(err.stderr_tail(), Some("started"));
    }

    #[test]
    fn max_output_keeps_the_first_bytes_and_drains_the_rest() {
        // 200 KiB on each stream: more than a pipe holds, so a child left
        // blocked on a full pipe would hang the test.
        let out = sh("head -c 204800 /dev/zero; head -c 204800 /dev/zero >&2; printf done")
            .max_output(10)
            .output()
            .unwrap();
        assert_eq!(out.stdout, vec![0u8; 10]);
        assert_eq!(out.stderr.len(), 10);
        assert!(out.stdout_truncated && out.stderr_truncated);
        let out = sh("printf ok")
            .max_output(2)
            .timeout(Duration::from_secs(10))
            .output()
            .unwrap();
        assert_eq!(
            (out.stdout.as_slice(), out.stdout_truncated),
            (&b"ok"[..], false)
        );
    }

    #[test]
    fn timeout_not_hit() {
        let out = sh("printf ok; printf e >&2")
            .timeout(Duration::from_secs(10))
            .output()
            .unwrap();
        assert_eq!(out.stdout, b"ok");
        assert_eq!(out.stderr, b"e");
    }

    #[test]
    fn timeout_survives_grandchild_holding_the_pipe() {
        let start = Instant::now();
        let err = sh("sleep 5 & sleep 5")
            .timeout(Duration::from_millis(100))
            .output()
            .unwrap_err();
        assert_eq!(err.code(), "cmd_timeout");
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn dry_run_does_not_execute() {
        let dir = std::env::temp_dir().join(format!("bpaf-kit-dry-{}", std::process::id()));
        let out = Cmd::new("mkdir")
            .arg(&dir)
            .echo(false)
            .dry_run(true)
            .output()
            .unwrap();
        assert!(out.dry_run && out.success());
        assert!(!dir.exists());
        let s = Cmd::new("false").dry_run(true).capture_stdout().unwrap();
        assert_eq!(s, "");
    }

    // The only test that turns on the process-wide flag; every other test
    // that runs a command sets `dry_run` on it explicitly.
    #[test]
    fn dry_run_false_exempts_a_command_from_global_dry_run() {
        set_dry_run(true);
        let exempt = sh("printf ran").capture_stdout();
        let skipped = Cmd::new("sh")
            .args(["-c", "printf ran"])
            .echo(false)
            .output();
        set_dry_run(false);
        assert_eq!(exempt.unwrap(), "ran");
        let skipped = skipped.unwrap();
        assert!(skipped.dry_run && skipped.stdout.is_empty());
    }

    #[test]
    fn env_and_cwd_reach_the_child() {
        let dir = std::env::temp_dir();
        let s = sh("printf \"$KIT_TEST_VAR:$(pwd -P)\"")
            .env("KIT_TEST_VAR", "v")
            .current_dir(&dir)
            .capture_stdout()
            .unwrap();
        let want = format!("v:{}", dir.canonicalize().unwrap().display());
        assert_eq!(s, want);
    }

    #[cfg(feature = "miette")]
    #[test]
    fn miette_codes_match() {
        use miette::Diagnostic;
        for err in [
            sh("exit 1").run().unwrap_err(),
            sh("printf '\\377'").capture_stdout().unwrap_err(),
            Cmd::new("definitely-not-a-program-7f3a")
                .echo(false)
                .dry_run(false)
                .run()
                .unwrap_err(),
        ] {
            let diag = Diagnostic::code(&err).map(|c| c.to_string());
            assert_eq!(diag.as_deref(), Some(err.code()));
        }
    }

    #[test]
    fn bools() {
        assert_eq!(parse_bool("ON"), Some(true));
        assert_eq!(parse_bool("0"), Some(false));
        assert_eq!(parse_bool(""), Some(false));
        assert_eq!(parse_bool("nah"), None);
    }

    #[test]
    fn flag_set_overrides_env_default() {
        let flag = Flag::new("BPAF_KIT_TEST_UNSET_FLAG_9c1", true);
        assert!(flag.get());
        flag.set(false);
        assert!(!flag.get());
    }
}
