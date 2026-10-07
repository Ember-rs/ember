use proc_macro::TokenStream;

pub(crate) fn into_token_stream(result: syn::Result<proc_macro2::TokenStream>) -> TokenStream {
    result.unwrap_or_else(syn::Error::into_compile_error).into()
}
