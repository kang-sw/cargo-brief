use proc_macro::TokenStream;

/// A bang proc-macro: invoked as `my_bang!(...)`.
#[proc_macro]
pub fn my_bang(input: TokenStream) -> TokenStream {
    input
}

/// An attribute proc-macro: invoked as `#[my_attr]`.
#[proc_macro_attribute]
pub fn my_attr(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// A derive proc-macro: invoked as `#[derive(MyDerive)]`.
/// Supports the `my_helper` helper attribute.
#[proc_macro_derive(MyDerive, attributes(my_helper))]
pub fn my_derive(input: TokenStream) -> TokenStream {
    input
}
