//! Manpages from the bpaf parser (feature `man`).
//!
//! `#[derive(Cli)]` answers `<bin> --man` (as the only argument) by printing
//! the page and exiting, so packaging can do
//! `my-tool --man > share/man/man1/my-tool.1`.

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
    parser.render_manpage(
        &*info.name,
        section,
        info.build_date.as_deref(),
        Some(&vendor),
        None,
    )
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
    }
}
