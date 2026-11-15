//! "Did you mean …?" for enum-like arguments.
//!
//! ```
//! use bpaf_kit::suggest;
//!
//! assert_eq!(suggest::did_you_mean("stauts", ["status", "stash", "show"]), Some("status"));
//!
//! #[derive(Clone, Copy, Debug, PartialEq)]
//! enum Format { Json, Yaml }
//! const FORMATS: &[(&str, Format)] = &[("json", Format::Json), ("yaml", Format::Yaml)];
//!
//! assert_eq!(suggest::parse_choice("format", "JSON", FORMATS), Ok(Format::Json));
//! let err = suggest::parse_choice("format", "jsno", FORMATS).unwrap_err();
//! assert_eq!(err.to_string(), "unknown format 'jsno', did you mean 'json'? (valid: json, yaml)");
//! ```
//!
//! With the `strum` feature, [`parse_enum`] does the same for any
//! `#[derive(EnumString, VariantNames)]` enum, and [`bpaf_enum`] plugs it
//! into a bpaf `parse()`.

use core::fmt;

/// An input that matched none of the valid values.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "miette", derive(miette::Diagnostic))]
#[cfg_attr(feature = "miette", diagnostic(code(unknown_value)))]
#[error("unknown {kind} '{input}'{}", Hint(self))]
pub struct UnknownValue {
    /// What was being parsed (`format`, `log level`, …).
    pub kind: String,
    /// What the user typed.
    pub input: String,
    /// The closest valid value, if one is close enough.
    pub suggestion: Option<String>,
    /// Every valid value.
    pub valid: Vec<String>,
}

impl UnknownValue {
    /// Build the error for `input`, picking a suggestion from `valid`.
    pub fn new<'a, I>(kind: impl Into<String>, input: &str, valid: I) -> Self
    where
        I: IntoIterator<Item = &'a str>,
    {
        let valid: Vec<String> = valid.into_iter().map(str::to_owned).collect();
        let suggestion = did_you_mean(input, valid.iter().map(String::as_str)).map(str::to_owned);
        Self {
            kind: kind.into(),
            input: input.to_owned(),
            suggestion,
            valid,
        }
    }

    /// Stable `snake_case` identifier.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        "unknown_value"
    }
}

struct Hint<'a>(&'a UnknownValue);

impl fmt::Display for Hint<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(s) = &self.0.suggestion {
            write!(f, ", did you mean '{s}'?")?;
        }
        if !self.0.valid.is_empty() {
            f.write_str(" (valid: ")?;
            for (i, v) in self.0.valid.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                f.write_str(v)?;
            }
            f.write_str(")")?;
        }
        Ok(())
    }
}

/// The candidate closest to `input` (case-insensitive Damerau-Levenshtein),
/// if it is within a third of the longer string's length (at least 1 edit),
/// or `input` is a prefix of it. Ties go to the earliest candidate.
pub fn did_you_mean<'a, I>(input: &str, candidates: I) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    let input_lower = input.to_lowercase();
    let mut best: Option<(&'a str, usize)> = None;
    for cand in candidates {
        let cand_lower = cand.to_lowercase();
        let dist = if input_lower.len() >= 2 && cand_lower.starts_with(&input_lower) {
            // A prefix is a good guess however much is missing.
            1
        } else {
            strsim::damerau_levenshtein(&input_lower, &cand_lower)
        };
        let limit = (input_lower.chars().count().max(cand_lower.chars().count()) / 3).max(1);
        if dist <= limit && best.is_none_or(|(_, d)| dist < d) {
            best = Some((cand, dist));
        }
    }
    best.map(|(c, _)| c)
}

/// Match `input` case-insensitively against `choices` (name, value) pairs.
///
/// # Errors
///
/// [`UnknownValue`] with a suggestion and the list of names.
pub fn parse_choice<T: Clone>(
    kind: &str,
    input: &str,
    choices: &[(&str, T)],
) -> Result<T, UnknownValue> {
    choices
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(input))
        .map(|(_, v)| v.clone())
        .ok_or_else(|| UnknownValue::new(kind, input, choices.iter().map(|(n, _)| *n)))
}

/// `"output format"` for `my_crate::OutputFormat`: the type's last path
/// segment split into lowercase words.
#[must_use]
pub fn kind_of<T: ?Sized>() -> String {
    let full = core::any::type_name::<T>();
    let name = full.split('<').next().unwrap_or(full);
    let name = name.rsplit("::").next().unwrap_or(name);
    let mut out = String::with_capacity(name.len() + 4);
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push(' ');
        }
        out.extend(c.to_lowercase());
    }
    out
}

/// Parse a strum enum through its `FromStr` (`EnumString`), falling back to
/// a case-insensitive match on its `VariantNames`, and erroring with a
/// suggestion.
///
/// ```
/// # #[cfg(feature = "strum")] {
/// #[derive(Debug, PartialEq, strum::EnumString, strum::VariantNames)]
/// #[strum(serialize_all = "kebab-case")]
/// enum OutputFormat { Json, PrettyTable }
///
/// use bpaf_kit::suggest::parse_enum;
/// assert_eq!(parse_enum::<OutputFormat>("pretty-table"), Ok(OutputFormat::PrettyTable));
/// assert_eq!(
///     parse_enum::<OutputFormat>("pretty-tabel").unwrap_err().to_string(),
///     "unknown output format 'pretty-tabel', did you mean 'pretty-table'? (valid: json, pretty-table)",
/// );
/// # }
/// ```
///
/// # Errors
///
/// [`UnknownValue`] when nothing matches.
#[cfg(feature = "strum")]
pub fn parse_enum<T>(input: &str) -> Result<T, UnknownValue>
where
    T: core::str::FromStr + strum::VariantNames,
{
    if let Ok(v) = input.parse() {
        return Ok(v);
    }
    if let Some(name) = T::VARIANTS.iter().find(|n| n.eq_ignore_ascii_case(input))
        && let Ok(v) = name.parse()
    {
        return Ok(v);
    }
    Err(UnknownValue::new(
        kind_of::<T>(),
        input,
        T::VARIANTS.iter().copied(),
    ))
}

/// [`parse_enum`] with the signature bpaf's `parse()` wants:
///
/// ```ignore
/// #[bpaf(long, argument::<String>("FORMAT"), parse(bpaf_kit::suggest::bpaf_enum))]
/// format: OutputFormat,
/// ```
///
/// # Errors
///
/// As [`parse_enum`].
#[cfg(feature = "strum")]
#[expect(
    clippy::needless_pass_by_value,
    reason = "bpaf's parse() passes the String by value"
)]
pub fn bpaf_enum<T>(input: String) -> Result<T, UnknownValue>
where
    T: core::str::FromStr + strum::VariantNames,
{
    parse_enum(&input)
}

/// `"one of: json, yaml"`, for help text listing a strum enum's values.
#[cfg(feature = "strum")]
#[must_use]
pub fn variants_help<T: strum::VariantNames>() -> String {
    format!("one of: {}", T::VARIANTS.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions() {
        let cmds = ["status", "stash", "show", "switch"];
        assert_eq!(did_you_mean("stauts", cmds), Some("status"));
        assert_eq!(did_you_mean("STASH", cmds), Some("stash"));
        assert_eq!(did_you_mean("swi", cmds), Some("switch"));
        assert_eq!(did_you_mean("shw", cmds), Some("show"));
        assert_eq!(did_you_mean("xyzzy", cmds), None);
        assert_eq!(did_you_mean("a", ["b", "c"]), Some("b"));
        assert_eq!(did_you_mean("ab", ["xy"]), None);
        assert_eq!(did_you_mean("anything", []), None);
    }

    #[test]
    fn unknown_value_display() {
        let e = UnknownValue::new("color", "purpel", ["red", "purple"]);
        assert_eq!(e.suggestion.as_deref(), Some("purple"));
        assert_eq!(
            e.to_string(),
            "unknown color 'purpel', did you mean 'purple'? (valid: red, purple)"
        );
        let e = UnknownValue::new("color", "zzz", ["red"]);
        assert_eq!(e.to_string(), "unknown color 'zzz' (valid: red)");
        assert_eq!(e.code(), "unknown_value");
    }

    #[test]
    fn choices() {
        let c = [("fast", 1), ("slow", 2)];
        assert_eq!(parse_choice("speed", "SLOW", &c), Ok(2));
        assert_eq!(
            parse_choice("speed", "fsat", &c)
                .unwrap_err()
                .suggestion
                .as_deref(),
            Some("fast")
        );
    }

    #[test]
    fn kinds() {
        struct OutputFormat;
        assert_eq!(kind_of::<OutputFormat>(), "output format");
        assert_eq!(kind_of::<u8>(), "u8");
        assert_eq!(kind_of::<Vec<u8>>(), "vec");
    }

    #[cfg(feature = "strum")]
    mod strum_enums {
        use super::super::*;

        #[derive(Debug, PartialEq, strum::EnumString, strum::VariantNames)]
        #[strum(serialize_all = "snake_case")]
        enum LogLevel {
            Warn,
            Info,
            Debug,
        }

        #[test]
        fn parses_and_suggests() {
            assert_eq!(parse_enum::<LogLevel>("info"), Ok(LogLevel::Info));
            assert_eq!(parse_enum::<LogLevel>("DEBUG"), Ok(LogLevel::Debug));
            let e = parse_enum::<LogLevel>("wran").unwrap_err();
            assert_eq!(e.kind, "log level");
            assert_eq!(e.suggestion.as_deref(), Some("warn"));
            assert_eq!(bpaf_enum::<LogLevel>("warn".into()), Ok(LogLevel::Warn));
            assert_eq!(variants_help::<LogLevel>(), "one of: warn, info, debug");
        }

        #[test]
        fn works_as_a_bpaf_parser() {
            use bpaf::Parser;
            let p = bpaf::long("level")
                .argument::<String>("LEVEL")
                .parse(bpaf_enum::<LogLevel>)
                .to_options();
            assert_eq!(
                p.run_inner(&["--level", "info"][..]).unwrap(),
                LogLevel::Info
            );
            let err = p
                .run_inner(&["--level", "inf"][..])
                .unwrap_err()
                .unwrap_stderr();
            assert!(err.contains("did you mean 'info'?"), "{err}");
        }
    }
}
