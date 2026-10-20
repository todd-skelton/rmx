//! Building blocks for providers (`remix/auth`'s provider.ts and utils.ts).

use std::time::{Duration, SystemTime};

use http::header;
use rmx_fetch::{Body, Fetch, Request};
use rmx_url::{SearchParams, Url};
use serde_json::{Map, Value};

use crate::{AuthError, OAuthTokens, OAuthTransaction};

/// A random URL-safe token from `bytes` random bytes.
fn random_token<const N: usize>() -> String {
    rmx_crypto::base64url_encode(&rmx_crypto::random_bytes::<N>())
}

/// The PKCE S256 challenge for a verifier.
pub fn code_challenge(code_verifier: &str) -> String {
    rmx_crypto::base64url_encode(&rmx_crypto::sha256(code_verifier.as_bytes()))
}

/// A fresh transaction: random state and a 64-character PKCE verifier.
pub fn create_transaction(provider: &str, return_to: Option<String>) -> OAuthTransaction {
    OAuthTransaction { provider: provider.to_string(), state: random_token::<32>(), code_verifier: random_token::<48>(), return_to, provider_state: None }
}

const RETURN_TO_BASE: &str = "https://remix.local";

/// A local path (with query and fragment) that cannot leave the site, or `None`. Paths that
/// normalize to `//host` are refused.
pub fn sanitize_return_to(value: Option<&str>) -> Option<String> {
    let value = value.filter(|v| v.starts_with('/'))?;
    let base = Url::parse(RETURN_TO_BASE).ok()?;
    let url = base.join(value).ok()?;
    if url.origin() != RETURN_TO_BASE || url.pathname().starts_with("//") {
        return None;
    }
    let hash = url.fragment().filter(|f| !f.is_empty()).map(|f| format!("#{f}")).unwrap_or_default();
    Some(format!("{}{}{hash}", url.pathname(), url.search()))
}

/// `endpoint` with `params` set in its query.
pub fn authorization_url(endpoint: &str, params: &[(&str, &str)]) -> Result<Url, AuthError> {
    let mut url = Url::parse(endpoint).map_err(|_| AuthError(format!("Invalid URL: {endpoint}")))?;
    let mut search = url.search_params();
    for (key, value) in params {
        search.set(*key, *value);
    }
    url.set_search_params(&search);
    Ok(url)
}

/// The `code` from a callback URL.
pub fn authorization_code(callback: &Url) -> Result<String, AuthError> {
    callback
        .search_params()
        .get("code")
        .filter(|c| !c.is_empty())
        .map(str::to_string)
        .ok_or_else(|| AuthError("Missing authorization code in OAuth callback request.".into()))
}

/// How the client proves itself to the token endpoint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClientAuthentication {
    /// `client_id` and `client_secret` in the form body (the default).
    #[default]
    RequestBody,
    /// HTTP Basic `Authorization`.
    Basic,
}

/// A token endpoint call.
#[derive(Clone, Debug)]
pub struct TokenRequest<'a> {
    pub token_endpoint: &'a str,
    pub client_id: &'a str,
    pub client_secret: &'a str,
    pub client_authentication: ClientAuthentication,
    /// Extra request headers.
    pub headers: &'a [(&'a str, &'a str)],
}

/// Trade an authorization code (with its PKCE verifier) for tokens.
pub async fn exchange_authorization_code(fetch: &dyn Fetch, request: TokenRequest<'_>, redirect_uri: &str, code: &str, code_verifier: &str) -> Result<OAuthTokens, AuthError> {
    let mut params = SearchParams::new();
    params.set("code", code);
    params.set("code_verifier", code_verifier);
    params.set("grant_type", "authorization_code");
    params.set("redirect_uri", redirect_uri);
    exchange(fetch, request, params, "OAuth token exchange failed.").await
}

/// Trade a refresh token for new tokens.
pub async fn exchange_refresh_token(fetch: &dyn Fetch, request: TokenRequest<'_>, refresh_token: &str, scopes: &[String]) -> Result<OAuthTokens, AuthError> {
    let mut params = SearchParams::new();
    params.set("grant_type", "refresh_token");
    params.set("refresh_token", refresh_token);
    if !scopes.is_empty() {
        params.set("scope", scopes.join(" "));
    }
    exchange(fetch, request, params, "OAuth refresh token exchange failed.").await
}

/// New tokens, keeping current values the refresh response left out.
pub fn merge_refreshed_tokens(current: &OAuthTokens, refreshed: OAuthTokens) -> OAuthTokens {
    OAuthTokens {
        access_token: refreshed.access_token,
        refresh_token: refreshed.refresh_token.or_else(|| current.refresh_token.clone()),
        token_type: refreshed.token_type.or_else(|| current.token_type.clone()),
        expires_at: refreshed.expires_at.or(current.expires_at),
        scope: refreshed.scope.or_else(|| current.scope.clone()),
        id_token: refreshed.id_token.or_else(|| current.id_token.clone()),
    }
}

async fn exchange(fetch: &dyn Fetch, request: TokenRequest<'_>, mut params: SearchParams, fallback: &str) -> Result<OAuthTokens, AuthError> {
    if request.client_authentication == ClientAuthentication::RequestBody {
        params.set("client_id", request.client_id);
        params.set("client_secret", request.client_secret);
    }
    let mut builder = http::Request::post(request.token_endpoint)
        .header(header::ACCEPT, "application/json")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    if request.client_authentication == ClientAuthentication::Basic {
        let credentials = rmx_crypto::base64_encode(format!("{}:{}", request.client_id, request.client_secret).as_bytes());
        builder = builder.header(header::AUTHORIZATION, format!("Basic {credentials}"));
    }
    for (name, value) in request.headers {
        builder = builder.header(*name, *value);
    }
    let req = builder.body(Body::from(params.to_string())).map_err(|e| AuthError(e.to_string()))?;
    normalize_tokens(fetch_json(fetch, req, fallback).await?)
}

fn normalize_tokens(json: Value) -> Result<OAuthTokens, AuthError> {
    let Value::Object(data) = json else { return Err(AuthError("Expected OAuth provider to return a JSON object.".into())) };
    let text = |key: &str| data.get(key).and_then(Value::as_str).map(str::to_string);
    let access_token = text("access_token").filter(|t| !t.is_empty()).ok_or_else(|| AuthError("OAuth token response did not include an access token.".into()))?;
    Ok(OAuthTokens {
        access_token,
        refresh_token: text("refresh_token"),
        token_type: text("token_type"),
        expires_at: data.get("expires_in").and_then(Value::as_f64).map(|secs| SystemTime::now() + Duration::from_secs_f64(secs.max(0.0))),
        scope: text("scope").filter(|s| !s.is_empty()).map(|s| s.split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty()).map(str::to_string).collect()),
        id_token: text("id_token"),
    })
}

/// Send `request` and read a JSON body. A non-2xx status or an OAuth `error` field fails with
/// the provider's message, else `fallback`. An empty body reads as `{}`.
pub async fn fetch_json(fetch: &dyn Fetch, request: Request, fallback: &str) -> Result<Value, AuthError> {
    let response = fetch.fetch(request).await.map_err(|e| AuthError(e.to_string()))?;
    let ok = response.status().is_success();
    let text = response.into_body().text().await.map_err(|e| AuthError(e.to_string()))?;
    let json: Value = if text.is_empty() { Value::Object(Map::new()) } else { serde_json::from_str(&text).map_err(|e| AuthError(e.to_string()))? };
    if !ok || has_oauth_error(&json) {
        return Err(AuthError(oauth_error_message(&json, fallback)));
    }
    Ok(json)
}

/// A GET with JSON accept and a bearer token.
pub(crate) fn bearer_get(url: &str, access_token: &str, extra: &[(&str, &str)]) -> Result<Request, AuthError> {
    let mut builder = http::Request::get(url).header(header::ACCEPT, "application/json").header(header::AUTHORIZATION, format!("Bearer {access_token}"));
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::empty()).map_err(|e| AuthError(e.to_string()))
}

fn has_oauth_error(json: &Value) -> bool {
    json.as_object().is_some_and(|o| o.get("error").is_some_and(Value::is_string) || o.get("error_description").is_some_and(Value::is_string))
}

fn oauth_error_message(json: &Value, fallback: &str) -> String {
    let Some(data) = json.as_object() else { return fallback.to_string() };
    ["error_description", "error", "message"]
        .iter()
        .find_map(|key| data.get(*key).and_then(Value::as_str).filter(|s| !s.is_empty()))
        .unwrap_or(fallback)
        .to_string()
}
