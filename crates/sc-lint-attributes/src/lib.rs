use proc_macro::TokenStream;
use proc_macro2::Span;
use proc_macro2::TokenStream as TokenStream2;
use syn::Error;
use syn::Ident;
use syn::LitStr;
use syn::Result;
use syn::Token;
use syn::parse::Parse;
use syn::parse::ParseStream;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Scope {
    Boundary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Directive {
    BoundaryAllow(Vec<String>),
    BoundaryInternalOnly,
    BoundaryForbidExternalImpls,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AttributeInput {
    directives: Vec<Directive>,
}

impl Parse for AttributeInput {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut directives = Vec::new();
        while !input.is_empty() {
            directives.push(parse_directive(input)?);
            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }
        Ok(Self { directives })
    }
}

fn parse_directive(input: ParseStream<'_>) -> Result<Directive> {
    let scope = parse_scope(input)?;
    input.parse::<Token![.]>()?;
    let action = input.parse::<Ident>()?;
    let action_name = action.to_string();

    match (scope, action_name.as_str()) {
        (Scope::Boundary, "allow") => {
            let content;
            syn::parenthesized!(content in input);
            let mut rule_ids = Vec::new();
            while !content.is_empty() {
                let lit = content.parse::<LitStr>()?;
                rule_ids.push(lit.value());
                if content.is_empty() {
                    break;
                }
                content.parse::<Token![,]>()?;
            }
            if rule_ids.is_empty() {
                return Err(Error::new(
                    action.span(),
                    "boundary.allow requires at least one rule id string",
                ));
            }
            Ok(Directive::BoundaryAllow(rule_ids))
        }
        (Scope::Boundary, "internal_only") => Ok(Directive::BoundaryInternalOnly),
        (Scope::Boundary, "forbid_external_impls") => Ok(Directive::BoundaryForbidExternalImpls),
        (Scope::Boundary, _) => Err(Error::new(
            action.span(),
            format!(
                "unsupported boundary directive `{action_name}`; supported: allow(...), internal_only, forbid_external_impls"
            ),
        )),
    }
}

fn parse_scope(input: ParseStream<'_>) -> Result<Scope> {
    let ident = input.parse::<Ident>()?;
    match ident.to_string().as_str() {
        "boundary" => Ok(Scope::Boundary),
        other => Err(Error::new(
            ident.span(),
            format!("unsupported sc_lint scope `{other}`; supported: boundary"),
        )),
    }
}

fn validate_attribute(input: &AttributeInput) -> Result<()> {
    for directive in &input.directives {
        match directive {
            Directive::BoundaryAllow(rule_ids) => {
                for rule_id in rule_ids {
                    if rule_id.trim().is_empty() {
                        return Err(Error::new(
                            Span::call_site(),
                            "boundary.allow rule ids must not be empty",
                        ));
                    }
                }
            }
            Directive::BoundaryInternalOnly | Directive::BoundaryForbidExternalImpls => {}
        }
    }
    Ok(())
}

fn expand_sc_lint(args: TokenStream2, item: TokenStream2) -> Result<TokenStream2> {
    let parsed = syn::parse2::<AttributeInput>(args)?;
    validate_attribute(&parsed)?;
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
            vec![Directive::BoundaryAllow(vec![
                "cycle.type_method_self_loop".to_string()
            ])]
        );
    }

    #[test]
    fn parses_boundary_internal_only() {
        let parsed: AttributeInput = syn::parse2(quote!(boundary.internal_only)).unwrap();
        assert_eq!(parsed.directives, vec![Directive::BoundaryInternalOnly]);
    }

    #[test]
    fn parses_boundary_forbid_external_impls() {
        let parsed: AttributeInput = syn::parse2(quote!(boundary.forbid_external_impls)).unwrap();
        assert_eq!(
            parsed.directives,
            vec![Directive::BoundaryForbidExternalImpls]
        );
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
                Directive::BoundaryInternalOnly,
                Directive::BoundaryForbidExternalImpls,
                Directive::BoundaryAllow(vec!["cycle.type_method_self_loop".to_string()]),
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
