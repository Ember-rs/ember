use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    parenthesized,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    Data, DeriveInput, Error, Ident, LitStr, Result, Token,
};

pub(crate) fn expand_controller(attr: TokenStream, item: TokenStream) -> Result<TokenStream2> {
    let arguments = syn::parse::<ControllerArgs>(attr)?;
    if !arguments.prefix.value().starts_with('/') {
        return Err(Error::new_spanned(
            arguments.prefix,
            "controller paths must start with '/', for example '/api'",
        ));
    }

    let input = syn::parse::<DeriveInput>(item)?;
    let ident = &input.ident;
    crate::parse::reject_generics(&input.generics)?;
    let data = match &input.data {
        Data::Struct(data) => data,
        _ => {
            return Err(Error::new_spanned(
                &input,
                "controller can only be applied to a struct",
            ))
        }
    };
    let (parameters, construction) = crate::parse::constructor_parts(data)?;
    let default_impl = if matches!(data.fields, syn::Fields::Unit)
        && !crate::parse::has_default_derive(&input.attrs)
    {
        quote! {
            impl ::std::default::Default for #ident {
                fn default() -> Self { Self }
            }
        }
    } else {
        quote! {}
    };
    let policy = arguments.policy.to_tokens();
    let prefix = &arguments.prefix;
    Ok(quote! {
        #input

        impl ::scafra::web::ControllerPrefix for #ident {
            const PREFIX: &'static str = #prefix;
            const AUTHORIZATION_POLICY: ::scafra::web::AuthorizationPolicy = #policy;
        }

        impl #ident {
            pub fn new(#parameters) -> Self {
                #construction
            }
        }

        // Keep the legacy route builder available for controllers whose
        // fields can still be constructed with `Default`. Dependency-bearing
        // controllers remain constructible through the typed graph alone.
        #default_impl

        ::scafra::core::__private::inventory::submit! {
            ::scafra::core::ComponentRegistration {
                name: stringify!(#ident),
                kind: ::scafra::core::ComponentKind::Controller,
            }
        }
    })
}

struct ControllerArgs {
    prefix: LitStr,
    policy: PolicyArgs,
}

impl Parse for ControllerArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let prefix = input.parse()?;
        let mut policy = PolicyArgs::default();
        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            policy.parse_entry(input)?;
        }
        policy.finish()?;
        Ok(Self { prefix, policy })
    }
}

#[derive(Default)]
struct PolicyArgs {
    explicit_mode: Option<PolicyMode>,
    role_groups: Vec<Vec<LitStr>>,
    required_scopes: Vec<LitStr>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PolicyMode {
    Inherit,
    Public,
    Protected,
}

impl PolicyArgs {
    fn parse_entry(&mut self, input: ParseStream<'_>) -> Result<()> {
        let entry: Ident = input.parse()?;
        match entry.to_string().as_str() {
            "public" => self.set_mode(PolicyMode::Public, &entry),
            "authenticated" => self.set_mode(PolicyMode::Protected, &entry),
            "roles" => {
                let content;
                parenthesized!(content in input);
                let roles = parse_names(&content, "role")?;
                self.role_groups.push(roles);
                Ok(())
            }
            "scopes" => {
                let content;
                parenthesized!(content in input);
                self.required_scopes.extend(parse_names(&content, "scope")?);
                Ok(())
            }
            _ => Err(Error::new_spanned(
                entry,
                "controller policy accepts `public`, `authenticated`, `roles(...)`, and `scopes(...)`",
            )),
        }
    }

    fn set_mode(&mut self, mode: PolicyMode, entry: &Ident) -> Result<()> {
        if self.explicit_mode.replace(mode).is_some() {
            return Err(Error::new_spanned(
                entry,
                "controller policy may declare `public` or `authenticated` only once",
            ));
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        let has_requirements = !self.role_groups.is_empty() || !self.required_scopes.is_empty();
        if self.explicit_mode == Some(PolicyMode::Public) && has_requirements {
            return Err(Error::new(
                proc_macro2::Span::call_site(),
                "a public controller cannot also require roles or scopes",
            ));
        }
        if has_requirements && self.explicit_mode.is_none() {
            self.explicit_mode = Some(PolicyMode::Protected);
        }
        Ok(())
    }

    fn to_tokens(&self) -> TokenStream2 {
        let mode = match self.explicit_mode.unwrap_or(PolicyMode::Inherit) {
            PolicyMode::Inherit => quote!(::scafra::web::AuthorizationMode::Inherit),
            PolicyMode::Public => quote!(::scafra::web::AuthorizationMode::Public),
            PolicyMode::Protected => quote!(::scafra::web::AuthorizationMode::Protected),
        };
        let role_groups = self.role_groups.iter().map(|group| quote!(&[#(#group),*]));
        let scopes = &self.required_scopes;
        quote! {
            ::scafra::web::AuthorizationPolicy {
                mode: #mode,
                role_groups: &[#(#role_groups),*],
                required_scopes: &[#(#scopes),*],
            }
        }
    }
}

fn parse_names(input: ParseStream<'_>, kind: &str) -> Result<Vec<LitStr>> {
    let values = Punctuated::<LitStr, Token![,]>::parse_terminated(input)?;
    if values.is_empty() {
        return Err(Error::new(
            input.span(),
            format!("at least one {kind} is required"),
        ));
    }
    for value in &values {
        let name = value.value();
        if name.trim().is_empty() || name.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return Err(Error::new_spanned(
                value,
                format!("{kind} names must be non-empty and contain no whitespace"),
            ));
        }
    }
    Ok(values.into_iter().collect())
}
