#![doc = include_str!("../README.md")]

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::{
    Attribute, Data, DeriveInput, Expr, ExprLit, Ident, Lit, LitStr, Meta, Token, parse_macro_input,
};

/// Adds `parser()` and `cli()` to a `#[bpaf(options)]` type, plus a test
/// that runs bpaf's `check_invariants` on the parser.
///
/// ```ignore
/// #[derive(Debug, Clone, bpaf::Bpaf, bpaf_kit::Cli)]
/// #[bpaf(options)]
/// #[cli(log = "info")]
/// struct Opts {
///     #[bpaf(external(bpaf_kit::log::log_args))]
///     #[cli(log_args)]
///     log: bpaf_kit::log::LogArgs,
/// }
///
/// fn main() {
///     let opts = Opts::cli();
/// }
/// ```
///
/// Generated:
///
/// - `fn parser() -> bpaf::OptionParser<Self>`: the bpaf-generated parser
///   (the type name in `snake_case`, or `#[cli(parser = name)]`) with the
///   compiled-in version attached.
/// - `fn cli() -> Self`: `bpaf_kit::App::run` on it: Ctrl-C handler,
///   `--man`, `--version [--verbose]`, parse, logging init.
/// - `#[cfg(test)] mod __<type>_cli`: a `parser_invariants` test.
///
/// A cargo subcommand, `#[bpaf(options("name"))]`, also answers
/// `cargo-name name --man` and `cargo-name name --version`, the way
/// `cargo name --man` runs it (`App::cargo_subcommand`).
///
/// Container attributes, `#[cli(...)]`:
///
/// - `log = "<filter>"`: default log filter (`"warn"` if absent);
///   `log = false` leaves logging to the program.
/// - `parser = <ident>`: the parser function, when bpaf's
///   `generate(...)` renamed it.
///
/// Field attribute `#[cli(log_args)]`: the `bpaf_kit::log::LogArgs` field
/// whose `-v`/`-q`/`--log-json` adjust logging (needs the `log` feature).
#[proc_macro_derive(Cli, attributes(cli))]
pub fn derive_cli(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

enum Log {
    Default,
    Filter(LitStr),
    Off,
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let ty = &input.ident;
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "#[derive(Cli)] does not support generic types",
        ));
    }

    let mut log = Log::Default;
    let mut parser_fn = format_ident!("{}", snake_case(&ty.to_string()), span = ty.span());
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("cli")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("log") {
                log = match meta.value()?.parse::<Expr>()? {
                    Expr::Lit(ExprLit {
                        lit: Lit::Str(s), ..
                    }) => Log::Filter(s),
                    Expr::Lit(ExprLit {
                        lit: Lit::Bool(b), ..
                    }) if !b.value => Log::Off,
                    other => {
                        return Err(syn::Error::new_spanned(
                            other,
                            "expected `log = \"<filter>\"` or `log = false`",
                        ));
                    }
                };
                Ok(())
            } else if meta.path.is_ident("parser") {
                parser_fn = meta.value()?.parse::<Ident>()?;
                Ok(())
            } else {
                Err(meta.error("unknown `cli` key; expected `log` or `parser`"))
            }
        })?;
    }

    let cargo = cargo_subcommand(&input.attrs).map(|name| quote!(.cargo_subcommand(#name)));
    let log_args = find_log_args(input)?;
    let log_setting = match &log {
        Log::Default => quote!(),
        Log::Filter(s) => quote!(.log(::core::option::Option::Some(#s))),
        Log::Off => quote!(.log(::core::option::Option::None)),
    };
    let run = if let Some(field) = log_args {
        quote!(.run_with_log_args(|opts: &Self| &opts.#field))
    } else {
        quote!(.run())
    };
    let test_mod = format_ident!("__{}_cli", snake_case(&ty.to_string()));

    Ok(quote! {
        impl #ty {
            /// The command-line parser, with the version attached.
            #[must_use]
            pub fn parser() -> ::bpaf_kit::bpaf::OptionParser<Self> {
                Self::__bpaf_kit_app().into_parser()
            }

            fn __bpaf_kit_app() -> ::bpaf_kit::App<Self> {
                ::bpaf_kit::App::new(#parser_fn(), ::bpaf_kit::build_info!()) #cargo #log_setting
            }

            /// Install the Ctrl-C handler, answer `--man` and `--version`,
            /// parse the command line and set up logging.
            #[must_use]
            pub fn cli() -> Self {
                Self::__bpaf_kit_app() #run
            }
        }

        #[cfg(test)]
        mod #test_mod {
            #[test]
            fn parser_invariants() {
                super::#ty::parser().check_invariants(false);
            }
        }
    })
}

fn find_log_args(input: &DeriveInput) -> syn::Result<Option<TokenStream2>> {
    let Data::Struct(data) = &input.data else {
        return Ok(None);
    };
    let mut found = None;
    for (i, field) in data.fields.iter().enumerate() {
        for attr in field.attrs.iter().filter(|a| a.path().is_ident("cli")) {
            attr.parse_nested_meta(|meta| {
                if !meta.path.is_ident("log_args") {
                    return Err(meta.error("unknown `cli` field key; expected `log_args`"));
                }
                if found.is_some() {
                    return Err(meta.error("only one field can be `#[cli(log_args)]`"));
                }
                found = Some(if let Some(ident) = &field.ident {
                    quote!(#ident)
                } else {
                    let index = syn::Index {
                        index: u32::try_from(i).unwrap_or(u32::MAX),
                        span: Span::call_site(),
                    };
                    quote!(#index)
                });
                Ok(())
            })?;
        }
    }
    Ok(found)
}

/// The `name` of `#[bpaf(options("name"))]`: bpaf then wraps the parser in
/// `cargo_helper("name", …)`. Attributes this can't read are left to bpaf.
fn cargo_subcommand(attrs: &[Attribute]) -> Option<LitStr> {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("bpaf"))
        .filter_map(|a| {
            a.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                .ok()
        })
        .flatten()
        .find_map(|meta| match meta {
            Meta::List(list) if list.path.is_ident("options") => list.parse_args::<LitStr>().ok(),
            _ => None,
        })
}

/// The parser function name bpaf's derive generates: every ASCII uppercase
/// letter starts a new `_`-separated word (`HTTPOpts` → `h_t_t_p_opts`).
fn snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() * 2);
    for c in name.strip_prefix("r#").unwrap_or(name).chars() {
        if c.is_ascii_uppercase() {
            if !out.is_empty() {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else if c == '-' {
            out.push('_');
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake() {
        assert_eq!(snake_case("Opts"), "opts");
        assert_eq!(snake_case("MyOpts"), "my_opts");
        assert_eq!(snake_case("HTTPOpts"), "h_t_t_p_opts");
        assert_eq!(snake_case("OptsV2"), "opts_v2");
    }

    #[test]
    fn expands() {
        let input: DeriveInput = syn::parse_quote! {
            #[bpaf(options)]
            #[cli(log = "info")]
            struct MyOpts {
                #[cli(log_args)]
                log: LogArgs,
            }
        };
        let out = expand(&input).unwrap().to_string();
        assert!(out.contains("my_opts ()"), "{out}");
        assert!(out.contains("run_with_log_args"), "{out}");
        assert!(out.contains("\"info\""), "{out}");
        assert!(out.contains("mod __my_opts_cli"), "{out}");
    }

    #[test]
    fn cargo_subcommand_from_bpaf_options() {
        let input: DeriveInput = syn::parse_quote! {
            #[bpaf(options("faster"), header("x"), generate(faster_opts))]
            #[cli(parser = faster_opts)]
            struct Opts;
        };
        let out = expand(&input).unwrap().to_string();
        assert!(out.contains(". cargo_subcommand (\"faster\")"), "{out}");

        let input: DeriveInput = syn::parse_quote! {
            #[bpaf(options, version)]
            struct Opts;
        };
        let out = expand(&input).unwrap().to_string();
        assert!(!out.contains("cargo_subcommand"), "{out}");
    }

    #[test]
    fn rejects_unknown_keys() {
        let input: DeriveInput = syn::parse_quote! {
            #[cli(lgo = "info")]
            struct Opts;
        };
        assert!(expand(&input).is_err());
        let input: DeriveInput = syn::parse_quote! {
            #[cli(log = 3)]
            struct Opts;
        };
        assert!(expand(&input).is_err());
    }
}
