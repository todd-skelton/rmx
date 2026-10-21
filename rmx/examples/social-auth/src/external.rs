//! External login (utils/external-auth.ts, actions/auth/{google,github,x}/controller.ts and
//! resolve-external-auth.ts) on rmx-auth.

use std::sync::Arc;

use rmx::auth::{
    finish_external_auth, github_provider, google_provider, start_external_auth, x_provider, ClientOptions, Fetch, FinishExternalAuthOptions, GitHubProfile,
    GitHubProvider, OAuthProvider, OidcProfile, OidcProvider, StartExternalAuthOptions, XProfile, XProvider,
};
use rmx::fetch::{response, BoxError, BoxFuture, Request, Response};
use rmx::middleware::auth::complete_auth;
use rmx::middleware::session::SessionExt;
use rmx::router::{action, Context, Router};

use crate::routes::{External, ExternalActions, ROUTES};
use serde::Serialize;

use crate::data::{normalize_email, ExternalProfile};
use crate::{db, with_return_to, AuthSession};

/// The env var prefix and label of each provider, in display order.
pub const PROVIDER_NAMES: [(&str, &str, &str); 3] = [("google", "Google", "GOOGLE"), ("github", "GitHub", "GITHUB"), ("x", "X", "X")];

pub fn provider_label(name: &str) -> &'static str {
    PROVIDER_NAMES.iter().find(|(n, ..)| *n == name).map_or("Credentials", |(_, label, _)| label)
}

/// The configured providers (`ExternalProviderRegistry`). A provider is `None` when its
/// client id or secret is missing.
#[derive(Clone, Default)]
pub struct ExternalProviders {
    pub google: Option<Arc<OidcProvider>>,
    pub github: Option<Arc<GitHubProvider>>,
    pub x: Option<Arc<XProvider>>,
    missing: Vec<(&'static str, Vec<String>)>,
}

impl ExternalProviders {
    /// `createExternalProviderRegistry({ env, origin })`: read `<PREFIX>_CLIENT_ID` and
    /// `<PREFIX>_CLIENT_SECRET` through `env`; callbacks go to `<origin>/auth/<name>/callback`.
    /// Providers make their HTTP requests through `fetch`.
    pub fn from_env(origin: &str, env: impl Fn(&str) -> Option<String>, fetch: impl Fetch) -> ExternalProviders {
        let fetch: Arc<dyn Fetch> = Arc::new(fetch);
        let shared = || {
            let fetch = fetch.clone();
            move |req: Request| -> BoxFuture<Result<Response, BoxError>> { fetch.fetch(req) }
        };
        let mut providers = ExternalProviders::default();
        for (name, _, prefix) in PROVIDER_NAMES {
            let (id, secret) = (env(&format!("{prefix}_CLIENT_ID")).filter(|v| !v.is_empty()), env(&format!("{prefix}_CLIENT_SECRET")).filter(|v| !v.is_empty()));
            let missing: Vec<String> = [("CLIENT_ID", &id), ("CLIENT_SECRET", &secret)].iter().filter(|(_, v)| v.is_none()).map(|(k, _)| format!("{prefix}_{k}")).collect();
            providers.missing.push((name, missing));
            let (Some(id), Some(secret)) = (id, secret) else { continue };
            let client = ClientOptions::new(id, secret, format!("{}/auth/{name}/callback", origin.trim_end_matches('/')));
            match name {
                "google" => providers.google = Some(Arc::new(google_provider(client, shared()))),
                "github" => providers.github = Some(Arc::new(github_provider(client, shared()))),
                _ => providers.x = Some(Arc::new(x_provider(client, shared()))),
            }
        }
        providers
    }

    pub fn is_enabled(&self, name: &str) -> bool {
        match name {
            "google" => self.google.is_some(),
            "github" => self.github.is_some(),
            _ => self.x.is_some(),
        }
    }

    /// The env vars a provider still needs.
    pub fn missing_env_vars(&self, name: &str) -> &[String] {
        self.missing.iter().find(|(n, _)| *n == name).map_or(&[], |(_, m)| m.as_slice())
    }

    /// One link per provider (`readExternalProviderLinks`): an href to start login when
    /// configured, else the reason it is disabled.
    pub fn links(&self, ctx: &Context) -> Vec<ProviderLink> {
        PROVIDER_NAMES
            .iter()
            .map(|&(name, label, _)| {
                if self.is_enabled(name) {
                    return ProviderLink { name, label, href: Some(with_return_to(&format!("/auth/{name}/login"), ctx)), disabled_reason: None };
                }
                let missing = self.missing_env_vars(name);
                let reason = if missing.is_empty() { format!("{label} login is not configured.") } else { format!("Set {} to enable {label} login.", missing.join(" and ")) };
                ProviderLink { name, label, href: None, disabled_reason: Some(reason) }
            })
            .collect()
    }
}

/// `http://127.0.0.1:<port>` (`getDemoOrigin`), the origin provider callbacks return to.
pub fn demo_origin(port: &str) -> String {
    format!("http://127.0.0.1:{port}")
}

pub struct ProviderLink {
    pub name: &'static str,
    pub label: &'static str,
    pub href: Option<String>,
    pub disabled_reason: Option<String>,
}

/// The login and callback routes for every provider.
pub fn routes(router: &mut Router, providers: &ExternalProviders) {
    controller(router, &ROUTES.auth.google, "google", providers.google.clone(), google_profile);
    controller(router, &ROUTES.auth.github, "github", providers.github.clone(), github_profile);
    controller(router, &ROUTES.auth.x, "x", providers.x.clone(), x_profile);
}

/// `actions/auth/<provider>/controller.ts`
fn controller<P: OAuthProvider>(
    router: &mut Router,
    routes: &External,
    name: &'static str,
    provider: Option<Arc<P>>,
    extract: fn(&P::Profile) -> ExternalProfile,
) where
    P::Profile: Serialize,
{
    let label = provider_label(name);
    let login = provider.clone();
    let login = action(move |ctx: Context| {
        let provider = login.clone();
        async move {
            let Some(provider) = provider else {
                ctx.session().flash("error", format!("{label} login is not configured."));
                return response::redirect(&with_return_to("/", &ctx));
            };
            let return_to = ctx.url().search_params().get("returnTo").map(str::to_string);
            match start_external_auth(&*provider, &ctx, StartExternalAuthOptions { return_to, ..Default::default() }).await {
                Ok(res) => res,
                Err(_) => {
                    ctx.session().flash("error", format!("We could not start {label} login."));
                    response::redirect(&with_return_to("/", &ctx))
                }
            }
        }
    });
    let callback = action(move |ctx: Context| {
        let provider = provider.clone();
        async move {
            let Some(provider) = provider else {
                ctx.session().flash("error", format!("{label} login is not configured."));
                return response::redirect("/");
            };
            let Ok(finished) = finish_external_auth(&*provider, &ctx, FinishExternalAuthOptions::default()).await else {
                ctx.session().flash("error", format!("We could not finish {label} login."));
                return response::redirect("/");
            };
            let result = finished.result;
            let (user, account) = db(&ctx).resolve_external_auth(&result.provider, &result.account.provider_account_id, &extract(&result.profile));
            complete_auth(&ctx).set("auth", AuthSession { user_id: user.id, login_method: result.provider, auth_account_id: Some(account.id) });
            response::redirect(finished.return_to.as_deref().unwrap_or("/account"))
        }
    });
    router.map(routes, ExternalActions { login, callback });
}

fn json_of(profile: &impl Serialize) -> String {
    serde_json::to_string(profile).unwrap_or_else(|_| "{}".into())
}

fn google_profile(profile: &OidcProfile) -> ExternalProfile {
    let email = profile.email().map(normalize_email);
    ExternalProfile {
        linkable_email: email.clone().filter(|_| profile.email_verified() != Some(false)),
        email,
        username: profile.preferred_username().map(str::to_string),
        display_name: profile.name().map(str::to_string),
        avatar_url: profile.picture().map(str::to_string),
        profile_json: json_of(profile),
    }
}

fn github_profile(profile: &GitHubProfile) -> ExternalProfile {
    let email = profile.email.as_deref().map(normalize_email);
    ExternalProfile {
        linkable_email: email.clone(),
        email,
        username: Some(profile.login.clone()),
        display_name: Some(profile.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| profile.login.clone())),
        avatar_url: profile.avatar_url.clone(),
        profile_json: json_of(profile),
    }
}

fn x_profile(profile: &XProfile) -> ExternalProfile {
    ExternalProfile {
        username: Some(profile.username.clone()),
        display_name: Some(profile.name.clone()),
        avatar_url: profile.profile_image_url.clone(),
        profile_json: json_of(profile),
        ..Default::default()
    }
}
