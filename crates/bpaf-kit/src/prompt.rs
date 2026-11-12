//! dialoguer helpers (feature `prompt`).
//!
//! Each helper installs the [`interrupt`](crate::interrupt) handler first,
//! so Ctrl-C during a prompt never leaves the cursor hidden.

pub use dialoguer;
use dialoguer::{Confirm, Input, theme::ColorfulTheme};

/// The theme the helpers use.
#[must_use]
pub fn theme() -> ColorfulTheme {
    ColorfulTheme::default()
}

/// Ask for a line of text; blank means `None`. Avoids the `()` that dialoguer
/// shows for an empty default.
///
/// # Errors
///
/// When the terminal can't be read.
pub fn optional_text(prompt: &str) -> dialoguer::Result<Option<String>> {
    let _ = crate::interrupt::install();
    let s: String = Input::with_theme(&theme())
        .with_prompt(format!("{prompt} (blank to skip)"))
        .allow_empty(true)
        .interact_text()?;
    let s = s.trim();
    Ok((!s.is_empty()).then(|| s.to_owned()))
}

/// Ask for a non-empty line of text.
///
/// # Errors
///
/// When the terminal can't be read.
pub fn text(prompt: &str) -> dialoguer::Result<String> {
    let _ = crate::interrupt::install();
    Input::with_theme(&theme())
        .with_prompt(prompt)
        .interact_text()
}

/// Ask a yes/no question.
///
/// # Errors
///
/// When the terminal can't be read.
pub fn confirm(prompt: &str, default: bool) -> dialoguer::Result<bool> {
    let _ = crate::interrupt::install();
    Confirm::with_theme(&theme())
        .with_prompt(prompt)
        .default(default)
        .interact()
}
