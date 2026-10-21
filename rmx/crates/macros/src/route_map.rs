//! `#[derive(RouteMap)]`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Type};

/// A field is a route when its type is `Route` (any path ending in `Route`); every other
/// field is a nested route map.
fn is_route(ty: &Type) -> bool {
    match ty {
        Type::Path(p) => p.qself.is_none() && p.path.segments.last().is_some_and(|s| s.ident == "Route" && s.arguments.is_none()),
        _ => false,
    }
}

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(&input.generics, "RouteMap cannot be derived for generic structs"));
    }
    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => &f.named,
            _ => return Err(syn::Error::new_spanned(name, "RouteMap needs a struct with named fields")),
        },
        _ => return Err(syn::Error::new_spanned(name, "RouteMap can only be derived for structs")),
    };
    let vis = &input.vis;
    let actions = format_ident!("{}Actions", name);
    let rm = quote!(::rmx::router::routes);

    let keys = fields.iter().map(|f| {
        let ident = f.ident.as_ref().expect("named field");
        let key = ident.to_string();
        let key = key.strip_prefix("r#").unwrap_or(&key).to_string();
        quote! { (#key, #rm::RouteMap::route_tree(&self.#ident)) }
    });
    let leaves: Vec<_> = fields.iter().filter(|f| is_route(&f.ty)).collect();
    let action_fields = leaves.iter().map(|f| {
        let ident = &f.ident;
        let fvis = &f.vis;
        let doc = format!("The action for `{}.{}`.", name, ident.as_ref().expect("named field"));
        quote! { #[doc = #doc] #fvis #ident: ::rmx::router::Action }
    });
    let pairs = leaves.iter().map(|f| {
        let ident = &f.ident;
        quote! { (::core::clone::Clone::clone(&self.#ident), actions.#ident) }
    });
    let doc = format!("The actions a controller for [`{name}`] supplies: one per direct route. Nested route maps are mapped separately.");

    Ok(quote! {
        #[doc = #doc]
        #vis struct #actions {
            #(#action_fields,)*
        }

        impl #rm::RouteMap for #name {
            type Actions = #actions;

            fn route_tree(&self) -> #rm::RouteTree {
                #rm::RouteTree::Group(::std::vec![#(#keys),*])
            }

            #[allow(unused_variables)]
            fn pair_actions(&self, actions: #actions) -> ::std::vec::Vec<(#rm::Route, ::rmx::router::Action)> {
                ::std::vec![#(#pairs),*]
            }
        }
    })
}
