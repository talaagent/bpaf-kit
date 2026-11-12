//! Build info for `--version` and `--version --verbose`.
//!
//! [`build_info!`](crate::build_info) captures, at the call site's compile
//! time, the package version and the values `bpaf-kit-build` bakes in from
//! `build.rs` (git rev, dirty flag, build date, target). Without the build
//! script those are simply unknown.
//!
//! ```
//! let info = bpaf_kit::build_info!();
//! println!("{} {}", info.name, info.short());
//! ```
//!
//! # Runtime override file
//!
//! Baking the git rev into the binary means every commit changes the
//! compiler's inputs, which defeats Nix's (and any other) build cache. The
//! alternative is to build the binary once without that info and let a cheap
//! wrapper derivation supply it at run time. [`BuildInfo::with_override`]
//! (which `#[derive(Cli)]` calls when handling `--version`) looks for a
//! version file:
//!
//! 1. at the path in `<NAME>_VERSION_FILE`, where `<NAME>` is the binary
//!    name upper-cased with `-` turned into `_` (`my-tool` →
//!    `MY_TOOL_VERSION_FILE`). Per-binary, so a tool that runs another
//!    bpaf-kit tool doesn't leak its version into the child;
//! 2. else at `<exe dir>/../share/<name>/version.env`, for packages that
//!    copy the binary next to a `share/` directory.
//!
//! The file holds `key=value` lines; `#` starts a comment. Every key is
//! optional and overrides the compiled-in value:
//!
//! ```text
//! version=0.4.1
//! rev=3f9c2a1d7e0b4c55a1e2f3d4c5b6a7980112233
//! dirty=false
//! date=2026-11-12
//! target=x86_64-unknown-linux-gnu
//! ```
//!
//! A Nix wrapper that provides it (`self` is the flake, `tool` the
//! unwrapped package):
//!
//! ```nix
//! let
//!   d = self.lastModifiedDate; # "YYYYMMDDhhmmss"
//!   versionFile = pkgs.writeText "my-tool-version.env" ''
//!     rev=${self.rev or self.dirtyRev or "unknown"}
//!     dirty=${if self ? dirtyRev then "true" else "false"}
//!     date=${builtins.substring 0 4 d}-${builtins.substring 4 2 d}-${builtins.substring 6 2 d}
//!   '';
//! in
//! pkgs.runCommand "my-tool-${tool.version}" { nativeBuildInputs = [ pkgs.makeWrapper ]; } ''
//!   install -Dm644 ${versionFile} $out/share/my-tool/version.env
//!   makeWrapper ${tool}/bin/my-tool $out/bin/my-tool \
//!     --set MY_TOOL_VERSION_FILE $out/share/my-tool/version.env
//! ''
//! ```
//!
//! The file is read only when the version is asked for, never on a normal
//! start.

use core::fmt;
use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Build information for one binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildInfo {
    /// Binary name.
    pub name: Cow<'static, str>,
    /// Package version.
    pub version: Cow<'static, str>,
    /// Full commit hash.
    pub git_rev: Option<Cow<'static, str>>,
    /// Whether tracked files had uncommitted changes.
    pub git_dirty: Option<bool>,
    /// UTC build date, `YYYY-MM-DD`.
    pub build_date: Option<Cow<'static, str>>,
    /// Target triple.
    pub target: Option<Cow<'static, str>>,
    /// The override file the values came from, if any.
    pub override_file: Option<PathBuf>,
}

/// Build info for the calling crate, from compile-time env vars.
///
/// The name is `CARGO_BIN_NAME` when compiling a binary (falling back to
/// `CARGO_PKG_NAME`).
#[macro_export]
macro_rules! build_info {
    () => {
        $crate::version::BuildInfo::compiled(
            match ::core::option_env!("CARGO_BIN_NAME") {
                ::core::option::Option::Some(name) => name,
                ::core::option::Option::None => ::core::env!("CARGO_PKG_NAME"),
            },
            ::core::env!("CARGO_PKG_VERSION"),
            ::core::option_env!("BPAF_KIT_GIT_REV"),
            ::core::option_env!("BPAF_KIT_GIT_DIRTY"),
            ::core::option_env!("BPAF_KIT_BUILD_DATE"),
            ::core::option_env!("BPAF_KIT_TARGET"),
        )
    };
}

impl BuildInfo {
    /// Build info from compile-time strings; see
    /// [`build_info!`](crate::build_info).
    #[must_use]
    pub fn compiled(
        name: &'static str,
        version: &'static str,
        git_rev: Option<&'static str>,
        git_dirty: Option<&'static str>,
        build_date: Option<&'static str>,
        target: Option<&'static str>,
    ) -> Self {
        let some = |s: Option<&'static str>| s.filter(|s| !s.is_empty()).map(Cow::Borrowed);
        Self {
            name: Cow::Borrowed(name),
            version: Cow::Borrowed(version),
            git_rev: some(git_rev),
            git_dirty: git_dirty.and_then(crate::cmd::parse_bool),
            build_date: some(build_date),
            target: some(target),
            override_file: None,
        }
    }

    /// The env var that points at this binary's override file:
    /// `MY_TOOL_VERSION_FILE` for `my-tool`.
    #[must_use]
    pub fn override_env_var(&self) -> String {
        let mut var: String = self
            .name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_uppercase()
                } else {
                    '_'
                }
            })
            .collect();
        var.push_str("_VERSION_FILE");
        var
    }

    /// Apply the runtime override file, if one is found (see the module
    /// docs). A missing or unreadable file leaves `self` unchanged.
    #[must_use]
    pub fn with_override(self) -> Self {
        let from_env = std::env::var_os(self.override_env_var()).map(PathBuf::from);
        let path = from_env.or_else(|| {
            let exe = std::env::current_exe().ok()?;
            let share = exe.parent()?.parent()?.join("share");
            Some(share.join(&*self.name).join("version.env"))
        });
        match path {
            Some(path) => self.with_override_file(&path),
            None => self,
        }
    }

    /// Apply the override file at `path`, if it can be read.
    #[must_use]
    pub fn with_override_file(self, path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let mut info = self.with_override_text(&text);
                info.override_file = Some(path.to_owned());
                info
            }
            Err(_) => self,
        }
    }

    /// Apply override `key=value` text.
    #[must_use]
    pub fn with_override_text(mut self, text: &str) -> Self {
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or_default().trim();
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            let owned = || (!value.is_empty()).then(|| Cow::Owned(value.to_owned()));
            match key.trim() {
                "version" if !value.is_empty() => self.version = Cow::Owned(value.to_owned()),
                "rev" => self.git_rev = owned(),
                "dirty" => self.git_dirty = crate::cmd::parse_bool(value),
                "date" => self.build_date = owned(),
                "target" => self.target = owned(),
                _ => {}
            }
        }
        self
    }

    /// The first 9 characters of the git rev.
    #[must_use]
    pub fn short_rev(&self) -> Option<&str> {
        self.git_rev.as_deref().map(|r| r.get(..9).unwrap_or(r))
    }

    /// One line: `0.1.0 (3f9c2a1d7-dirty 2026-11-12)`.
    #[must_use]
    pub fn short(&self) -> Short<'_> {
        Short(self)
    }

    /// Several lines: the name and short version, then every field.
    #[must_use]
    pub fn verbose(&self) -> Verbose<'_> {
        Verbose(self)
    }

    /// Print `name short` (or the verbose form) to stdout.
    pub fn print(&self, verbose: bool) {
        if verbose {
            print!("{}", self.verbose());
        } else {
            println!("{} {}", self.name, self.short());
        }
    }
}

/// See [`BuildInfo::short`].
pub struct Short<'a>(&'a BuildInfo);

impl fmt::Display for Short<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let info = self.0;
        f.write_str(&info.version)?;
        let rev = info.short_rev();
        if rev.is_none() && info.build_date.is_none() {
            return Ok(());
        }
        f.write_str(" (")?;
        if let Some(rev) = rev {
            f.write_str(rev)?;
            if info.git_dirty == Some(true) {
                f.write_str("-dirty")?;
            }
            if info.build_date.is_some() {
                f.write_str(" ")?;
            }
        }
        if let Some(date) = &info.build_date {
            f.write_str(date)?;
        }
        f.write_str(")")
    }
}

/// See [`BuildInfo::verbose`].
pub struct Verbose<'a>(&'a BuildInfo);

impl fmt::Display for Verbose<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const UNKNOWN: &str = "unknown";
        let info = self.0;
        writeln!(f, "{} {}", info.name, info.short())?;
        writeln!(f, "version:    {}", info.version)?;
        writeln!(
            f,
            "commit:     {}",
            info.git_rev.as_deref().unwrap_or(UNKNOWN)
        )?;
        let dirty = match info.git_dirty {
            Some(true) => "yes",
            Some(false) => "no",
            None => UNKNOWN,
        };
        writeln!(f, "dirty:      {dirty}")?;
        writeln!(
            f,
            "build date: {}",
            info.build_date.as_deref().unwrap_or(UNKNOWN)
        )?;
        writeln!(
            f,
            "target:     {}",
            info.target.as_deref().unwrap_or(UNKNOWN)
        )?;
        match &info.override_file {
            Some(path) => writeln!(f, "info from:  {}", path.display()),
            None => writeln!(f, "info from:  compiled in"),
        }
    }
}

/// If `args` (without the program name) ask only for the version —
/// `--version` or `-V`, optionally with `--verbose` — return whether the
/// verbose form was asked for.
pub fn version_request<I, S>(args: I) -> Option<bool>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let (mut version, mut verbose) = (false, false);
    for (i, arg) in args.into_iter().enumerate() {
        match arg.as_ref().to_str() {
            _ if i >= 2 => return None,
            Some("--version" | "-V") => version = true,
            Some("--verbose") => verbose = true,
            _ => return None,
        }
    }
    version.then_some(verbose)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> BuildInfo {
        BuildInfo::compiled(
            "my-tool",
            "0.3.0",
            Some("3f9c2a1d7e0b4c55a1e2f3d4c5b6a79801122334"),
            Some("true"),
            Some("2026-11-12"),
            Some("x86_64-unknown-linux-gnu"),
        )
    }

    #[test]
    fn short_forms() {
        assert_eq!(
            info().short().to_string(),
            "0.3.0 (3f9c2a1d7-dirty 2026-11-12)"
        );
        let bare = BuildInfo::compiled("t", "1.0.0", None, None, None, None);
        assert_eq!(bare.short().to_string(), "1.0.0");
        let date_only = BuildInfo::compiled("t", "1.0.0", Some(""), None, Some("2026-01-02"), None);
        assert_eq!(date_only.short().to_string(), "1.0.0 (2026-01-02)");
        let clean = BuildInfo::compiled("t", "1.0.0", Some("abc"), Some("false"), None, None);
        assert_eq!(clean.short().to_string(), "1.0.0 (abc)");
    }

    #[test]
    fn verbose_form() {
        let text = BuildInfo::compiled("t", "1.0.0", None, None, None, None)
            .verbose()
            .to_string();
        assert_eq!(
            text,
            "t 1.0.0\nversion:    1.0.0\ncommit:     unknown\ndirty:      unknown\n\
             build date: unknown\ntarget:     unknown\ninfo from:  compiled in\n"
        );
        assert!(info().verbose().to_string().contains("dirty:      yes\n"));
    }

    #[test]
    fn macro_uses_caller_package() {
        let info = crate::build_info!();
        assert_eq!(info.name, "bpaf-kit");
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn env_var_name() {
        assert_eq!(info().override_env_var(), "MY_TOOL_VERSION_FILE");
    }

    #[test]
    fn override_text() {
        let i = info().with_override_text(
            "# from the wrapper\nrev = deadbeef\ndirty=no\n\nversion=0.3.1 # bumped\nnonsense\nother=1\n",
        );
        assert_eq!(i.git_rev.as_deref(), Some("deadbeef"));
        assert_eq!(i.git_dirty, Some(false));
        assert_eq!(i.version, "0.3.1");
        assert_eq!(i.build_date.as_deref(), Some("2026-11-12"));
        let cleared = info().with_override_text("rev=\ndate=");
        assert_eq!(cleared.git_rev, None);
        assert_eq!(cleared.build_date, None);
    }

    #[test]
    fn override_file() {
        let path =
            std::env::temp_dir().join(format!("bpaf-kit-version-{}.env", std::process::id()));
        std::fs::write(&path, "rev=0123456789abcdef\n").unwrap();
        let i = info().with_override_file(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(i.short_rev(), Some("012345678"));
        assert_eq!(i.override_file.as_deref(), Some(path.as_path()));
        assert!(
            i.verbose()
                .to_string()
                .contains(&path.display().to_string())
        );
        // Missing file: unchanged.
        assert_eq!(info().with_override_file(&path), info());
    }

    #[test]
    fn requests() {
        assert_eq!(version_request(["--version"]), Some(false));
        assert_eq!(version_request(["-V", "--verbose"]), Some(true));
        assert_eq!(version_request(["--verbose", "--version"]), Some(true));
        assert_eq!(version_request(["--verbose"]), None);
        assert_eq!(version_request(Vec::<&str>::new()), None);
        assert_eq!(version_request(["build", "--version"]), None);
        assert_eq!(version_request(["--version", "--verbose", "x"]), None);
    }
}
