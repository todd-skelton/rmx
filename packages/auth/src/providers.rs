//! The built-in providers (`remix/auth`'s providers/*.ts).

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use rmx_fetch::Fetch;
use rmx_url::Url;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::oauth::{authorization_code, authorization_url, bearer_get, code_challenge, exchange_authorization_code, exchange_refresh_token, fetch_json, merge_refreshed_tokens, ClientAuthentication, TokenRequest};
use crate::{AuthError, OAuthAccount, OAuthProvider, OAuthResult, OAuthTokens, OAuthTransaction};

/// An OAuth client registration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientOptions {
    pub client_id: String,
    pub client_secret: String,
    /// The app's callback URL, as registered with the provider.
    pub redirect_uri: String,
    /// Scopes to request; each provider has its own default.
    pub scopes: Option<Vec<String>>,
    /// Extra authorization URL parameters (`authorizationParams`, such as `prompt`).
    pub authorization_params: Vec<(String, String)>,
}

impl ClientOptions {
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>, redirect_uri: impl Into<String>) -> ClientOptions {
        ClientOptions { client_id: client_id.into(), client_secret: client_secret.into(), redirect_uri: redirect_uri.into(), scopes: None, authorization_params: Vec::new() }
    }

    pub fn scopes(mut self, scopes: impl IntoIterator<Item = impl Into<String>>) -> ClientOptions {
        self.scopes = Some(scopes.into_iter().map(Into::into).collect());
        self
    }

    pub fn authorization_param(mut self, name: impl Into<String>, value: impl Into<String>) -> ClientOptions {
        self.authorization_params.push((name.into(), value.into()));
        self
    }

    fn scope(&self, default: &[&str]) -> String {
        match &self.scopes {
            Some(scopes) => scopes.join(" "),
            None => default.join(" "),
        }
    }

    /// The authorization URL with PKCE, `authorization_params` first, then the standard ones.
    fn authorize(&self, endpoint: &str, default_scopes: &[&str], response_type: bool, transaction: &OAuthTransaction) -> Result<Url, AuthError> {
        let challenge = code_challenge(&transaction.code_verifier);
        let scope = self.scope(default_scopes);
        let mut params: Vec<(&str, &str)> = self.authorization_params.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        params.extend([("client_id", self.client_id.as_str()), ("redirect_uri", self.redirect_uri.as_str())]);
        if response_type {
            params.push(("response_type", "code"));
        }
        params.extend([("scope", scope.as_str()), ("state", transaction.state.as_str()), ("code_challenge", challenge.as_str()), ("code_challenge_method", "S256")]);
        authorization_url(endpoint, &params)
    }

    fn token_request<'a>(&'a self, endpoint: &'a str, auth: ClientAuthentication, headers: &'a [(&'a str, &'a str)]) -> TokenRequest<'a> {
        TokenRequest { token_endpoint: endpoint, client_id: &self.client_id, client_secret: &self.client_secret, client_authentication: auth, headers }
    }
}

fn account(provider: &str, id: impl Into<String>) -> OAuthAccount {
    OAuthAccount { provider: provider.to_string(), provider_account_id: id.into() }
}

fn decode<T: serde::de::DeserializeOwned>(value: Value, message: &str) -> Result<T, AuthError> {
    serde_json::from_value(value).map_err(|_| AuthError(message.to_string()))
}

// GitHub ---------------------------------------------------------------------------------

const GITHUB_AUTHORIZATION_ENDPOINT: &str = "https://github.com/login/oauth/authorize";
const GITHUB_TOKEN_ENDPOINT: &str = "https://github.com/login/oauth/access_token";
const GITHUB_USER_ENDPOINT: &str = "https://api.github.com/user";
const GITHUB_USER_EMAILS_ENDPOINT: &str = "https://api.github.com/user/emails";
const GITHUB_HEADERS: [(&str, &str); 1] = [("user-agent", "rmx-auth")];

/// A field GitHub sent, possibly as `null` (`Some(None)`); a missing field stays `None`.
fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

/// A GitHub user. Fields GitHub sends beyond these are kept in `extra`. `name` and `email`
/// tell a `null` (`Some(None)`) from a missing field (`None`) so the profile serializes as
/// GitHub sent it; read them with [GitHubProfile::name] and [GitHubProfile::email].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GitHubProfile {
    pub id: i64,
    #[serde(default)]
    pub login: String,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub name: Option<Option<String>>,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub email: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html_url: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl GitHubProfile {
    /// The display name, when GitHub sent one.
    pub fn name(&self) -> Option<&str> {
        self.name.as_ref().and_then(Option::as_deref)
    }

    /// The profile email, or the verified address looked up when the profile had none.
    pub fn email(&self) -> Option<&str> {
        self.email.as_ref().and_then(Option::as_deref)
    }
}

/// One of a GitHub user's email addresses.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubEmail {
    pub email: String,
    #[serde(default)]
    pub primary: bool,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub visibility: Option<String>,
}

/// GitHub OAuth (`createGitHubAuthProvider`). Default scopes `read:user user:email`. When the
/// profile has no public email, the primary verified address is looked up.
pub struct GitHubProvider {
    options: ClientOptions,
    fetch: Arc<dyn Fetch>,
}

pub fn github_provider(options: ClientOptions, fetch: impl Fetch) -> GitHubProvider {
    GitHubProvider { options, fetch: Arc::new(fetch) }
}

impl OAuthProvider for GitHubProvider {
    type Profile = GitHubProfile;
    type Tokens = OAuthTokens;

    fn name(&self) -> &str {
        "github"
    }

    async fn authorization_url(&self, transaction: &mut OAuthTransaction) -> Result<Url, AuthError> {
        self.options.authorize(GITHUB_AUTHORIZATION_ENDPOINT, &["read:user", "user:email"], false, transaction)
    }

    async fn handle_callback(&self, callback: &Url, transaction: &OAuthTransaction) -> Result<OAuthResult<GitHubProfile>, AuthError> {
        let code = authorization_code(callback)?;
        let accept = [("accept", "application/json")];
        let request = self.options.token_request(GITHUB_TOKEN_ENDPOINT, ClientAuthentication::RequestBody, &accept);
        let tokens = exchange_authorization_code(&*self.fetch, request, &self.options.redirect_uri, &code, &transaction.code_verifier).await?;
        let json = fetch_json(&*self.fetch, bearer_get(GITHUB_USER_ENDPOINT, &tokens.access_token, &GITHUB_HEADERS)?, "Failed to load GitHub profile.").await?;
        if !json.get("id").is_some_and(|id| id.is_i64() || id.is_u64()) {
            return Err(AuthError("GitHub profile did not include a valid id.".into()));
        }
        let mut profile: GitHubProfile = decode(json, "GitHub profile did not include a valid id.")?;
        if profile.email().is_none() {
            let emails = fetch_json(&*self.fetch, bearer_get(GITHUB_USER_EMAILS_ENDPOINT, &tokens.access_token, &GITHUB_HEADERS)?, "Failed to load GitHub email addresses.").await?;
            let emails: Vec<GitHubEmail> = decode(emails, "Failed to load GitHub email addresses.")?;
            let pick = emails.iter().find(|e| e.primary && e.verified).or_else(|| emails.iter().find(|e| e.verified));
            if let Some(pick) = pick {
                profile.email = Some(Some(pick.email.clone()));
            }
        }
        Ok(OAuthResult { provider: "github".into(), account: account("github", profile.id.to_string()), profile, tokens })
    }
}

// X --------------------------------------------------------------------------------------

const X_AUTHORIZATION_ENDPOINT: &str = "https://x.com/i/oauth2/authorize";
const X_TOKEN_ENDPOINT: &str = "https://api.x.com/2/oauth2/token";
const X_PROFILE_ENDPOINT: &str = "https://api.x.com/2/users/me?user.fields=profile_image_url,verified,description,url";

/// An X user.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct XProfile {
    pub id: String,
    pub name: String,
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_image_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// X (Twitter) OAuth 2 (`createXAuthProvider`). Default scopes `tweet.read users.read`; the
/// token endpoint uses HTTP Basic client authentication. Supports refresh.
pub struct XProvider {
    options: ClientOptions,
    fetch: Arc<dyn Fetch>,
}

pub fn x_provider(options: ClientOptions, fetch: impl Fetch) -> XProvider {
    XProvider { options, fetch: Arc::new(fetch) }
}

impl OAuthProvider for XProvider {
    type Profile = XProfile;
    type Tokens = OAuthTokens;

    fn name(&self) -> &str {
        "x"
    }

    async fn authorization_url(&self, transaction: &mut OAuthTransaction) -> Result<Url, AuthError> {
        self.options.authorize(X_AUTHORIZATION_ENDPOINT, &["tweet.read", "users.read"], true, transaction)
    }

    async fn handle_callback(&self, callback: &Url, transaction: &OAuthTransaction) -> Result<OAuthResult<XProfile>, AuthError> {
        let code = authorization_code(callback)?;
        let request = self.options.token_request(X_TOKEN_ENDPOINT, ClientAuthentication::Basic, &[]);
        let tokens = exchange_authorization_code(&*self.fetch, request, &self.options.redirect_uri, &code, &transaction.code_verifier).await?;
        let json = fetch_json(&*self.fetch, bearer_get(X_PROFILE_ENDPOINT, &tokens.access_token, &[])?, "Failed to load X profile.").await?;
        let data = json.get("data").cloned().unwrap_or(Value::Null);
        for (field, message) in [("id", "X profile did not include a valid id."), ("username", "X profile did not include a valid username."), ("name", "X profile did not include a valid name.")] {
            if !data.get(field).and_then(Value::as_str).is_some_and(|v| !v.is_empty()) {
                return Err(AuthError(message.into()));
            }
        }
        let profile: XProfile = decode(data, "X profile did not include a valid id.")?;
        Ok(OAuthResult { provider: "x".into(), account: account("x", profile.id.clone()), profile, tokens })
    }

    async fn refresh_tokens(&self, tokens: &OAuthTokens) -> Result<OAuthTokens, AuthError> {
        let refresh = tokens.refresh_token.as_deref().filter(|t| !t.is_empty()).ok_or_else(|| AuthError("X provider did not receive a refresh token.".into()))?;
        let request = self.options.token_request(X_TOKEN_ENDPOINT, ClientAuthentication::Basic, &[]);
        Ok(merge_refreshed_tokens(tokens, exchange_refresh_token(&*self.fetch, request, refresh, &[]).await?))
    }
}

// Facebook -------------------------------------------------------------------------------

const FACEBOOK_AUTHORIZATION_ENDPOINT: &str = "https://www.facebook.com/dialog/oauth";
const FACEBOOK_TOKEN_ENDPOINT: &str = "https://graph.facebook.com/oauth/access_token";
const FACEBOOK_PROFILE_ENDPOINT: &str = "https://graph.facebook.com/me?fields=id,name,email,picture";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FacebookPicture {
    pub data: FacebookPictureData,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FacebookPictureData {
    pub url: String,
}

/// A Facebook user.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FacebookProfile {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture: Option<FacebookPicture>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Facebook Login (`createFacebookAuthProvider`). Default scopes `public_profile email`.
pub struct FacebookProvider {
    options: ClientOptions,
    fetch: Arc<dyn Fetch>,
}

pub fn facebook_provider(options: ClientOptions, fetch: impl Fetch) -> FacebookProvider {
    FacebookProvider { options, fetch: Arc::new(fetch) }
}

impl OAuthProvider for FacebookProvider {
    type Profile = FacebookProfile;
    type Tokens = OAuthTokens;

    fn name(&self) -> &str {
        "facebook"
    }

    async fn authorization_url(&self, transaction: &mut OAuthTransaction) -> Result<Url, AuthError> {
        self.options.authorize(FACEBOOK_AUTHORIZATION_ENDPOINT, &["public_profile", "email"], true, transaction)
    }

    async fn handle_callback(&self, callback: &Url, transaction: &OAuthTransaction) -> Result<OAuthResult<FacebookProfile>, AuthError> {
        let code = authorization_code(callback)?;
        let request = self.options.token_request(FACEBOOK_TOKEN_ENDPOINT, ClientAuthentication::RequestBody, &[]);
        let tokens = exchange_authorization_code(&*self.fetch, request, &self.options.redirect_uri, &code, &transaction.code_verifier).await?;
        let json = fetch_json(&*self.fetch, bearer_get(FACEBOOK_PROFILE_ENDPOINT, &tokens.access_token, &[])?, "Failed to load Facebook profile.").await?;
        if !json.get("id").and_then(Value::as_str).is_some_and(|id| !id.is_empty()) {
            return Err(AuthError("Facebook profile did not include a valid id.".into()));
        }
        let profile: FacebookProfile = decode(json, "Facebook profile did not include a valid id.")?;
        Ok(OAuthResult { provider: "facebook".into(), account: account("facebook", profile.id.clone()), profile, tokens })
    }
}

// OpenID Connect -------------------------------------------------------------------------

/// An OpenID Connect provider's discovery document (the fields used here).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OidcMetadata {
    #[serde(default)]
    pub issuer: String,
    #[serde(default)]
    pub authorization_endpoint: String,
    #[serde(default)]
    pub token_endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub userinfo_endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jwks_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_session_endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes_supported: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claims_supported: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_challenge_methods_supported: Option<Vec<String>>,
}

impl OidcMetadata {
    fn validate(self, name: &str) -> Result<OidcMetadata, AuthError> {
        for (value, field) in [(&self.issuer, "an issuer"), (&self.authorization_endpoint, "an authorization_endpoint"), (&self.token_endpoint, "a token_endpoint")] {
            if value.is_empty() {
                return Err(AuthError(format!("OIDC metadata for \"{name}\" did not include {field}.")));
            }
        }
        Ok(self)
    }
}

/// The userinfo claims. `sub` is always present; read others with the accessors or [`OidcProfile::claim`].
#[derive(Clone, Debug, PartialEq)]
pub struct OidcProfile {
    pub sub: String,
    pub claims: Map<String, Value>,
}

impl OidcProfile {
    pub fn claim(&self, name: &str) -> Option<&Value> {
        self.claims.get(name)
    }

    fn text(&self, name: &str) -> Option<&str> {
        self.claims.get(name).and_then(Value::as_str)
    }

    pub fn name(&self) -> Option<&str> {
        self.text("name")
    }

    pub fn email(&self) -> Option<&str> {
        self.text("email")
    }

    pub fn email_verified(&self) -> Option<bool> {
        self.claims.get("email_verified").and_then(Value::as_bool)
    }

    pub fn picture(&self) -> Option<&str> {
        self.text("picture")
    }

    pub fn preferred_username(&self) -> Option<&str> {
        self.text("preferred_username")
    }
}

impl Serialize for OidcProfile {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.claims.serialize(serializer)
    }
}

/// Options for [oidc_provider].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OidcOptions {
    /// The provider name. Default `oidc`.
    pub name: String,
    pub issuer: String,
    pub client: ClientOptions,
    /// Where to read the discovery document. Default `<issuer>/.well-known/openid-configuration`.
    pub discovery_url: Option<String>,
    /// Fixed metadata instead of discovery.
    pub metadata: Option<OidcMetadata>,
}

impl OidcOptions {
    pub fn new(issuer: impl Into<String>, client: ClientOptions) -> OidcOptions {
        OidcOptions { name: "oidc".into(), issuer: issuer.into(), client, discovery_url: None, metadata: None }
    }
}

/// What [OidcProvider::map_profile]'s function receives (`mapProfile`'s input). Remix passes
/// the request context; here it is the callback URL, as in [OAuthProvider::handle_callback].
#[derive(Clone, Debug)]
pub struct OidcProfileInput {
    pub claims: OidcProfile,
    pub tokens: OAuthTokens,
    pub metadata: OidcMetadata,
    pub callback: Url,
}

type MapProfile<Pr> = Arc<dyn Fn(OidcProfileInput) -> Pin<Box<dyn Future<Output = Pr> + Send>> + Send + Sync>;

/// Any OpenID Connect provider (`createOIDCAuthProvider`): PKCE login, the userinfo endpoint
/// for the profile, and refresh. Metadata comes from discovery (cached after the first
/// success) unless given. Default scopes `openid profile email`. The profile is the userinfo
/// claims unless [OidcProvider::map_profile] maps them to another type `Pr`.
pub struct OidcProvider<Pr = OidcProfile> {
    options: OidcOptions,
    default_scopes: &'static [&'static str],
    fetch: Arc<dyn Fetch>,
    metadata: Mutex<Option<OidcMetadata>>,
    map: MapProfile<Pr>,
}

pub fn oidc_provider(options: OidcOptions, fetch: impl Fetch) -> OidcProvider {
    let claims: MapProfile<OidcProfile> = Arc::new(|input: OidcProfileInput| -> Pin<Box<dyn Future<Output = OidcProfile> + Send>> { Box::pin(std::future::ready(input.claims)) });
    OidcProvider { options, default_scopes: &["openid", "profile", "email"], fetch: Arc::new(fetch), metadata: Mutex::new(None), map: claims }
}

/// Google (`createGoogleAuthProvider`): OIDC with Google's endpoints. Default scopes
/// `openid email profile`.
pub fn google_provider(client: ClientOptions, fetch: impl Fetch) -> OidcProvider {
    const ISSUER: &str = "https://accounts.google.com";
    let metadata = OidcMetadata {
        issuer: ISSUER.into(),
        authorization_endpoint: "https://accounts.google.com/o/oauth2/v2/auth".into(),
        token_endpoint: "https://oauth2.googleapis.com/token".into(),
        userinfo_endpoint: Some("https://openidconnect.googleapis.com/v1/userinfo".into()),
        ..Default::default()
    };
    let options = OidcOptions { name: "google".into(), issuer: ISSUER.into(), client, discovery_url: None, metadata: Some(metadata) };
    OidcProvider { default_scopes: &["openid", "email", "profile"], ..oidc_provider(options, fetch) }
}

/// Options for [microsoft_provider].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MicrosoftOptions {
    pub client: ClientOptions,
    /// `common` (default), `organizations`, `consumers` or a tenant id.
    pub tenant: Option<String>,
}

/// Microsoft Entra ID (`createMicrosoftAuthProvider`).
pub fn microsoft_provider(options: MicrosoftOptions, fetch: impl Fetch) -> OidcProvider {
    let issuer = format!("https://login.microsoftonline.com/{}/v2.0", options.tenant.as_deref().unwrap_or("common"));
    oidc_provider(OidcOptions { name: "microsoft".into(), ..OidcOptions::new(issuer, options.client) }, fetch)
}

/// Okta (`createOktaAuthProvider`); `issuer` is the Okta authorization server.
pub fn okta_provider(issuer: impl Into<String>, client: ClientOptions, fetch: impl Fetch) -> OidcProvider {
    oidc_provider(OidcOptions { name: "okta".into(), ..OidcOptions::new(issuer, client) }, fetch)
}

/// Options for [auth0_provider].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Auth0Options {
    /// The tenant domain, such as `example.us.auth0.com` (a scheme is optional).
    pub domain: String,
    pub client: ClientOptions,
}

/// Auth0 (`createAuth0AuthProvider`).
pub fn auth0_provider(options: Auth0Options, fetch: impl Fetch) -> OidcProvider {
    let domain = options.domain.trim();
    let with_scheme = if domain.starts_with("http://") || domain.starts_with("https://") { domain.to_string() } else { format!("https://{domain}") };
    let issuer = match Url::parse(&with_scheme) {
        Ok(url) => format!("{}/", url.origin()),
        Err(_) => with_scheme,
    };
    oidc_provider(OidcOptions { name: "auth0".into(), ..OidcOptions::new(issuer, options.client) }, fetch)
}

impl<Pr> OidcProvider<Pr> {
    /// Map the userinfo claims to the app's profile type (`mapProfile`). The function also
    /// gets the tokens, the provider metadata and the callback URL.
    pub fn map_profile<Q, F, Fut>(self, map: F) -> OidcProvider<Q>
    where
        F: Fn(OidcProfileInput) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Q> + Send + 'static,
    {
        let OidcProvider { options, default_scopes, fetch, metadata, map: _ } = self;
        let map: MapProfile<Q> = Arc::new(move |input: OidcProfileInput| -> Pin<Box<dyn Future<Output = Q> + Send>> { Box::pin(map(input)) });
        OidcProvider { options, default_scopes, fetch, metadata, map }
    }

    async fn metadata(&self) -> Result<OidcMetadata, AuthError> {
        let name = &self.options.name;
        if let Some(metadata) = &self.options.metadata {
            return metadata.clone().validate(name);
        }
        if let Some(cached) = self.metadata.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(cached);
        }
        let url = match &self.options.discovery_url {
            Some(url) => url.clone(),
            None if self.options.issuer.ends_with('/') => format!("{}.well-known/openid-configuration", self.options.issuer),
            None => format!("{}/.well-known/openid-configuration", self.options.issuer),
        };
        let request = http::Request::get(url.as_str()).body(rmx_fetch::Body::empty()).map_err(|e| AuthError(e.to_string()))?;
        let json = fetch_json(&*self.fetch, request, &format!("Failed to load OIDC metadata for \"{name}\".")).await?;
        let metadata: OidcMetadata = decode(json, &format!("OIDC metadata for \"{name}\" did not include an issuer."))?;
        let metadata = metadata.validate(name)?;
        *self.metadata.lock().unwrap_or_else(|e| e.into_inner()) = Some(metadata.clone());
        Ok(metadata)
    }
}

impl<Pr: Send + 'static> OAuthProvider for OidcProvider<Pr> {
    type Profile = Pr;
    type Tokens = OAuthTokens;

    fn name(&self) -> &str {
        &self.options.name
    }

    async fn authorization_url(&self, transaction: &mut OAuthTransaction) -> Result<Url, AuthError> {
        let metadata = self.metadata().await?;
        self.options.client.authorize(&metadata.authorization_endpoint, self.default_scopes, true, transaction)
    }

    async fn handle_callback(&self, callback: &Url, transaction: &OAuthTransaction) -> Result<OAuthResult<Pr>, AuthError> {
        let metadata = self.metadata().await?;
        let name = self.name();
        let code = authorization_code(callback)?;
        let client = &self.options.client;
        let request = client.token_request(&metadata.token_endpoint, ClientAuthentication::RequestBody, &[]);
        let tokens = exchange_authorization_code(&*self.fetch, request, &client.redirect_uri, &code, &transaction.code_verifier).await?;
        let userinfo = metadata
            .userinfo_endpoint
            .as_deref()
            .filter(|u| !u.is_empty())
            .ok_or_else(|| AuthError(format!("OIDC provider \"{name}\" did not publish a userinfo_endpoint.")))?;
        let json = fetch_json(&*self.fetch, bearer_get(userinfo, &tokens.access_token, &[])?, &format!("Failed to load OIDC profile for \"{name}\".")).await?;
        let Value::Object(claims) = json else { return Err(AuthError(format!("OIDC provider \"{name}\" did not return a valid \"sub\" claim."))) };
        let sub = claims
            .get("sub")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AuthError(format!("OIDC provider \"{name}\" did not return a valid \"sub\" claim.")))?
            .to_string();
        let input = OidcProfileInput { claims: OidcProfile { sub: sub.clone(), claims }, tokens: tokens.clone(), metadata, callback: callback.clone() };
        let profile = (self.map)(input).await;
        Ok(OAuthResult { provider: name.to_string(), account: account(name, sub), profile, tokens })
    }

    async fn refresh_tokens(&self, tokens: &OAuthTokens) -> Result<OAuthTokens, AuthError> {
        let name = self.name();
        let refresh = tokens
            .refresh_token
            .as_deref()
            .filter(|t| !t.is_empty())
            .ok_or_else(|| AuthError(format!("OIDC provider \"{name}\" did not receive a refresh token.")))?;
        let metadata = self.metadata().await?;
        let request = self.options.client.token_request(&metadata.token_endpoint, ClientAuthentication::RequestBody, &[]);
        Ok(merge_refreshed_tokens(tokens, exchange_refresh_token(&*self.fetch, request, refresh, &[]).await?))
    }
}
