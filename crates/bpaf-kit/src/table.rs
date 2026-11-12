//! comfy-table presets (feature `table`).
//!
//! ```
//! let mut t = bpaf_kit::table::new(["name", "status"]);
//! t.add_row(["api", "up"]);
//! println!("{t}");
//! ```

use std::io::IsTerminal;

pub use comfy_table;
use comfy_table::{ContentArrangement, Table, presets};

/// A table with thin box-drawing borders that wraps to the terminal width.
pub fn new<I, T>(headers: I) -> Table
where
    I: IntoIterator<Item = T>,
    T: Into<comfy_table::Cell>,
{
    let mut table = Table::new();
    table
        .load_preset(presets::UTF8_FULL_CONDENSED)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(headers);
    table
}

/// A borderless, unwrapped table: columns separated by spaces, for output
/// that gets piped to `grep`, `awk` and friends.
pub fn plain<I, T>(headers: I) -> Table
where
    I: IntoIterator<Item = T>,
    T: Into<comfy_table::Cell>,
{
    let mut table = Table::new();
    table
        .load_preset(presets::NOTHING)
        .set_content_arrangement(ContentArrangement::Disabled)
        .force_no_tty()
        .set_header(headers);
    table
}

/// [`new`] when stdout is a terminal, [`plain`] otherwise.
pub fn auto<I, T>(headers: I) -> Table
where
    I: IntoIterator<Item = T>,
    T: Into<comfy_table::Cell>,
{
    if std::io::stdout().is_terminal() {
        new(headers)
    } else {
        plain(headers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boxed() {
        let mut t = new(["name", "n"]);
        t.add_row(["a", "1"]).add_row(["bb", "22"]);
        t.set_width(40);
        let s = t.to_string();
        assert!(s.starts_with('┌'), "{s}");
        assert!(s.contains("│ bb   ┆ 22 │"), "{s}");
    }

    #[test]
    fn plain_has_no_borders() {
        let mut t = plain(["name", "n"]);
        t.add_row(["a", "1"]);
        let lines: Vec<_> = t
            .to_string()
            .lines()
            .map(str::trim_end)
            .map(str::to_owned)
            .collect();
        assert_eq!(lines, [" name  n", " a     1"]);
    }
}
