//! comfy-table presets (feature `table`).
//!
//! ```
//! let mut t = bpaf_kit::table::new(["name", "status"]);
//! t.add_row(["api", "up"]);
//! println!("{t}");
//! ```

use alloc::string::{String, ToString};
use core::fmt;
use std::io::IsTerminal;

pub use comfy_table;
use comfy_table::{Cell, CellAlignment, ContentArrangement, Table, presets};

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

/// A borderless table for reports read by people and by `awk` alike:
/// columns two spaces apart, cells that start with a digit (`12`, `1.5s`,
/// `3 units`) right-aligned except in the last column, and no trailing
/// whitespace.
///
/// ```
/// let mut t = bpaf_kit::table::compact(["time", "crate"]);
/// t.add_row(["1.0s", "serde"]).add_row(["10.2s", "syn"]);
/// assert_eq!(t.to_string(), "time   crate\n 1.0s  serde\n10.2s  syn");
/// ```
pub fn compact<I, T>(headers: I) -> Compact
where
    I: IntoIterator<Item = T>,
    T: Into<Cell>,
{
    let mut compact = Compact {
        table: plain(headers),
    };
    compact.pad();
    compact
}

/// The table from [`compact`]. Its [`Display`](fmt::Display) prints the
/// lines without trailing whitespace, joined by `\n`, with no final
/// newline (like [`Table`]).
#[derive(Debug)]
pub struct Compact {
    table: Table,
}

impl Compact {
    /// Add a row, right-aligning the cells that start with an ASCII digit,
    /// except in the last column.
    pub fn add_row<I, T>(&mut self, row: I) -> &mut Self
    where
        I: IntoIterator<Item = T>,
        T: ToString,
    {
        let last = self.table.column_count().saturating_sub(1);
        let cells = row.into_iter().enumerate().map(|(i, cell)| {
            let text = cell.to_string();
            let numeric = i < last && text.starts_with(|c: char| c.is_ascii_digit());
            let cell = Cell::new(text);
            if numeric {
                cell.set_alignment(CellAlignment::Right)
            } else {
                cell
            }
        });
        self.table.add_row(cells);
        self.pad();
        self
    }

    /// The underlying table.
    #[must_use]
    pub const fn table(&self) -> &Table {
        &self.table
    }

    /// The underlying table, for column constraints, cell styles and the
    /// like. Rows added through it are not aligned automatically.
    pub const fn table_mut(&mut self) -> &mut Table {
        &mut self.table
    }

    /// The rendered lines, without trailing whitespace.
    pub fn lines(&self) -> impl Iterator<Item = String> {
        self.table.lines().map(|line| match line.trim_end().len() {
            n if n == line.len() => line,
            n => line[..n].to_string(),
        })
    }

    /// Two spaces after each column but the last.
    fn pad(&mut self) {
        let last = self.table.column_count().saturating_sub(1);
        for (i, column) in self.table.column_iter_mut().enumerate() {
            column.set_padding((0, if i == last { 0 } else { 2 }));
        }
    }
}

impl fmt::Display for Compact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, line) in self.lines().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            f.write_str(&line)?;
        }
        Ok(())
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

    #[test]
    fn compact_aligns_numbers_and_trims() {
        let mut t = compact(["time", "count", "crate"]);
        t.add_row(["1.0s", "3", "a"])
            .add_row(["10.0s", "12", "b"])
            .add_row(["-", "n/a", "3 units"]);
        assert_eq!(
            t.to_string(),
            "time   count  crate\n 1.0s      3  a\n10.0s     12  b\n-      n/a    3 units"
        );
        assert!(t.lines().all(|l| l == l.trim_end()));
    }

    #[test]
    fn compact_single_column() {
        let mut t = compact(["n"]);
        t.add_row([1]).add_row([22]);
        assert_eq!(t.to_string(), "n\n1\n22");
    }
}
