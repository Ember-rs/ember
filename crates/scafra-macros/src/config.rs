use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{
    Attribute, Data, DataStruct, DeriveInput, Error, Fields, GenericArgument, LitStr, Path,
    PathArguments, Result, Type,
};

pub(crate) fn expand_config(input: TokenStream) -> Result<proc_macro2::TokenStream> {
    let input = syn::parse::<DeriveInput>(input)?;
    let ident = &input.ident;
    reject_config_generics(&input.generics)?;
    let data = match &input.data {
        Data::Struct(data) => data,
        _ => {
            return Err(Error::new_spanned(
                &input,
                "Config can only be derived for a struct",
            ))
        }
    };
    let prefix =
        parse_config_prefix(&input.attrs)?.unwrap_or_else(|| LitStr::new("", Span::call_site()));
    let validations = config_validations(data)?;

    Ok(quote! {
        impl ::scafra::config::Config for #ident {
            type Error = ::scafra::config::ValidationError;

            fn validate(&self) -> ::std::result::Result<(), Self::Error> {
                #(#validations)*
                ::std::result::Result::Ok(())
            }
        }

        impl #ident {
            pub const CONFIG_PREFIX: &'static str = #prefix;
        }

        impl ::scafra::config::ConfigProperties for #ident {
            const CONFIG_PREFIX: &'static str = #prefix;
        }
    })
}

fn reject_config_generics(generics: &syn::Generics) -> Result<()> {
    if generics.params.is_empty() {
        Ok(())
    } else {
        Err(Error::new_spanned(
            generics,
            "Config derive currently requires a non-generic struct",
        ))
    }
}

fn parse_config_prefix(attrs: &[Attribute]) -> Result<Option<LitStr>> {
    let mut prefix = None;

    for attribute in attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("config"))
    {
        let options = attribute.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        )?;
        if options.is_empty() {
            return Err(Error::new_spanned(
                attribute,
                "the config attribute must specify `prefix = \"...\"`",
            ));
        }

        for option in options {
            let syn::Meta::NameValue(value) = option else {
                return Err(Error::new_spanned(
                    option,
                    "unknown config option; expected `prefix = \"...\"`",
                ));
            };
            if !value.path.is_ident("prefix") {
                return Err(Error::new_spanned(
                    value.path,
                    "unknown config option; expected `prefix = \"...\"`",
                ));
            }
            if prefix.is_some() {
                return Err(Error::new_spanned(
                    value.path,
                    "duplicate config option `prefix`",
                ));
            }
            let syn::Expr::Lit(expression) = value.value else {
                return Err(Error::new_spanned(
                    value.value,
                    "config prefix must be a string literal",
                ));
            };
            let syn::Lit::Str(literal) = expression.lit else {
                return Err(Error::new_spanned(
                    expression.lit,
                    "config prefix must be a string literal",
                ));
            };
            prefix = Some(literal);
        }
    }

    Ok(prefix)
}

#[derive(Clone, Copy)]
enum RequiredFieldKind {
    String,
    Option,
}

fn config_validations(data: &DataStruct) -> Result<Vec<proc_macro2::TokenStream>> {
    match &data.fields {
        Fields::Named(fields) => fields
            .named
            .iter()
            .map(|field| {
                let Some(()) = parse_required_field_option(&field.attrs)? else {
                    return Ok(None);
                };
                let field_name = field
                    .ident
                    .as_ref()
                    .expect("named config field has an identifier");
                let validation = match required_field_kind(&field.ty) {
                    Some(RequiredFieldKind::String) => quote! {
                        if self.#field_name.trim().is_empty() {
                            return ::std::result::Result::Err(::scafra::config::ValidationError {
                                field: stringify!(#field_name),
                                message: "must not be blank".to_owned(),
                            });
                        }
                    },
                    Some(RequiredFieldKind::Option) => quote! {
                        if self.#field_name.is_none() {
                            return ::std::result::Result::Err(::scafra::config::ValidationError {
                                field: stringify!(#field_name),
                                message: "must be present".to_owned(),
                            });
                        }
                    },
                    None => {
                        return Err(Error::new_spanned(
                            &field.ty,
                            "config(required) supports only String, std::string::String, Option<T>, or std::option::Option<T>",
                        ))
                    }
                };
                Ok(Some(validation))
            })
            .collect::<Result<Vec<_>>>()
            .map(|validations| validations.into_iter().flatten().collect()),
        Fields::Unnamed(fields) => {
            for field in &fields.unnamed {
                if parse_required_field_option(&field.attrs)?.is_some() {
                    return Err(Error::new_spanned(
                        field,
                        "config(required) requires a named field",
                    ));
                }
            }
            Ok(Vec::new())
        }
        Fields::Unit => Ok(Vec::new()),
    }
}

fn parse_required_field_option(attrs: &[Attribute]) -> Result<Option<()>> {
    let mut required = false;

    for attribute in attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("config"))
    {
        let options = attribute.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        )?;
        if options.is_empty() {
            return Err(Error::new_spanned(
                attribute,
                "the field config attribute must specify `required`",
            ));
        }

        for option in options {
            match option {
                syn::Meta::Path(path) if path.is_ident("required") => {
                    if required {
                        return Err(Error::new_spanned(
                            path,
                            "duplicate field config option `required`",
                        ));
                    }
                    required = true;
                }
                syn::Meta::NameValue(value) if value.path.is_ident("required") => {
                    return Err(Error::new_spanned(
                        value,
                        "config(required) does not take a value; use `#[config(required)]`",
                    ));
                }
                other => {
                    return Err(Error::new_spanned(
                        other,
                        "unknown field config option; expected `required`",
                    ));
                }
            }
        }
    }

    Ok(required.then_some(()))
}

fn required_field_kind(ty: &Type) -> Option<RequiredFieldKind> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    if type_path.qself.is_some() {
        return None;
    }

    if path_has_segments(&type_path.path, &["String"])
        || path_has_segments(&type_path.path, &["std", "string", "String"])
    {
        return Some(RequiredFieldKind::String);
    }

    let segments = type_path.path.segments.iter().collect::<Vec<_>>();
    let is_option_path = path_has_names(&type_path.path, &["Option"])
        || path_has_names(&type_path.path, &["std", "option", "Option"]);
    if !is_option_path {
        return None;
    }
    let option = segments.last()?;
    let PathArguments::AngleBracketed(arguments) = &option.arguments else {
        return None;
    };
    (arguments.args.len() == 1 && matches!(arguments.args.first(), Some(GenericArgument::Type(_))))
        .then_some(RequiredFieldKind::Option)
}

fn path_has_segments(path: &Path, expected: &[&str]) -> bool {
    path.segments.len() == expected.len()
        && path
            .segments
            .iter()
            .zip(expected)
            .all(|(segment, expected)| segment.ident == *expected && segment.arguments.is_empty())
}

fn path_has_names(path: &Path, expected: &[&str]) -> bool {
    path.segments.len() == expected.len()
        && path
            .segments
            .iter()
            .zip(expected)
            .enumerate()
            .all(|(index, (segment, expected))| {
                segment.ident == *expected
                    && (index + 1 == path.segments.len() || segment.arguments.is_empty())
            })
}
