//! Port of Remix's `providers/credentials.test.ts`. `createCredentialsAuthProvider` is
//! `rmx_auth_middleware::credentials_provider`.

use std::future::{ready, Ready};

use rmx_auth_middleware::credentials_provider;
use rmx_fetch_router::Context;

fn parse(ctx: &Context) -> String {
    ctx.url().search_params().get("email").unwrap_or_default().to_string()
}

fn verify(input: String, _ctx: &Context) -> Ready<Option<&'static str>> {
    ready((!input.is_empty()).then_some("u1"))
}

#[test]
fn defaults_the_provider_name_to_password_and_preserves_the_configured_hooks() {
    let provider = credentials_provider(parse, verify);

    assert_eq!(provider.name(), "password");
    assert!(std::ptr::fn_addr_eq(*provider.parse() as fn(&Context) -> String, parse as fn(&Context) -> String));
    assert!(std::ptr::fn_addr_eq(*provider.verify() as fn(String, &Context) -> Ready<Option<&'static str>>, verify as fn(String, &Context) -> Ready<Option<&'static str>>));
}

#[test]
fn uses_the_configured_provider_name() {
    let provider = credentials_provider(|_: &Context| String::new(), |_: String, _: &Context| ready(None::<()>)).named("email-link");

    assert_eq!(provider.name(), "email-link");
}
