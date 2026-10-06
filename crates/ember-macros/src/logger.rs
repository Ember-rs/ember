use proc_macro2::TokenStream;
use quote::quote;
use syn::{Error, ImplItem, Item, ItemFn, ItemImpl, Result};

/// Adds a tracing span to a function or every method in an impl block.
pub(crate) fn expand_logger(item: TokenStream) -> Result<TokenStream> {
    let parsed = syn::parse2::<Item>(item)?;
    match parsed {
        Item::Fn(function) => expand_function(function, None),
        Item::Impl(implementation) => expand_impl(implementation),
        Item::Struct(item) => Ok(quote!(#item)),
        other => Err(Error::new_spanned(
            other,
            "logger can only be applied to a function, struct, or impl block",
        )),
    }
}

fn expand_function(function: ItemFn, target: Option<String>) -> Result<TokenStream> {
    let target_attribute = target.map(|target| {
        let target = syn::LitStr::new(&target, proc_macro2::Span::call_site());
        quote!(skip_all, target = #target)
    });
    let target_attribute = target_attribute.unwrap_or_else(|| quote!(skip_all));
    let attrs = function.attrs;
    let vis = function.vis;
    let sig = function.sig;
    let block = function.block;
    Ok(quote! {
        #[::ember::__private::tracing::instrument(#target_attribute)]
        #(#attrs)*
        #vis #sig #block
    })
}

fn expand_impl(mut implementation: ItemImpl) -> Result<TokenStream> {
    let target = match implementation.self_ty.as_ref() {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        _ => None,
    };

    for item in &mut implementation.items {
        if let ImplItem::Fn(method) = item {
            let target_attribute = target.as_ref().map(|target| {
                let target = syn::LitStr::new(target, proc_macro2::Span::call_site());
                quote!(skip_all, target = #target)
            });
            let target_attribute = target_attribute.unwrap_or_else(|| quote!(skip_all));
            let instrument: syn::Attribute = syn::parse_quote! {
                #[::ember::__private::tracing::instrument(#target_attribute)]
            };
            method.attrs.insert(0, instrument);
        }
    }

    Ok(quote!(#implementation))
}
