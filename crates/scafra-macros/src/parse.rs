use quote::{format_ident, quote};
use syn::{Attribute, DataStruct, Error, Path, Result};

pub(crate) fn constructor_parts(
    data: &DataStruct,
) -> Result<(proc_macro2::TokenStream, proc_macro2::TokenStream)> {
    match &data.fields {
        syn::Fields::Named(fields) => {
            let fields = fields.named.iter().map(|field| {
                let ident = field.ident.as_ref().expect("named field has an identifier");
                let ty = &field.ty;
                (ident, format_ident!("__scafra_field_{ident}"), ty)
            });
            let parameters = fields
                .clone()
                .map(|(_, parameter, ty)| quote!(#parameter: #ty));
            let construction = fields
                .map(|(ident, parameter, _)| quote!(#ident: #parameter))
                .collect::<Vec<_>>();
            Ok((
                quote!(#(#parameters),*),
                quote!(Self { #(#construction),* }),
            ))
        }
        syn::Fields::Unnamed(fields) => {
            let fields = fields
                .unnamed
                .iter()
                .enumerate()
                .map(|(index, field)| (format_ident!("field_{index}"), &field.ty));
            let parameters = fields.clone().map(|(ident, ty)| quote!(#ident: #ty));
            let construction = fields.map(|(ident, _)| quote!(#ident)).collect::<Vec<_>>();
            Ok((quote!(#(#parameters),*), quote!(Self(#(#construction),*))))
        }
        syn::Fields::Unit => Ok((quote!(), quote!(Self))),
    }
}

pub(crate) fn default_construction(data: &DataStruct) -> proc_macro2::TokenStream {
    match &data.fields {
        syn::Fields::Named(fields) => {
            let fields = fields.named.iter().map(|field| {
                let ident = field.ident.as_ref().expect("named field has an identifier");
                quote!(#ident: ::std::default::Default::default())
            });
            quote!(Self { #(#fields),* })
        }
        syn::Fields::Unnamed(fields) => {
            let fields = fields
                .unnamed
                .iter()
                .map(|_| quote!(::std::default::Default::default()));
            quote!(Self(#(#fields),*))
        }
        syn::Fields::Unit => quote!(Self),
    }
}

pub(crate) fn reject_generics(generics: &syn::Generics) -> Result<()> {
    if generics.params.is_empty() {
        Ok(())
    } else {
        Err(Error::new_spanned(
            generics,
            "Scafra component macros currently require non-generic structs",
        ))
    }
}

pub(crate) fn has_default_derive(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attribute| {
        if !attribute.path().is_ident("derive") {
            return false;
        }
        let Ok(paths) = attribute
            .parse_args_with(syn::punctuated::Punctuated::<Path, syn::Token![,]>::parse_terminated)
        else {
            return false;
        };
        paths.iter().any(|path| path.is_ident("Default"))
    })
}
