use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{Attribute, Error, FnArg, ItemImpl, LitStr, Result, Type};

pub(crate) fn expand_routes(item: TokenStream) -> Result<proc_macro2::TokenStream> {
    let mut input = syn::parse::<ItemImpl>(item)?;
    if input.trait_.is_some() {
        return Err(Error::new_spanned(
            &input.self_ty,
            "routes must be applied to an inherent impl block",
        ));
    }
    if !input.generics.params.is_empty() {
        return Err(Error::new_spanned(
            &input.generics,
            "routes currently requires a non-generic impl block",
        ));
    }

    let controller = match input.self_ty.as_ref() {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.clone())
            .ok_or_else(|| {
                Error::new_spanned(&input.self_ty, "could not determine controller type")
            })?,
        _ => {
            return Err(Error::new_spanned(
                &input.self_ty,
                "routes requires a named controller type",
            ))
        }
    };

    let mut handlers = Vec::new();
    let mut registrations = Vec::new();
    let mut metadata = Vec::new();
    let mut route_index = 0usize;

    for impl_item in &mut input.items {
        let syn::ImplItem::Fn(method) = impl_item else {
            continue;
        };
        let route_attributes = method
            .attrs
            .iter()
            .filter_map(parse_route_attribute)
            .collect::<Result<Vec<_>>>()?;
        method
            .attrs
            .retain(|attribute| parse_route_attribute(attribute).is_none());

        if route_attributes.is_empty() {
            continue;
        }
        validate_handler_signature(method)?;

        for (http_method, path) in route_attributes {
            let handler = format_ident!("__scafra_route_handler_{route_index}");
            route_index += 1;
            let method_ident = &method.sig.ident;
            let arguments = method
                .sig
                .inputs
                .iter()
                .skip(1)
                .map(|argument| match argument {
                    FnArg::Typed(argument) => match argument.pat.as_ref() {
                        syn::Pat::Ident(pattern) => {
                            Ok((pattern.ident.clone(), argument.ty.clone()))
                        }
                        _ => Err(Error::new_spanned(
                            &argument.pat,
                            "route extractor arguments must use a simple identifier pattern",
                        )),
                    },
                    FnArg::Receiver(_) => Err(Error::new_spanned(
                        argument,
                        "only '&self' may be used as the controller receiver",
                    )),
                })
                .collect::<Result<Vec<_>>>()?;
            let argument_names = arguments.iter().map(|(ident, _)| ident);
            let argument_definitions = arguments.iter().map(|(ident, ty)| quote!(#ident: #ty));
            let routing_function = format_ident!("{}", http_method);
            let path_literal = LitStr::new(&path, Span::call_site());
            let http_method_literal = LitStr::new(&http_method.to_uppercase(), Span::call_site());
            let output = &method.sig.output;

            handlers.push(quote! {
                async fn #handler(
                    extension: ::scafra::web::axum::extract::Extension<
                        ::std::sync::Arc<Self>
                    >,
                    #(#argument_definitions),*
                ) #output {
                    extension.0.#method_ident(#(#argument_names),*).await
                }
            });
            registrations.push(quote! {
                let path = ::scafra::web::join_paths(
                    <Self as ::scafra::web::ControllerPrefix>::PREFIX,
                    #path_literal,
                );
                router = router.merge(
                    ::scafra::web::axum::Router::new()
                        .route(
                            &path,
                            ::scafra::web::axum::routing::#routing_function(Self::#handler)
                                .layer(::scafra::web::axum::Extension(controller.clone())),
                        ),
                );
            });
            metadata.push(quote! {
                ::scafra::web::RouteMetadata {
                    controller: stringify!(#controller),
                    method: #http_method_literal,
                    prefix: <#controller as ::scafra::web::ControllerPrefix>::PREFIX,
                    path: #path_literal,
                }
            });
        }
    }

    let metadata_ident = format_ident!("__SCAFRA_{}_ROUTES", controller);
    Ok(quote! {
        #input

        impl #controller {
            #(#handlers)*
        }

        impl ::scafra::web::ControllerRoutes for #controller {
            fn register_routes(
                mut router: ::scafra::web::axum::Router,
            ) -> ::scafra::web::axum::Router {
                let controller = ::std::sync::Arc::new(Self::default());
                #(#registrations)*
                router
            }
        }

        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        const #metadata_ident: &[::scafra::web::RouteMetadata] = &[
            #(#metadata),*
        ];

        ::scafra::web::__private::inventory::submit! {
            ::scafra::web::ControllerRegistration {
                controller: stringify!(#controller),
                register: <#controller as ::scafra::web::ControllerRoutes>::register_routes,
                routes: #metadata_ident,
            }
        }
    })
}

fn parse_route_attribute(attribute: &Attribute) -> Option<Result<(String, String)>> {
    let method = if attribute.path().is_ident("get") {
        "get"
    } else if attribute.path().is_ident("post") {
        "post"
    } else if attribute.path().is_ident("put") {
        "put"
    } else if attribute.path().is_ident("delete") {
        "delete"
    } else {
        return None;
    };
    Some(
        attribute
            .parse_args::<LitStr>()
            .map(|path| (method.to_owned(), path.value())),
    )
}

fn validate_handler_signature(method: &syn::ImplItemFn) -> Result<()> {
    if method.sig.asyncness.is_none() {
        return Err(Error::new_spanned(
            method.sig.fn_token,
            "route handlers must be async functions",
        ));
    }
    match method.sig.inputs.first() {
        Some(FnArg::Receiver(receiver))
            if receiver.reference.is_some() && receiver.mutability.is_none() => {}
        Some(argument) => {
            return Err(Error::new_spanned(
                argument,
                "route handlers must use '&self' as their first argument",
            ))
        }
        None => {
            return Err(Error::new_spanned(
                &method.sig.ident,
                "route handlers must have a '&self' receiver",
            ))
        }
    }
    Ok(())
}
