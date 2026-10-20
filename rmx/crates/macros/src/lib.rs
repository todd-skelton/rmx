//! `rsx!`: JSX-like templates that compile to `rmx::Node` trees.
//!
//! - Lowercase tags are elements: `<div class="x">"text" {expr}</div>`.
//! - Capitalized tags are components: `<Counter initial=5/>` builds `Counter`'s `Props`
//! - `on:click={handler}` attaches an event handler, `key={id}` sets a list key.
//! - `mix={css(..)}` or `mix={[a, b]}` applies mixins to the host element.
//! - Component props left out of the template use `Default` values.
//! - Text must be quoted (`"Hello"`); `{expr}` accepts anything implementing `IntoNode`.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as Tokens};
use quote::{quote, quote_spanned, ToTokens};
use rstml::node::{KeyedAttribute, Node, NodeAttribute, NodeElement, NodeName};
use syn::spanned::Spanned;
use syn::{Expr, ExprLit, Lit};

#[proc_macro]
pub fn rsx(input: TokenStream) -> TokenStream {
    match rstml::parse(input) {
        Ok(nodes) => {
            let mut nodes: Vec<Tokens> = nodes.iter().filter_map(gen_node).collect();
            let out = if nodes.len() == 1 {
                nodes.remove(0)
            } else {
                quote! { ::rmx::Node::Fragment(::std::vec![#(#nodes),*]) }
            };
            out.into()
        }
        Err(e) => e.to_compile_error().into(),
    }
}

fn gen_node(node: &Node) -> Option<Tokens> {
    Some(match node {
        Node::Element(el) => gen_element(el),
        Node::Text(text) => {
            let s = text.value_string();
            quote! { ::rmx::Node::Text(::std::borrow::Cow::Borrowed(#s)) }
        }
        Node::RawText(raw) => {
            let s = raw.to_string_best();
            quote! { ::rmx::Node::Text(::std::borrow::Cow::Borrowed(#s)) }
        }
        Node::Block(block) => {
            let span = block.span();
            let inner = match block {
                rstml::node::NodeBlock::ValidBlock(b) => block_expr(b),
                invalid => invalid.to_token_stream(),
            };
            quote_spanned! {span=> ::rmx::IntoNode::into_node(#inner) }
        }
        Node::Fragment(frag) => {
            let children = children(&frag.children);
            quote! { ::rmx::Node::Fragment(::std::vec![#(#children),*]) }
        }
        Node::Doctype(doc) => {
            let s = format!("<!DOCTYPE {}>", doc.value.to_string_best());
            quote! { ::rmx::Node::Raw(::std::borrow::Cow::Borrowed(#s)) }
        }
        Node::Comment(_) => return None,
        Node::Custom(_) => unreachable!("no custom nodes configured"),
    })
}

fn children(nodes: &[Node]) -> Vec<Tokens> {
    nodes.iter().filter_map(gen_node).collect()
}

fn is_component(name: &NodeName) -> bool {
    match name {
        NodeName::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|s| s.ident.to_string().starts_with(|c: char| c.is_ascii_uppercase())),
        _ => false,
    }
}

fn attr_value(attr: &KeyedAttribute) -> Option<&Expr> {
    attr.value()
}

/// `{expr}` becomes `expr` (avoids "unnecessary braces" lints at every call site); blocks
/// with statements are kept as blocks.
fn block_expr(block: &syn::Block) -> Tokens {
    match block.stmts.as_slice() {
        [syn::Stmt::Expr(expr, None)] => expr.to_token_stream(),
        // `{rsx! { .. }}`: a brace-delimited macro call parses as a statement.
        [syn::Stmt::Macro(m)] if m.semi_token.is_none() && m.attrs.is_empty() => m.mac.to_token_stream(),
        _ => block.to_token_stream(),
    }
}

fn value_tokens(expr: &Expr) -> Tokens {
    match expr {
        Expr::Block(b) if b.attrs.is_empty() && b.label.is_none() => block_expr(&b.block),
        other => other.to_token_stream(),
    }
}

fn string_literal(expr: &Expr) -> Option<&syn::LitStr> {
    match expr {
        Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) => Some(s),
        _ => None,
    }
}

fn gen_element(el: &NodeElement<rstml::Infallible>) -> Tokens {
    if is_component(el.name()) {
        return gen_component(el);
    }
    let tag = el.name().to_string();
    let mut key = quote! { ::core::option::Option::None };
    let mut attrs = Vec::new();
    let mut events = Vec::new();
    let mut mixins: Vec<Tokens> = Vec::new();
    for attr in el.attributes() {
        let attr = match attr {
            NodeAttribute::Attribute(a) => a,
            NodeAttribute::Block(b) => {
                return syn::Error::new(b.span(), "rsx!: spread attributes are not supported").to_compile_error()
            }
        };
        let name = attr.key.to_string();
        let value = attr_value(attr);
        if name == "key" {
            let Some(v) = value else {
                return syn::Error::new(attr.span(), "rsx!: key needs a value").to_compile_error();
            };
            let v2 = value_tokens(v);
            key = quote_spanned! {v.span()=> ::core::option::Option::Some(::rmx::Key::from(#v2)) };
        } else if name == "mix" {
            let Some(v) = value else {
                return syn::Error::new(attr.span(), "rsx!: mix needs a value").to_compile_error();
            };
            let v2 = value_tokens(v);
            mixins.push(quote_spanned! {v.span()=> ::rmx::IntoMixins::into_mixins(#v2) });
        } else if let Some(event) = name.strip_prefix("on:") {
            let Some(v) = value else {
                return syn::Error::new(attr.span(), "rsx!: event needs a handler").to_compile_error();
            };
            let v2 = value_tokens(v);
            events.push(quote_spanned! {v.span()=> (#event, #v2) });
        } else {
            let v = match value {
                None => quote! { ::rmx::AttrValue::Bool(true) },
                Some(v) => match string_literal(v) {
                    Some(lit) => quote! { ::rmx::AttrValue::Str(::std::borrow::Cow::Borrowed(#lit)) },
                    None => { let v2 = value_tokens(v); quote_spanned! {v.span()=> ::rmx::IntoAttr::into_attr(#v2) } }
                },
            };
            attrs.push(quote! { (#name, #v) });
        }
    }
    let children = children(&el.children);
    if mixins.is_empty() {
        return quote! {
            ::rmx::Node::element(
                #tag,
                #key,
                ::std::vec![#(#attrs),*],
                ::std::vec![#(#events),*],
                ::std::vec![#(#children),*],
            )
        };
    }
    quote! {
        ::rmx::Node::element_with_mixins(
            #tag,
            #key,
            ::std::vec![#(#attrs),*],
            ::std::vec![#(#events),*],
            ::std::vec![#(#children),*],
            {
                let mut __mixins = ::std::vec::Vec::new();
                #( __mixins.extend(#mixins); )*
                __mixins
            },
        )
    }
}

fn gen_component(el: &NodeElement<rstml::Infallible>) -> Tokens {
    let NodeName::Path(path) = el.name() else { unreachable!("checked by is_component") };
    let component = path.path.clone();

    let mut key = quote! { ::core::option::Option::None };
    let mut fields = Vec::new();
    for attr in el.attributes() {
        let attr = match attr {
            NodeAttribute::Attribute(a) => a,
            NodeAttribute::Block(b) => {
                return syn::Error::new(b.span(), "rsx!: spread props are not supported").to_compile_error()
            }
        };
        let name = attr.key.to_string();
        let value = attr_value(attr);
        if name == "key" {
            if let Some(v) = value {
                let v2 = value_tokens(v);
            key = quote_spanned! {v.span()=> ::core::option::Option::Some(::rmx::Key::from(#v2)) };
            }
            continue;
        }
        let field = syn::Ident::new(&name.replace('-', "_"), attr.key.span());
        // Values go through `IntoProp` so `"text"` fills a `String` prop and a value fills
        // an `Option<_>` prop without `Some(..)`.
        let v = match value {
            None => quote! { true },
            Some(v) => {
                let v2 = value_tokens(v);
                quote_spanned! {v.span()=> ::rmx::IntoProp::into_prop(#v2) }
            }
        };
        fields.push(quote! { #field: #v });
    }
    let kids = children(&el.children);
    if !kids.is_empty() {
        let field = syn::Ident::new("children", Span::call_site());
        fields.push(quote! {
            #field: ::core::convert::From::from(::rmx::Node::Fragment(::std::vec![#(#kids),*]))
        });
    }
    // Props not given in the template take their `Default` values, so optional props
    // work like they do in JSX. Props types therefore implement `Default`.
    let props = if fields.is_empty() {
        quote! { ::core::default::Default::default() }
    } else {
        quote! {
            {
                // The props struct is named through the component, so it need not be imported.
                type __Props = <#component as ::rmx::Component>::Props;
                #[allow(clippy::needless_update)]
                let __props = __Props { #(#fields,)* ..::core::default::Default::default() };
                __props
            }
        }
    };
    quote! { ::rmx::Node::component::<#component>(#key, #props) }
}
