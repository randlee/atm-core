use syn::Error;
use syn::Ident;
use syn::LitStr;
use syn::Result;
use syn::Token;
use syn::parse::Parse;
use syn::parse::ParseStream;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Boundary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Directive {
    Allow(Vec<String>),
    InternalOnly,
    ForbidExternalImpls,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeInput {
    pub directives: Vec<Directive>,
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

pub fn parse_directive(input: ParseStream<'_>) -> Result<Directive> {
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
                let rule_id = lit.value();
                if rule_id.trim().is_empty() {
                    return Err(Error::new(
                        lit.span(),
                        "boundary.allow rule ids must not be empty",
                    ));
                }
                rule_ids.push(rule_id);
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
            Ok(Directive::Allow(rule_ids))
        }
        (Scope::Boundary, "internal_only") => Ok(Directive::InternalOnly),
        (Scope::Boundary, "forbid_external_impls") => Ok(Directive::ForbidExternalImpls),
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

#[cfg(test)]
mod tests {
    use super::AttributeInput;
    use super::Directive;
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
}
