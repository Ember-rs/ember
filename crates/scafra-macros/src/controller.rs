use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Error, LitStr, Result};

pub(crate) fn expand_controller(
    attr: TokenStream,
    item: TokenStream,
) -> Result<proc_macro2::TokenStream> {
    let prefix = syn::parse::<LitStr>(attr)?;
    if !prefix.value().starts_with('/') {
        return Err(Error::new_spanned(
            prefix,
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
    let default_construction = crate::parse::default_construction(data);
    let default_impl = if crate::parse::has_default_derive(&input.attrs) {
        quote! {}
    } else {
        quote! {
            impl ::std::default::Default for #ident {
                fn default() -> Self {
                    #default_construction
                }
            }
        }
    };

    Ok(quote! {
        #input

        impl ::scafra::web::ControllerPrefix for #ident {
            const PREFIX: &'static str = #prefix;
        }

        impl #ident {
            pub fn new(#parameters) -> Self {
                #construction
            }
        }

        #default_impl

        ::scafra::core::__private::inventory::submit! {
            ::scafra::core::ComponentRegistration {
                name: stringify!(#ident),
                kind: ::scafra::core::ComponentKind::Controller,
            }
        }
    })
}
