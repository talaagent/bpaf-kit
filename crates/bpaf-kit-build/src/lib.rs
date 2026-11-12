//! Build-script helper for `bpaf_kit::version`.
//!
//! Call [`emit`] from your binary crate's `build.rs` `main`:
//!
//! ```no_run
//! bpaf_kit_build::emit();
//! ```
//!
//! It sets these compile-time env vars, which `bpaf_kit::build_info!()` reads
//! with `option_env!`:
//!
//! | var                   | value                                      |
//! |-----------------------|--------------------------------------------|
//! | `BPAF_KIT_GIT_REV`    | full commit hash (`git rev-parse HEAD`)    |
//! | `BPAF_KIT_GIT_DIRTY`  | `true` / `false` (tracked files modified)  |
//! | `BPAF_KIT_BUILD_DATE` | `YYYY-MM-DD` (UTC)                         |
//! | `BPAF_KIT_TARGET`     | the target triple                          |
//!
//! Each value can be forced by setting the same variable in the build
//! environment, which is how sandboxed builds without a `.git` directory
//! (Nix, vendored tarballs) pass them in. `SOURCE_DATE_EPOCH` is honoured for
//! the build date so reproducible builds stay reproducible; Nix's
//! placeholder value (1980-01-01) leaves the date unset.
//!
//! The helper has no dependencies and runs at most three short `git`
//! commands. If `git` or the repository is missing, the value is left unset
//! and the binary reports it as unknown.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Env var carrying the full commit hash.
pub const GIT_REV: &str = "BPAF_KIT_GIT_REV";
/// Env var carrying `true`/`false` for uncommitted changes to tracked files.
pub const GIT_DIRTY: &str = "BPAF_KIT_GIT_DIRTY";
/// Env var carrying the UTC build date as `YYYY-MM-DD`.
pub const BUILD_DATE: &str = "BPAF_KIT_BUILD_DATE";
/// Env var carrying the target triple.
pub const TARGET: &str = "BPAF_KIT_TARGET";

/// The `SOURCE_DATE_EPOCH` Nix uses when it has no real timestamp
/// (1980-01-01); treated as "no build date".
const NIX_PLACEHOLDER_EPOCH: u64 = 315_532_800;

/// Build information collected by [`collect`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Info {
    /// Full commit hash.
    pub git_rev: Option<String>,
    /// Whether tracked files had uncommitted changes.
    pub git_dirty: Option<bool>,
    /// UTC build date, `YYYY-MM-DD`.
    pub build_date: Option<String>,
    /// Target triple.
    pub target: Option<String>,
    /// Files whose change should rerun the build script.
    pub watch: Vec<PathBuf>,
}

/// Collect build info and print the `cargo:` directives that bake it in.
pub fn emit() {
    let info = collect();
    for var in [GIT_REV, GIT_DIRTY, BUILD_DATE, "SOURCE_DATE_EPOCH"] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    for path in &info.watch {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    if let Some(rev) = &info.git_rev {
        println!("cargo:rustc-env={GIT_REV}={rev}");
    }
    if let Some(dirty) = info.git_dirty {
        println!("cargo:rustc-env={GIT_DIRTY}={dirty}");
    }
    if let Some(date) = &info.build_date {
        println!("cargo:rustc-env={BUILD_DATE}={date}");
    }
    if let Some(target) = &info.target {
        println!("cargo:rustc-env={TARGET}={target}");
    }
}

/// Collect build info without printing anything.
///
/// Explicit env vars win over `git`; `git` runs in `CARGO_MANIFEST_DIR` (or
/// the current directory outside a build script).
#[must_use]
pub fn collect() -> Info {
    let dir = env::var_os("CARGO_MANIFEST_DIR").map_or_else(|| PathBuf::from("."), PathBuf::from);
    let mut info = Info {
        git_rev: non_empty_env(GIT_REV),
        git_dirty: non_empty_env(GIT_DIRTY).and_then(|v| parse_bool(&v)),
        build_date: non_empty_env(BUILD_DATE).or_else(|| {
            let secs = match non_empty_env("SOURCE_DATE_EPOCH") {
                // Nix sets 1980-01-01 as a placeholder: no real date to report.
                Some(epoch) => epoch
                    .trim()
                    .parse()
                    .ok()
                    .filter(|&s| s != NIX_PLACEHOLDER_EPOCH)?,
                None => std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()?
                    .as_secs(),
            };
            Some(date_from_unix(secs))
        }),
        target: non_empty_env(TARGET).or_else(|| non_empty_env("TARGET")),
        watch: Vec::new(),
    };

    if info.git_rev.is_none() || info.git_dirty.is_none() {
        // `--git-dir` and `--git-common-dir` differ inside a linked worktree:
        // HEAD and the index live in the former, branch refs in the latter.
        if let Some(out) = git(
            &dir,
            &[
                "rev-parse",
                "--absolute-git-dir",
                "--git-common-dir",
                "HEAD",
            ],
        ) {
            let mut lines = out.lines();
            let git_dir = lines.next().map(PathBuf::from);
            let common = lines.next().map(|c| {
                let c = Path::new(c);
                if c.is_absolute() {
                    c.to_path_buf()
                } else {
                    dir.join(c)
                }
            });
            let rev = lines.next().filter(|r| is_hex(r)).map(str::to_owned);
            if let Some(git_dir) = git_dir {
                info.watch.push(git_dir.join("HEAD"));
                info.watch.push(git_dir.join("index"));
                if let Some(head_ref) = git(&dir, &["symbolic-ref", "-q", "HEAD"]) {
                    let common = common.unwrap_or_else(|| git_dir.clone());
                    info.watch.push(common.join(head_ref.trim()));
                    info.watch.push(common.join("packed-refs"));
                }
            }
            info.watch.retain(|p| p.exists());
            if info.git_rev.is_none() {
                info.git_rev = rev;
            }
            if info.git_dirty.is_none() && info.git_rev.is_some() {
                info.git_dirty = git(&dir, &["status", "--porcelain", "--untracked-files=no"])
                    .map(|s| !s.trim().is_empty());
            }
        }
    }
    info
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

fn non_empty_env(key: &str) -> Option<String> {
    env::var(key).ok().filter(|v| !v.trim().is_empty())
}

fn is_hex(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Parse the boolean spellings accepted in env vars and version files.
#[must_use]
pub fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Format a Unix timestamp as a UTC `YYYY-MM-DD` date.
#[must_use]
pub fn date_from_unix(secs: u64) -> String {
    // Howard Hinnant's days-to-civil algorithm, unsigned since we start at 1970.
    let days = secs / 86_400;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(date_from_unix(0), "1970-01-01");
        assert_eq!(date_from_unix(951_782_400), "2000-02-29");
        assert_eq!(date_from_unix(1_709_164_800), "2024-02-29");
        assert_eq!(date_from_unix(1_798_761_599), "2026-12-31");
        assert_eq!(date_from_unix(1_798_761_600), "2027-01-01");
    }

    #[test]
    fn bools() {
        assert_eq!(parse_bool("TRUE"), Some(true));
        assert_eq!(parse_bool(" 0 "), Some(false));
        assert_eq!(parse_bool("maybe"), None);
    }

    #[test]
    fn collects_from_this_repo() {
        // The test runs inside the workspace checkout, so git should answer
        // unless the env forces values or git is unavailable.
        let info = collect();
        if let Some(rev) = &info.git_rev {
            assert!(is_hex(rev), "{rev}");
            assert!(info.git_dirty.is_some());
        }
        // Unset only under Nix's placeholder SOURCE_DATE_EPOCH.
        if let Some(date) = info.build_date {
            assert_eq!(date.len(), 10, "{date}");
        }
    }
}
