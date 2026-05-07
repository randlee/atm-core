use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use sc_lint_directives::AttributeInput;
use syn::Result;

#[cfg(test)]
use sc_lint_directives::Directive;

fn expand_sc_lint(args: TokenStream2, item: TokenStream2) -> Result<TokenStream2> {
    let _parsed = syn::parse2::<AttributeInput>(args)?;
    Ok(item)
}

#[proc_macro_attribute]
pub fn sc_lint(args: TokenStream, item: TokenStream) -> TokenStream {
    let item_ts: TokenStream2 = item.clone().into();
    let args_ts: TokenStream2 = args.into();
    match expand_sc_lint(args_ts, item_ts) {
        Ok(expanded) => TokenStream::from(expanded),
        Err(error) => TokenStream::from(error.to_compile_error()),
    }
}

#[cfg(test)]
mod tests {
    use super::AttributeInput;
    use super::Directive;
    use super::expand_sc_lint;
    use quote::quote;

    #[test]
    fn parses_boundary_allow_rule() {
        let parsed: AttributeInput =
            syn::parse2(quote!(boundary.allow("cycle.type_method_self_loop"))).unwrap();
        assert_eq!(
            parsed.directives,
            vec![Directive::Allow(vec![
                "cycle.type_method_self_loop".to_string()
            ])]
        );
    }

    #[test]
    fn parses_boundary_internal_only() {
        let parsed: AttributeInput = syn::parse2(quote!(boundary.internal_only)).unwrap();
        assert_eq!(parsed.directives, vec![Directive::InternalOnly]);
    }

    #[test]
    fn parses_boundary_forbid_external_impls() {
        let parsed: AttributeInput = syn::parse2(quote!(boundary.forbid_external_impls)).unwrap();
        assert_eq!(parsed.directives, vec![Directive::ForbidExternalImpls]);
    }

    #[test]
    fn parses_multiple_directives() {
        let parsed: AttributeInput = syn::parse2(quote!(
            boundary.internal_only,
            boundary.forbid_external_impls,
            boundary.allow("cycle.type_method_self_loop")
        ))
        .unwrap();
        assert_eq!(
            parsed.directives,
            vec![
                Directive::InternalOnly,
                Directive::ForbidExternalImpls,
                Directive::Allow(vec!["cycle.type_method_self_loop".to_string()]),
            ]
        );
    }

    #[test]
    fn rejects_unknown_boundary_directive() {
        let error = syn::parse2::<AttributeInput>(quote!(boundary.unknown)).unwrap_err();
        assert!(error.to_string().contains("unsupported boundary directive"));
    }

    #[test]
    fn expansion_is_noop_for_supported_directives() {
        let expanded = expand_sc_lint(
            quote!(
                boundary.internal_only,
                boundary.forbid_external_impls,
                boundary.allow("cycle.type_method_self_loop")
            ),
            quote!(
                pub struct Example;
            ),
        )
        .unwrap();
        assert_eq!(
            expanded.to_string(),
            quote!(
                pub struct Example;
            )
            .to_string()
        );
    }
}
