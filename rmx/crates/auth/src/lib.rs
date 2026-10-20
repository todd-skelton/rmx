//! External login with OAuth 2 and OpenID Connect, ported from `remix/auth`.
//!
//! A provider knows how to build an authorization URL and turn a callback into an
//! [OAuthResult]. The flow functions do the rest with the request's session:
//!
//! 1. [start_external_auth] stores an [OAuthTransaction] (state, PKCE verifier, return target)
//!    in the session and redirects to the provider.
//! 2. [finish_external_auth] checks the callback's `state`, lets the provider exchange the
//!    code for tokens and load the profile, and clears the transaction.
//! 3. [refresh_external_auth] trades a refresh token for new tokens.
//!
//! Built-in providers: [github_provider], [google_provider], [x_provider],
//! [facebook_provider], [microsoft_provider], [okta_provider], [auth0_provider] and
//! [oidc_provider] for any OpenID Connect issuer. Implement [OAuthProvider] for others
//! (`createOAuthProvider`).
//!
//! Providers make HTTP requests through the [Fetch] they are given, so the app picks the
//! HTTPS client and tests answer in memory. Credentials login and `completeAuth` live in
//! `rmx_middleware::auth` (`credentials_provider`, `verify_credentials`, `complete_auth`).
//!
//! ```
//! use rmx_auth::{github_provider, start_external_auth, ClientOptions, StartExternalAuthOptions};
//! use rmx_fetch::{BoxError, Request, Response};
//! use rmx_router::Context;
//!
//! async fn https_fetch(_req: Request) -> Result<Response, BoxError> {
//!     unimplemented!("any HTTPS client")
//! }
//!
//! let github = std::sync::Arc::new(github_provider(
//!     ClientOptions::new("client-id", "client-secret", "https://app.example/auth/github/callback"),
//!     https_fetch,
//! ));
//! let login = move |ctx: Context| {
//!     let github = github.clone();
//!     async move {
//!         let return_to = ctx.url().search_params().get("returnTo").map(str::to_string);
//!         start_external_auth(&*github, &ctx, StartExternalAuthOptions { return_to, ..Default::default() })
//!             .await
//!             .unwrap_or_else(|_| rmx_fetch::response::redirect("/"))
//!     }
//! };
//! # let _ = login;
//! ```

mod oauth;
mod providers;

use std::fmt;
use std::future::Future;
use std::time::SystemTime;

use rmx_fetch::Response;
use rmx_router::Context;
use rmx_session::Session;
use rmx_url::Url;
use serde::{Deserialize, Serialize};

pub use oauth::{
    authorization_code, authorization_url, code_challenge, create_transaction, exchange_authorization_code, exchange_refresh_token, fetch_json,
    merge_refreshed_tokens, sanitize_return_to, ClientAuthentication, TokenRequest,
};
pub use providers::{
    auth0_provider, facebook_provider, github_provider, google_provider, microsoft_provider, oidc_provider, okta_provider, x_provider, Auth0Options,
    ClientOptions, FacebookPicture, FacebookProfile, FacebookProvider, GitHubEmail, GitHubProfile, GitHubProvider, MicrosoftOptions, OidcMetadata,
    OidcOptions, OidcProfile, OidcProvider, XProfile, XProvider,
};
pub use rmx_fetch::Fetch;

/// Why an auth step failed. The messages match Remix's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthError(pub String);

impl AuthError {
    pub fn new(message: impl Into<String>) -> AuthError {
        AuthError(message.into())
    }
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AuthError {}

/// Tokens from a provider's token endpoint.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthTokens {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<SystemTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
}

/// The provider and the user's id there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthAccount {
    pub provider: String,
    pub provider_account_id: String,
}

/// What a finished login returns.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OAuthResult<P> {
    pub provider: String,
    pub account: OAuthAccount,
    pub profile: P,
    pub tokens: OAuthTokens,
}

impl<P> OAuthResult<P> {
    /// The same result with the profile mapped to the app's shape (`mapProfile`).
    pub fn map_profile<Q>(self, map: impl FnOnce(P) -> Q) -> OAuthResult<Q> {
        OAuthResult { provider: self.provider, account: self.account, profile: map(self.profile), tokens: self.tokens }
    }
}

/// The state kept in the session between redirecting to the provider and its callback.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthTransaction {
    pub provider: String,
    pub state: String,
    pub code_verifier: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_state: Option<String>,
}

/// An OAuth provider (`createOAuthProvider(name, runtime)`).
pub trait OAuthProvider: Send + Sync + 'static {
    type Profile: Send + 'static;

    /// The provider name stored with the transaction and in results.
    fn name(&self) -> &str;

    /// Where to send the user to log in.
    fn authorization_url(&self, transaction: &OAuthTransaction) -> impl Future<Output = Result<Url, AuthError>> + Send;

    /// Exchange the callback's code for tokens and load the profile. `callback` is the full
    /// callback URL.
    fn handle_callback(&self, callback: &Url, transaction: &OAuthTransaction) -> impl Future<Output = Result<OAuthResult<Self::Profile>, AuthError>> + Send;

    /// Exchange a refresh token. Providers without refresh support fail.
    fn refresh_tokens(&self, tokens: &OAuthTokens) -> impl Future<Output = Result<OAuthTokens, AuthError>> + Send {
        let message = format!("OAuth provider \"{}\" does not support refresh-token exchange.", self.name());
        let _ = tokens;
        async move { Err(AuthError(message)) }
    }
}

const DEFAULT_TRANSACTION_KEY: &str = "__auth";

fn session_of(ctx: &Context, source: &str) -> Result<Session, AuthError> {
    ctx.get::<Session>()
        .cloned()
        .ok_or_else(|| AuthError(format!("Session not found. Make sure session() middleware runs before {source}.")))
}

/// Options for [start_external_auth].
#[derive(Clone, Debug, Default)]
pub struct StartExternalAuthOptions {
    /// Session key for the transaction. Default `__auth`.
    pub transaction_key: Option<String>,
    /// Where to go after login. Only local paths are kept (see [sanitize_return_to]).
    pub return_to: Option<String>,
}

/// Store a new transaction in the session and redirect (302) to the provider.
pub fn start_external_auth<'a, P: OAuthProvider>(provider: &'a P, ctx: &Context, options: StartExternalAuthOptions) -> impl Future<Output = Result<Response, AuthError>> + Send + 'a {
    let session = session_of(ctx, "startExternalAuth()");
    async move {
        let session = session?;
        let transaction = create_transaction(provider.name(), sanitize_return_to(options.return_to.as_deref()));
        let url = provider.authorization_url(&transaction).await?;
        session.set(options.transaction_key.as_deref().unwrap_or(DEFAULT_TRANSACTION_KEY), &transaction);
        Ok(rmx_fetch::response::redirect_with(&url.href(), http::StatusCode::FOUND))
    }
}

/// Options for [finish_external_auth].
#[derive(Clone, Debug, Default)]
pub struct FinishExternalAuthOptions {
    /// Session key for the transaction. Default `__auth`.
    pub transaction_key: Option<String>,
}

/// A finished login and where to send the user next.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishedExternalAuth<P> {
    pub result: OAuthResult<P>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_to: Option<String>,
}

/// Validate the provider's callback and finish the login. The transaction is removed from the
/// session whether this succeeds or fails. Call `complete_auth` before writing the login to
/// the session, to rotate the session id.
pub fn finish_external_auth<'a, P: OAuthProvider>(
    provider: &'a P,
    ctx: &Context,
    options: FinishExternalAuthOptions,
) -> impl Future<Output = Result<FinishedExternalAuth<P::Profile>, AuthError>> + Send + 'a {
    let session = session_of(ctx, "finishExternalAuth()");
    let url = ctx.url().clone();
    async move {
        let session = session?;
        let key = options.transaction_key.unwrap_or_else(|| DEFAULT_TRANSACTION_KEY.into());
        let result = finish(provider, &session, &key, &url).await;
        if result.is_err() && session.has(&key) {
            session.unset(&key);
        }
        result
    }
}

async fn finish<P: OAuthProvider>(provider: &P, session: &Session, key: &str, url: &Url) -> Result<FinishedExternalAuth<P::Profile>, AuthError> {
    let transaction: Option<OAuthTransaction> = session.get(key);
    let params = url.search_params();
    if let Some(error) = params.get("error") {
        return Err(AuthError(params.get("error_description").unwrap_or(error).to_string()));
    }
    let transaction = transaction
        .filter(|t| t.provider == provider.name())
        .ok_or_else(|| AuthError(format!("Missing OAuth transaction for \"{}\".", provider.name())))?;
    let state = params.get("state").filter(|s| !s.is_empty()).ok_or_else(|| AuthError("Missing \"state\" in OAuth callback request.".into()))?;
    if state != transaction.state {
        return Err(AuthError("Invalid OAuth state.".into()));
    }
    let result = provider.handle_callback(url, &transaction).await?;
    session.unset(key);
    Ok(FinishedExternalAuth { result, return_to: sanitize_return_to(transaction.return_to.as_deref()) })
}

/// Refreshed tokens.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RefreshedExternalAuth {
    pub provider: String,
    pub tokens: OAuthTokens,
}

/// Exchange `tokens.refresh_token` for new tokens (fields the provider leaves out are kept).
pub async fn refresh_external_auth<P: OAuthProvider>(provider: &P, tokens: &OAuthTokens) -> Result<RefreshedExternalAuth, AuthError> {
    Ok(RefreshedExternalAuth { provider: provider.name().to_string(), tokens: provider.refresh_tokens(tokens).await? })
}

#[cfg(test)]
mod tests;
