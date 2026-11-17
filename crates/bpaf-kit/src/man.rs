//! Manpages from the bpaf parser (feature `man`).
//!
//! `#[derive(Cli)]` answers `<bin> --man` (as the only argument) by printing
//! the page and exiting, so packaging can do
//! `my-tool --man > share/man/man1/my-tool.1`.
//!
//! With subcommands, the page reads `NAME`, `SYNOPSIS` (one line per
//! command), `DESCRIPTION` (the top-level header, options and commands),
//! then a section per subcommand with its description, `USAGE:` line and
//! options. bpaf's own layout repeats `NAME` and `SYNOPSIS` per command
//! under a `.SH <NAME>\ ` heading; [`render`] rearranges it.

use bpaf::OptionParser;
pub use bpaf::doc::Section;

use crate::version::BuildInfo;

/// The argument that asks for the manpage.
pub const FLAG: &str = "--man";

/// Render the manpage (roff) for `parser` in section 1. The footer shows
/// `<name> <version>` and the build date when known.
pub fn render<T>(parser: &OptionParser<T>, info: &BuildInfo) -> String {
    render_section(parser, info, Section::General)
}

/// [`render`] in another section.
pub fn render_section<T>(parser: &OptionParser<T>, info: &BuildInfo, section: Section) -> String {
    let vendor = format!("{} {}", info.name, info.version);
    tidy(&parser.render_manpage(
        &*info.name,
        section,
        info.build_date.as_deref(),
        Some(&vendor),
        None,
    ))
}

/// One `.SH` section of bpaf's page: the heading line and what follows it.
struct Chunk<'a> {
    heading: &'a str,
    body: Vec<&'a str>,
}

impl Chunk<'_> {
    /// A per-command heading: bpaf writes the command path with a trailing
    /// escaped space (`.SH DEMO\ EXEC\ `).
    fn is_command(&self) -> bool {
        self.heading.ends_with("\\ ")
    }

    fn is(&self, name: &str) -> bool {
        self.heading.strip_prefix(".SH ") == Some(name)
    }
}

/// Rearrange a multi-command page from bpaf (see the module docs). Pages
/// without per-command sections (single-command parsers, or a bpaf that
/// no longer writes them) are returned unchanged.
fn tidy(page: &str) -> String {
    let mut preamble = Vec::new();
    let mut chunks: Vec<Chunk<'_>> = Vec::new();
    for line in page.lines() {
        if line.starts_with(".SH ") {
            chunks.push(Chunk {
                heading: line,
                body: Vec::new(),
            });
        } else if let Some(chunk) = chunks.last_mut() {
            chunk.body.push(line);
        } else {
            preamble.push(line);
        }
    }
    let Some(first) = chunks.iter().position(Chunk::is_command) else {
        return page.to_owned();
    };
    let (overview, commands) = chunks.split_at(first);

    let mut out: Vec<&str> = preamble;
    // Group each command heading with the NAME and SYNOPSIS after it.
    let mut groups = commands;
    let mut root = true;
    while let Some((heading, rest)) = groups.split_first() {
        let end = rest
            .iter()
            .position(Chunk::is_command)
            .unwrap_or(rest.len());
        let (own, next) = rest.split_at(end);
        let name = own.iter().find(|c| c.is("NAME"));
        let synopsis = own.iter().find(|c| c.is("SYNOPSIS"));
        let others = own.iter().filter(|c| !c.is("NAME") && !c.is("SYNOPSIS"));
        if root {
            // NAME, the overall SYNOPSIS, then this command's synopsis
            // (already in the overall one) gives way to DESCRIPTION.
            if let Some(name) = name {
                out.push(name.heading);
                out.extend(&name.body);
            }
            for chunk in overview {
                out.push(chunk.heading);
                out.extend(&chunk.body);
            }
            if let Some(synopsis) = synopsis {
                out.push(".SH DESCRIPTION");
                out.extend(after_usage(&synopsis.body));
            }
        } else {
            out.push(heading.heading.trim_end_matches("\\ "));
            if let Some(name) = name {
                out.push(".PP");
                out.extend(name.body.iter().map(|l| description(l)));
            }
            if let Some(synopsis) = synopsis {
                out.push(".SS USAGE:");
                out.extend(&synopsis.body);
            }
        }
        for chunk in others {
            out.push(chunk.heading);
            out.extend(&chunk.body);
        }
        root = false;
        groups = next;
    }
    let mut page = out.join("\n");
    page.push('\n');
    page
}

/// The lines after the usage text that opens a SYNOPSIS section.
fn after_usage<'a>(body: &'a [&'a str]) -> &'a [&'a str] {
    let start = body
        .iter()
        .position(|l| l.starts_with('.'))
        .unwrap_or(body.len());
    &body[start..]
}

/// A NAME line, `\fR<app> \- \fP<description>`, without the app name,
/// which bpaf writes even for subcommands.
fn description(line: &str) -> &str {
    line.strip_prefix("\\fR")
        .and_then(|l| l.split_once(" \\- \\fP"))
        .map_or(line, |(_, descr)| descr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpaf::Parser;

    #[test]
    fn renders_roff() {
        let parser = bpaf::long("name")
            .help("Who to greet")
            .argument::<String>("NAME")
            .to_options()
            .descr("Greets people");
        let info = BuildInfo::compiled("greet", "1.2.3", None, None, Some("2026-11-12"), None);
        let page = render(&parser, &info);
        assert!(page.starts_with(".ie \\n(.g .ds Aq"), "{page}");
        assert!(
            page.contains(".TH greet 1 2026-11-12 greet\\ 1.2.3"),
            "{page}"
        );
        assert!(page.contains("Who to greet"), "{page}");
        // One command: bpaf's layout is kept.
        assert_eq!(page.matches(".SH SYNOPSIS").count(), 1, "{page}");
        assert!(!page.contains("DESCRIPTION"), "{page}");
    }

    /// Example tool
    #[derive(Debug, Clone, bpaf::Bpaf)]
    #[bpaf(options, header("Runs things."), footer("See the docs."))]
    #[allow(dead_code, reason = "only rendered")]
    struct Tool {
        /// Say more
        verbose: bool,
        #[bpaf(external(tool_cmd))]
        cmd: ToolCmd,
    }

    #[derive(Debug, Clone, bpaf::Bpaf)]
    #[allow(dead_code, reason = "only rendered")]
    enum ToolCmd {
        /// Run a program
        #[bpaf(command)]
        Exec {
            /// The program and its arguments
            #[bpaf(positional("CMD"), many)]
            argv: Vec<String>,
        },
        /// List things
        #[bpaf(command)]
        List {
            /// Everything
            all: bool,
        },
    }

    /// The page for a parser with subcommands, against
    /// `tests/golden/man.1`. `BPAF_KIT_BLESS=1` rewrites the file.
    #[test]
    fn golden_with_subcommands() {
        const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/man.1");
        let info = BuildInfo::compiled("tool", "1.0.0", None, None, Some("2026-11-12"), None);
        let page = render(&tool(), &info);
        if std::env::var_os("BPAF_KIT_BLESS").is_some() {
            std::fs::write(GOLDEN, &page).unwrap();
        }
        let golden = std::fs::read_to_string(GOLDEN).unwrap();
        assert_eq!(page, golden, "BPAF_KIT_BLESS=1 updates the golden file");
        assert_eq!(page.matches(".SH SYNOPSIS").count(), 1, "{page}");
        assert_eq!(page.matches(".SH NAME").count(), 1, "{page}");
        assert!(!page.contains("\\ \n"), "{page}");
    }

    #[test]
    fn description_drops_the_app_name() {
        assert_eq!(description("\\fRtool \\- \\fP\\fRList\\fP"), "\\fRList\\fP");
        assert_eq!(description("plain"), "plain");
    }
}
