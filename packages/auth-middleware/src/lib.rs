//! Resolve who is making the request ([`auth`]) and require it on routes ([`require_auth`]).
//!
//! ```
//! use rmx_auth_middleware::{auth, require_auth, scheme, session_scheme};
//! use rmx_fetch_router::{middleware, Context, Router};
//!
//! #[derive(Clone)]
//! struct User { name: String }
//!
//! let load_auth = auth(vec![scheme(session_scheme(
//!     |session| session.get::<String>("user"),
//!     |name: String| async move { Some(User { name }) },
//! ))]);
//! let mut router = Router::new().with(load_auth);
//! router.group(vec![middleware(require_auth::<User>())], |r| {
//!     r.get("/account", |ctx: Context| async move {
//!         use rmx_auth_middleware::AuthExt;
//!         format!("hi {}", ctx.auth::<User>().identity().unwrap().name)
//!     });
//! });
//! ```

use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;

use http::header::{HeaderValue, WWW_AUTHENTICATE};
use http::StatusCode;
use rmx_fetch::Response;
use rmx_fetch_router::{BoxFuture, Context, IntoResponse, MaybeSend, MaybeSync, Middleware, Next};
use rmx_session::Session;

/// Why authentication failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureCode {
    MissingCredentials,
    InvalidCredentials,
}

/// A scheme that tried to authenticate the request and rejected it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthFailure {
    /// The scheme's name.
    pub method: String,
    pub code: FailureCode,
    pub message: String,
    /// Sent as `WWW-Authenticate` by [`require_auth`].
    pub challenge: Option<String>,
}

/// The request's auth state, stored in the context by [`auth`]. Read it with
/// [`AuthExt::auth`].
#[derive(Clone, Debug)]
pub enum Auth<I> {
    Authenticated { identity: I, method: String },
    /// No scheme succeeded; `error` is set when one rejected the credentials.
    Anonymous { error: Option<AuthFailure> },
}

impl<I> Auth<I> {
    pub fn is_ok(&self) -> bool {
        matches!(self, Auth::Authenticated { .. })
    }

    pub fn identity(&self) -> Option<&I> {
        match self {
            Auth::Authenticated { identity, .. } => Some(identity),
            Auth::Anonymous { .. } => None,
        }
    }

    /// The scheme that authenticated the request.
    pub fn method(&self) -> Option<&str> {
        match self {
            Auth::Authenticated { method, .. } => Some(method),
            Auth::Anonymous { .. } => None,
        }
    }

    pub fn error(&self) -> Option<&AuthFailure> {
        match self {
            Auth::Anonymous { error } => error.as_ref(),
            Auth::Authenticated { .. } => None,
        }
    }
}

/// What a scheme decided. Schemes return `None` to skip a request they do not apply to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemeResult<I> {
    Success(I),
    Failure { code: FailureCode, message: String, challenge: Option<String> },
}

impl<I> SchemeResult<I> {
    /// An `InvalidCredentials` failure.
    pub fn invalid(message: impl Into<String>) -> SchemeResult<I> {
        SchemeResult::Failure { code: FailureCode::InvalidCredentials, message: message.into(), challenge: None }
    }

    /// A failure with Remix's defaults (`{ status: 'failure' }`): `InvalidCredentials`,
    /// "Invalid credentials", no challenge.
    pub fn failure() -> SchemeResult<I> {
        SchemeResult::invalid("Invalid credentials")
    }
}

/// A way to authenticate requests. Read what you need from the context synchronously,
/// then verify it in the returned future.
pub trait AuthScheme<I>: MaybeSend + MaybeSync + 'static {
    /// Exposed as [`Auth::method`].
    fn name(&self) -> &str;
    fn authenticate(&self, ctx: &Context) -> BoxFuture<Option<SchemeResult<I>>>;
}

/// Box a scheme for [`auth`].
pub fn scheme<I>(s: impl AuthScheme<I>) -> Arc<dyn AuthScheme<I>> {
    Arc::new(s)
}

/// A scheme from a closure.
pub fn scheme_fn<I, F, Fut>(name: impl Into<String>, f: F) -> FnScheme<F>
where
    F: Fn(&Context) -> Fut + MaybeSend + MaybeSync + 'static,
    Fut: Future<Output = Option<SchemeResult<I>>> + MaybeSend + 'static,
{
    FnScheme { name: name.into(), f }
}

pub struct FnScheme<F> {
    name: String,
    f: F,
}

impl<I, F, Fut> AuthScheme<I> for FnScheme<F>
where
    F: Fn(&Context) -> Fut + MaybeSend + MaybeSync + 'static,
    Fut: Future<Output = Option<SchemeResult<I>>> + MaybeSend + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn authenticate(&self, ctx: &Context) -> BoxFuture<Option<SchemeResult<I>>> {
        Box::pin((self.f)(ctx))
    }
}

/// Runs each scheme in order and stores the first decision as [`Auth<I>`]: the first
/// success, or the first failure. When every scheme skips, the request is anonymous.
///
/// # Panics
///
/// When `schemes` is empty.
pub fn auth<I: Clone + Send + Sync + 'static>(schemes: Vec<Arc<dyn AuthScheme<I>>>) -> impl Middleware {
    assert!(!schemes.is_empty(), "auth() requires at least one authentication scheme");
    let schemes: Arc<[Arc<dyn AuthScheme<I>>]> = schemes.into();
    move |mut ctx: Context, next: Next| {
        let schemes = schemes.clone();
        async move {
            let mut state = Auth::Anonymous { error: None };
            for s in schemes.iter() {
                match s.authenticate(&ctx).await {
                    None => continue,
                    Some(SchemeResult::Success(identity)) => {
                        state = Auth::Authenticated { identity, method: s.name().to_string() };
                    }
                    Some(SchemeResult::Failure { code, message, challenge }) => {
                        let error = AuthFailure { method: s.name().to_string(), code, message, challenge };
                        state = Auth::Anonymous { error: Some(error) };
                    }
                }
                break;
            }
            ctx.set(state);
            next.run(ctx).await
        }
    }
}

type FailureFn<I> = dyn Fn(&Context, &Auth<I>) -> Response + Send + Sync;

/// Route middleware that lets authenticated requests through and answers the rest with
/// 401 (or [`RequireAuth::on_failure`]), adding the failing scheme's challenge as
/// `WWW-Authenticate`.
pub struct RequireAuth<I> {
    on_failure: Option<Arc<FailureFn<I>>>,
    identity: PhantomData<fn() -> I>,
}

pub fn require_auth<I: Clone + Send + Sync + 'static>() -> RequireAuth<I> {
    RequireAuth { on_failure: None, identity: PhantomData }
}

impl<I> RequireAuth<I> {
    /// The response for unauthenticated requests, for example a redirect to login.
    pub fn on_failure(mut self, respond: impl Fn(&Context, &Auth<I>) -> Response + Send + Sync + 'static) -> RequireAuth<I> {
        self.on_failure = Some(Arc::new(respond));
        self
    }
}

impl<I> Clone for RequireAuth<I> {
    fn clone(&self) -> Self {
        RequireAuth { on_failure: self.on_failure.clone(), identity: PhantomData }
    }
}

impl<I: Clone + Send + Sync + 'static> Middleware for RequireAuth<I> {
    fn call(&self, ctx: Context, next: Next) -> BoxFuture<Response> {
        let state = ctx
            .get::<Auth<I>>()
            .expect("Auth state not found. Make sure auth() middleware runs before requireAuth().");
        if state.is_ok() {
            return Box::pin(next.run(ctx));
        }
        let mut res = match &self.on_failure {
            Some(respond) => respond(&ctx, state),
            None => (StatusCode::UNAUTHORIZED, "Unauthorized").into_response(),
        };
        let challenge = state.error().and_then(|e| e.challenge.as_deref());
        if let (Some(challenge), false) = (challenge, res.headers().contains_key(WWW_AUTHENTICATE)) {
            if let Ok(v) = HeaderValue::from_str(challenge) {
                res.headers_mut().append(WWW_AUTHENTICATE, v);
            }
        }
        Box::pin(async move { res })
    }
}

/// `ctx.auth::<User>()`: the state stored by [`auth`].
pub trait AuthExt {
    /// # Panics
    ///
    /// When [`auth`] for identity type `I` did not run for this request.
    fn auth<I: Send + Sync + 'static>(&self) -> &Auth<I>;
}

impl AuthExt for Context {
    fn auth<I: Send + Sync + 'static>(&self) -> &Auth<I> {
        self.get::<Auth<I>>().expect("auth state not found: add the auth() middleware for this identity type")
    }
}

/// Rotate the session id after a successful login (dropping the old session's stored
/// data) and return the session to write the login into. Remix's `completeAuth`.
pub fn complete_auth(ctx: &Context) -> Session {
    let session = ctx.get::<Session>().expect("Session not found. Make sure session() middleware runs before completeAuth().");
    session.regenerate_id(true);
    session.clone()
}

// ---------------------------------------------------------------------------------------
// Credentials (remix/auth's createCredentialsAuthProvider and verifyCredentials)
// ---------------------------------------------------------------------------------------

/// Direct form-based login: `parse` reads the submitted credentials from the request and
/// `verify` checks them (with the request context at hand), yielding who logged in or `None`
/// when they are rejected. A `verify` that can fail returns `Result<Option<_>, E>`, which
/// [verify_credentials] hands back unchanged.
///
/// ```
/// use rmx_auth_middleware::{credentials_provider, verify_credentials};
/// use rmx_form_data_middleware::FormDataExt;
///
/// let password = credentials_provider(
///     |ctx| (ctx.form_data().get("email").unwrap_or_default().trim().to_lowercase(), ctx.form_data().get("password").unwrap_or_default().to_string()),
///     |(email, password): (String, String), _ctx| async move { (email == "a@b.c" && password == "secret").then_some(1_u64) },
/// );
/// assert_eq!(password.name(), "password");
/// // In a handler: let Some(user_id) = verify_credentials(&password, &ctx).await else { .. };
/// ```
pub struct CredentialsProvider<P, V> {
    name: String,
    parse: P,
    verify: V,
}

pub fn credentials_provider<In, P, V, Fut>(parse: P, verify: V) -> CredentialsProvider<P, V>
where
    P: Fn(&Context) -> In,
    V: Fn(In, &Context) -> Fut,
    Fut: Future,
{
    CredentialsProvider { name: "password".into(), parse, verify }
}

impl<P, V> CredentialsProvider<P, V> {
    /// Rename the provider (default `"password"`).
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The hook that reads the submitted credentials.
    pub fn parse(&self) -> &P {
        &self.parse
    }

    /// The hook that checks them.
    pub fn verify(&self) -> &V {
        &self.verify
    }
}

/// Parse and verify the submitted credentials: whatever `verify` returns, such as `None`
/// when they are rejected or an error it raised.
pub fn verify_credentials<In, P, V, Fut>(provider: &CredentialsProvider<P, V>, ctx: &Context) -> Fut
where
    P: Fn(&Context) -> In,
    V: Fn(In, &Context) -> Fut,
    Fut: Future,
{
    (provider.verify)((provider.parse)(ctx), ctx)
}

// ---------------------------------------------------------------------------------------
// Built-in schemes
// ---------------------------------------------------------------------------------------

type InvalidateFn = dyn Fn(&Session) + Send + Sync;

/// Authenticate from a value saved in the session at login.
pub struct SessionScheme<R, V> {
    name: String,
    read: R,
    verify: V,
    invalidate: Option<Arc<InvalidateFn>>,
    code: FailureCode,
    message: String,
}

/// `read` pulls the login value out of the session (`None` skips the scheme); `verify`
/// turns it into an identity (`None` rejects it, then `invalidate` runs).
pub fn session_scheme<T, I, R, V, Fut>(read: R, verify: V) -> SessionScheme<R, V>
where
    R: Fn(&Session) -> Option<T> + Send + Sync + 'static,
    V: Fn(T) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Option<I>> + MaybeSend + 'static,
{
    SessionScheme {
        name: "session".into(),
        read,
        verify,
        invalidate: None,
        code: FailureCode::InvalidCredentials,
        message: "Invalid session".into(),
    }
}

impl<R, V> SessionScheme<R, V> {
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Clear stale login data when verification fails.
    pub fn invalidate(mut self, f: impl Fn(&Session) + Send + Sync + 'static) -> Self {
        self.invalidate = Some(Arc::new(f));
        self
    }

    pub fn failure(mut self, code: FailureCode, message: impl Into<String>) -> Self {
        self.code = code;
        self.message = message.into();
        self
    }
}

impl<T, I, R, V, Fut> AuthScheme<I> for SessionScheme<R, V>
where
    T: 'static,
    I: MaybeSend + 'static,
    R: Fn(&Session) -> Option<T> + Send + Sync + 'static,
    V: Fn(T) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Option<I>> + MaybeSend + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn authenticate(&self, ctx: &Context) -> BoxFuture<Option<SchemeResult<I>>> {
        let session = ctx
            .get::<Session>()
            .expect("Session not found. Make sure session() middleware runs before createSessionAuthScheme().")
            .clone();
        let Some(value) = (self.read)(&session) else {
            return Box::pin(async { None });
        };
        let verified = (self.verify)(value);
        let (code, message) = (self.code, self.message.clone());
        let invalidate = self.invalidate.clone();
        Box::pin(async move {
            match verified.await {
                Some(identity) => Some(SchemeResult::Success(identity)),
                None => {
                    if let Some(f) = invalidate {
                        f(&session);
                    }
                    Some(SchemeResult::Failure { code, message, challenge: None })
                }
            }
        })
    }
}

/// Authenticate `Authorization: Bearer <token>` headers.
pub struct BearerScheme<V> {
    name: String,
    header_name: String,
    scheme: String,
    challenge: Option<String>,
    verify: V,
}

/// `verify` turns a token into an identity (`None` rejects it).
pub fn bearer_scheme<I, V, Fut>(verify: V) -> BearerScheme<V>
where
    V: Fn(String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Option<I>> + MaybeSend + 'static,
{
    BearerScheme { name: "bearer".into(), header_name: "Authorization".into(), scheme: "Bearer".into(), challenge: None, verify }
}

impl<V> BearerScheme<V> {
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn header_name(mut self, header: impl Into<String>) -> Self {
        self.header_name = header.into();
        self
    }

    /// The expected prefix (default `Bearer`, compared case-insensitively).
    pub fn scheme(mut self, scheme: impl Into<String>) -> Self {
        self.scheme = scheme.into();
        self
    }

    /// The `WWW-Authenticate` value on failure (default: the scheme prefix).
    pub fn challenge(mut self, challenge: impl Into<String>) -> Self {
        self.challenge = Some(challenge.into());
        self
    }
}

impl<I, V, Fut> AuthScheme<I> for BearerScheme<V>
where
    I: MaybeSend + 'static,
    V: Fn(String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Option<I>> + MaybeSend + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn authenticate(&self, ctx: &Context) -> BoxFuture<Option<SchemeResult<I>>> {
        let challenge = Some(self.challenge.clone().unwrap_or_else(|| self.scheme.clone()));
        let fail = |code, message: String| Some(SchemeResult::Failure { code, message, challenge: challenge.clone() });
        // Fetch's `Headers` strips surrounding whitespace, so Remix never sees it.
        let Some(value) = ctx.header(&self.header_name).map(|v| v.trim_matches([' ', '\t'])) else {
            return Box::pin(async { None });
        };
        let Some((prefix, token)) = value.split_once(char::is_whitespace).filter(|(p, t)| !p.is_empty() && !t.is_empty())
        else {
            let res = fail(FailureCode::InvalidCredentials, format!("{} header is malformed", self.header_name));
            return Box::pin(async { res });
        };
        if !prefix.eq_ignore_ascii_case(&self.scheme) {
            return Box::pin(async { None });
        }
        if token.trim().is_empty() {
            let res = fail(FailureCode::MissingCredentials, format!("{} token is missing", self.scheme));
            return Box::pin(async { res });
        }
        let verified = (self.verify)(token.trim().to_string());
        let rejected = fail(FailureCode::InvalidCredentials, "Invalid credentials".into());
        Box::pin(async move { verified.await.map(SchemeResult::Success).or(rejected) })
    }
}

/// Authenticate an API key header (default `X-Api-Key`).
pub struct ApiKeyScheme<V> {
    name: String,
    header_name: String,
    verify: V,
}

pub fn api_key_scheme<I, V, Fut>(verify: V) -> ApiKeyScheme<V>
where
    V: Fn(String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Option<I>> + MaybeSend + 'static,
{
    ApiKeyScheme { name: "api-key".into(), header_name: "X-Api-Key".into(), verify }
}

impl<V> ApiKeyScheme<V> {
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn header_name(mut self, header: impl Into<String>) -> Self {
        self.header_name = header.into();
        self
    }
}

impl<I, V, Fut> AuthScheme<I> for ApiKeyScheme<V>
where
    I: MaybeSend + 'static,
    V: Fn(String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Option<I>> + MaybeSend + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn authenticate(&self, ctx: &Context) -> BoxFuture<Option<SchemeResult<I>>> {
        let Some(key) = ctx.header(&self.header_name).map(str::trim) else {
            return Box::pin(async { None });
        };
        if key.is_empty() {
            let message = format!("{} header is empty", self.header_name);
            return Box::pin(async move {
                Some(SchemeResult::Failure { code: FailureCode::MissingCredentials, message, challenge: None })
            });
        }
        let verified = (self.verify)(key.to_string());
        Box::pin(async move { Some(verified.await.map(SchemeResult::Success).unwrap_or_else(SchemeResult::failure)) })
    }
}

#[cfg(test)]
mod tests;
