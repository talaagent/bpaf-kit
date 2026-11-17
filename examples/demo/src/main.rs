//! `demo`: a small CLI that uses every bpaf-kit module.
//!
//! ```text
//! demo exec -- ls -l          # run a command, echoed set -x style
//! demo exec --capture -- git rev-parse HEAD
//! demo --dry-run exec -- rm -rf build
//! demo services --format plain
//! demo greet                  # interactive prompt; Ctrl-C restores the cursor
//! demo --version --verbose
//! demo --man | man -l -
//! ```

use core::time::Duration;
use std::process::ExitCode;

use bpaf::Bpaf;
use bpaf_kit::cmd::{self, Cmd, Redirect};
use bpaf_kit::interrupt;
use bpaf_kit::log::LogArgs;
use bpaf_kit::report::{self, Coded};
use bpaf_kit::{Cli, suggest, table};

/// Example CLI for bpaf-kit
#[derive(Debug, Clone, Bpaf, Cli)]
#[bpaf(options)]
#[cli(log = "warn")]
struct Opts {
    /// Print commands instead of running them
    #[bpaf(long)]
    dry_run: bool,
    /// Don't echo commands before running them
    #[bpaf(long)]
    no_echo: bool,
    #[bpaf(external(bpaf_kit::log::log_args))]
    #[cli(log_args)]
    log: LogArgs,
    #[bpaf(external(command))]
    command: Command,
}

#[derive(Debug, Clone, Bpaf)]
enum Command {
    /// Run a program
    #[bpaf(command)]
    Exec {
        /// Return stdout as a string and print it afterwards
        #[bpaf(long)]
        capture: bool,
        /// Kill the program after this many seconds
        #[bpaf(long, argument("SECS"))]
        timeout: Option<u64>,
        /// The program and its arguments
        #[bpaf(positional("CMD"), strict, some("a program to run"))]
        argv: Vec<String>,
    },
    /// List some made-up services as a table
    #[bpaf(command)]
    Services {
        /// Table style
        #[bpaf(
            long,
            argument::<String>("STYLE"),
            parse(suggest::bpaf_enum),
            fallback(Style::Auto),
            display_fallback
        )]
        format: Style,
    },
    /// Ask for a name and greet it
    #[bpaf(command)]
    Greet,
}

#[derive(Debug, Clone, Copy, strum::EnumString, strum::VariantNames, strum::Display)]
#[strum(serialize_all = "kebab-case")]
enum Style {
    Auto,
    Boxed,
    Plain,
}

fn main() -> ExitCode {
    let opts = Opts::cli();
    if opts.dry_run {
        cmd::set_dry_run(true);
    }
    if opts.no_echo {
        cmd::set_echo(false);
    }
    match run(opts.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => report::report(&err),
    }
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    Cmd(#[from] cmd::Error),
    #[error("prompt failed: {0}")]
    Prompt(#[from] bpaf_kit::prompt::dialoguer::Error),
}

impl Coded for Error {
    fn code(&self) -> &str {
        match self {
            Self::Cmd(e) => e.code(),
            Self::Prompt(_) => "prompt",
        }
    }
}

fn run(command: Command) -> Result<(), Error> {
    match command {
        Command::Exec {
            capture,
            timeout,
            argv,
        } => {
            // `some(..)` makes bpaf reject an empty CMD.
            let Some((program, args)) = argv.split_first() else {
                return Ok(());
            };
            let mut c = Cmd::new(program);
            c.args(args);
            if let Some(secs) = timeout {
                c.timeout(Duration::from_secs(secs));
            }
            tracing::info!(program, "running");
            // Runs on Ctrl-C while the command is running.
            let _cleanup = interrupt::on_interrupt(|| eprintln!("demo: interrupted, cleaning up"));
            if capture {
                let out = c.capture_stdout()?;
                println!("captured {} bytes:\n{out}", out.len());
            } else {
                c.stderr(Redirect::Tee).run()?;
            }
            Ok(())
        }
        Command::Services { format } => {
            let headers = ["service", "status", "uptime"];
            let mut t = match format {
                Style::Auto => table::auto(headers),
                Style::Boxed => table::new(headers),
                Style::Plain => table::plain(headers),
            };
            t.add_row(["api", "up", "3d 4h"])
                .add_row(["worker", "degraded", "12m"])
                .add_row(["scheduler", "down", "-"]);
            println!("{t}");
            Ok(())
        }
        Command::Greet => {
            let name = bpaf_kit::prompt::optional_text("Your name")?;
            println!("hello, {}!", name.as_deref().unwrap_or("stranger"));
            Ok(())
        }
    }
}
