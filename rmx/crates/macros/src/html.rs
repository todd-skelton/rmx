//! `html!` and `html_raw!`: format-style templates that build `rmx_html_template::SafeHtml`.

use proc_macro2::{Span, TokenStream as Tokens};
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{Expr, Ident, LitStr, Token};

pub struct Template {
    literal: LitStr,
    positional: Vec<Expr>,
    named: Vec<(Ident, Expr)>,
}

impl Parse for Template {
    fn parse(input: ParseStream) -> syn::Result<Template> {
        let literal: LitStr = input.parse()?;
        let mut positional = Vec::new();
        let mut named = Vec::new();
        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            if input.peek(Ident) && input.peek2(Token![=]) && !input.peek2(Token![==]) {
                let name: Ident = input.parse()?;
                input.parse::<Token![=]>()?;
                named.push((name, input.parse()?));
            } else if named.is_empty() {
                positional.push(input.parse()?);
            } else {
                return Err(input.error("positional arguments must come before named ones"));
            }
        }
        Ok(Template { literal, positional, named })
    }
}

enum Piece {
    Text(String),
    Positional(usize),
    Named(String),
}

fn pieces(template: &str, span: Span) -> syn::Result<Vec<Piece>> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut next = 0;
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                text.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                text.push('}');
            }
            '}' => return Err(syn::Error::new(span, "unmatched '}' in template; write '}}' for a literal brace")),
            '{' => {
                let mut inner = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) => inner.push(c),
                        None => return Err(syn::Error::new(span, "unclosed '{' in template; write '{{' for a literal brace")),
                    }
                }
                let inner = inner.trim();
                if !text.is_empty() {
                    out.push(Piece::Text(std::mem::take(&mut text)));
                }
                if inner.is_empty() {
                    out.push(Piece::Positional(next));
                    next += 1;
                } else if let Ok(index) = inner.parse::<usize>() {
                    out.push(Piece::Positional(index));
                } else if syn::parse_str::<Ident>(inner).is_ok() {
                    out.push(Piece::Named(inner.to_string()));
                } else {
                    return Err(syn::Error::new(span, format!("unsupported placeholder {{{inner}}}; use {{}}, {{0}} or {{name}}")));
                }
            }
            c => text.push(c),
        }
    }
    if !text.is_empty() {
        out.push(Piece::Text(text));
    }
    Ok(out)
}

pub fn expand(template: Template, raw: bool) -> syn::Result<Tokens> {
    let span = template.literal.span();
    let pieces = pieces(&template.literal.value(), span)?;
    let krate = quote!(::rmx_html_template::__private);
    let push = if raw { quote!(push_raw) } else { quote!(push) };

    let mut bindings = Vec::new();
    for (i, expr) in template.positional.iter().enumerate() {
        let name = format_ident!("__rmx_arg{}", i, span = Span::mixed_site());
        bindings.push(quote! { let #name = &(#expr); });
    }
    for (ident, expr) in &template.named {
        let name = format_ident!("__rmx_named_{}", ident, span = Span::mixed_site());
        bindings.push(quote! { let #name = &(#expr); });
    }

    let mut used = vec![false; template.positional.len()];
    let mut used_named = vec![false; template.named.len()];
    let mut capacity = 0;
    let mut steps = Vec::new();
    for piece in &pieces {
        steps.push(match piece {
            Piece::Text(text) => {
                capacity += text.len();
                quote! { __rmx_out.push_str(#text); }
            }
            Piece::Positional(i) => {
                let Some(flag) = used.get_mut(*i) else {
                    return Err(syn::Error::new(span, format!("the template refers to argument {i} but has {} positional arguments", template.positional.len())));
                };
                *flag = true;
                capacity += 16;
                let name = format_ident!("__rmx_arg{}", i, span = Span::mixed_site());
                quote! { __rmx_out.#push(#name); }
            }
            Piece::Named(name) => {
                capacity += 16;
                match template.named.iter().position(|(ident, _)| ident == name) {
                    Some(i) => {
                        used_named[i] = true;
                        let binding = format_ident!("__rmx_named_{}", name, span = Span::mixed_site());
                        quote! { __rmx_out.#push(#binding); }
                    }
                    None => {
                        let captured = Ident::new(name, span);
                        quote! { __rmx_out.#push(&#captured); }
                    }
                }
            }
        });
    }
    if let Some(i) = used.iter().position(|u| !u) {
        return Err(syn::Error::new(span, format!("positional argument {i} is never used")));
    }
    if let Some(i) = used_named.iter().position(|u| !u) {
        return Err(syn::Error::new(template.named[i].0.span(), "named argument is never used"));
    }
    let out = Ident::new("__rmx_out", Span::mixed_site());
    let steps = steps.into_iter().map(|step| {
        // Re-span the builder name so it resolves to the mixed-site binding below.
        let mut tokens = Tokens::new();
        for tt in step {
            match tt {
                proc_macro2::TokenTree::Ident(ident) if ident == "__rmx_out" => tokens.extend(quote!(#out)),
                other => tokens.extend(std::iter::once(other)),
            }
        }
        tokens
    });
    Ok(quote! {{
        #(#bindings)*
        let mut #out = #krate::Builder::new(#capacity);
        #(#steps)*
        #out.finish()
    }})
}
