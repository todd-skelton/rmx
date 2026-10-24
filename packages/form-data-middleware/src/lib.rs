//! Parse form bodies once per request into a [`FormData`] in the request context.

use std::future::Future;
use std::sync::Arc;

use http::{header, StatusCode};
pub use rmx_fetch::form_data::FileUpload;
use rmx_fetch::{Body, BoxError, FormData, FormDataError, FormLimits, FormValue, UploadResult};
use rmx_fetch_router::{BoxFuture, Context, Error, IntoResponse, Middleware, Next};

type UploadHandler = Arc<dyn Fn(FileUpload) -> BoxFuture<Result<Option<FormValue>, BoxError>> + Send + Sync>;

/// The `formData()` middleware. Parses URL-encoded and multipart bodies, streaming
/// multipart files to the upload handler as they arrive. Other requests (GET, HEAD, JSON and
/// any other media type) get an empty `FormData` and keep their body for the handler. A
/// request that already has a `FormData` is left alone. Bodies over the limits get a 413,
/// malformed bodies a 400 and a failed upload handler a 500 (the last two become an empty
/// form with [`FormDataParser::suppress_errors`]). Remix throws these errors instead.
#[derive(Clone)]
pub struct FormDataParser {
    limits: FormLimits,
    upload_handler: Option<UploadHandler>,
    suppress_errors: bool,
}

pub fn form_data() -> FormDataParser {
    FormDataParser { limits: FormLimits::default(), upload_handler: None, suppress_errors: false }
}

pub fn form_data_with(limits: FormLimits) -> FormDataParser {
    form_data().limits(limits)
}

impl FormDataParser {
    pub fn limits(mut self, limits: FormLimits) -> FormDataParser {
        self.limits = limits;
        self
    }

    /// Treat a malformed body as an empty form instead of answering 400 (`suppressErrors`).
    /// Limit errors are never suppressed.
    pub fn suppress_errors(mut self, suppress: bool) -> FormDataParser {
        self.suppress_errors = suppress;
        self
    }

    /// Decide what each uploaded file becomes in the form (`uploadHandler`): store it and
    /// return a reference such as a URL, keep it as a file, or return `None` to drop it.
    /// Without a handler, files stay in the form as they are. The handler may also return a
    /// `Result`; an error stops the request with a 500.
    ///
    /// ```
    /// use rmx_fetch::FormValue;
    /// use rmx_form_data_middleware::{form_data, FileUpload};
    ///
    /// let parser = form_data().upload_handler(|upload: FileUpload| async move {
    ///     // Save upload.file somewhere, then put its URL in the form instead.
    ///     Some(FormValue::Text(format!("/uploads/{}", upload.field_name)))
    /// });
    /// ```
    pub fn upload_handler<F, Fut>(mut self, handler: F) -> FormDataParser
    where
        F: Fn(FileUpload) -> Fut + Send + Sync + 'static,
        Fut: Future + Send + 'static,
        Fut::Output: UploadResult,
    {
        self.upload_handler = Some(Arc::new(move |upload| {
            let fut = handler(upload);
            Box::pin(async move { fut.await.into_upload_result() })
        }));
        self
    }
}

impl Middleware for FormDataParser {
    fn call(&self, ctx: Context, next: Next) -> BoxFuture<rmx_fetch::Response> {
        let this = self.clone();
        Box::pin(parse_into_context(ctx, next, this.suppress_errors, move |content_type, body| async move {
            match &this.upload_handler {
                Some(handler) => FormData::parse_with(content_type.as_deref(), body, this.limits, |upload| handler(upload)).await,
                None => FormData::parse(content_type.as_deref(), body, this.limits).await,
            }
        }))
    }
}

/// Parse a form request's body with `parse` and put the form in the context, answering
/// limit errors with a 413, upload handler errors with a 500 and malformed bodies with a 400
/// (or an empty form, when `suppress_errors` is set).
async fn parse_into_context<F, Fut>(mut ctx: Context, next: Next, suppress_errors: bool, parse: F) -> rmx_fetch::Response
where
    F: FnOnce(Option<String>, Body) -> Fut,
    Fut: Future<Output = Result<FormData, FormDataError>>,
{
    if ctx.get::<FormData>().is_some() {
        return next.run(ctx).await;
    }
    let content_type = ctx.header(header::CONTENT_TYPE.as_str()).map(str::to_string);
    if !is_form_request(ctx.method(), content_type.as_deref()) {
        ctx.set(FormData::new());
        return next.run(ctx).await;
    }
    let body = ctx.take_body();
    let form = match parse(content_type, body).await {
        Ok(form) => form,
        Err(e) if e.is_limit() => return Error::new(StatusCode::PAYLOAD_TOO_LARGE, e.to_string()).into_response(),
        Err(_) if suppress_errors => FormData::new(),
        Err(FormDataError::Upload(e)) => return Error::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        Err(e) => return Error::bad_request(format!("invalid form data: {e}")).into_response(),
    };
    ctx.set(form);
    next.run(ctx).await
}

pub mod future;

/// `ctx.form_data()`: the parsed form (empty when the middleware did not run).
pub trait FormDataExt {
    fn form_data(&self) -> &FormData;
}

static EMPTY: std::sync::OnceLock<FormData> = std::sync::OnceLock::new();

impl FormDataExt for Context {
    fn form_data(&self) -> &FormData {
        self.get::<FormData>().unwrap_or_else(|| EMPTY.get_or_init(FormData::new))
    }
}

/// Only multipart and URL-encoded bodies are forms; GET and HEAD never carry one.
fn is_form_request(method: &http::Method, content_type: Option<&str>) -> bool {
    if method == http::Method::GET || method == http::Method::HEAD {
        return false;
    }
    let essence = content_type.unwrap_or("").split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    essence.starts_with("multipart/") || essence == "application/x-www-form-urlencoded"
}

#[cfg(test)]
mod tests {
    use rmx_fetch::{block_on, Body, Request};
    use rmx_fetch_router::Router;

    use super::*;

    fn multipart() -> Request {
        let body = "--b\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nDune\r\n\
                    --b\r\nContent-Disposition: form-data; name=\"cover\"; filename=\"dune.png\"\r\nContent-Type: image/png\r\n\r\nPNG\r\n\
                    --b\r\nContent-Disposition: form-data; name=\"skip\"; filename=\"x.bin\"\r\n\r\nX\r\n--b--\r\n";
        http::Request::post("http://x/").header("content-type", "multipart/form-data; boundary=b").body(Body::from(body)).unwrap()
    }

    fn echo() -> impl rmx_fetch_router::Handler {
        |ctx: Context| async move {
            let form = ctx.form_data();
            let files: Vec<String> = form.iter().filter_map(|(k, v)| v.as_file().map(|f| format!("{k}={}", f.name))).collect();
            rmx_response::text(format!("{:?} {:?}", form.to_search_params().to_string(), files))
        }
    }

    #[test]
    fn upload_handler_replaces_or_drops_files() {
        let mut router = Router::new().with(form_data().upload_handler(|upload: FileUpload| async move {
            (upload.field_name != "skip").then(|| FormValue::Text(format!("/uploads/{}/{}", upload.field_name, upload.file.name)))
        }));
        router.post("/", echo());
        let res = block_on(router.fetch(multipart()));
        let text = block_on(res.into_body().text()).unwrap();
        assert_eq!(text, r#""title=Dune&cover=%2Fuploads%2Fcover%2Fdune.png" []"#);
    }

    #[test]
    fn json_bodies_are_left_for_the_handler() {
        let mut router = Router::new().with(form_data());
        router.post("/", |mut ctx: Context| async move {
            let fields = ctx.form_data().iter().count();
            let body = ctx.take_body().text().await.unwrap();
            rmx_response::text(format!("{fields} {body}"))
        });
        let req = http::Request::post("http://x/").header("content-type", "application/json").body(Body::from("{\"a\":1}")).unwrap();
        let text = block_on(block_on(router.fetch(req)).into_body().text()).unwrap();
        assert_eq!(text, r#"0 {"a":1}"#);
    }

    fn post(content_type: &str, body: &'static str) -> Request {
        http::Request::post("http://x/").header("content-type", content_type).body(Body::from(body)).unwrap()
    }

    #[test]
    fn limits_and_malformed_bodies() {
        let mut router = Router::new().with(form_data_with(FormLimits { max_files: 1, ..Default::default() }));
        router.post("/", echo());
        let res = block_on(router.fetch(multipart()));
        assert_eq!(res.status(), 413);
        assert_eq!(block_on(res.into_body().text()).unwrap(), "Maximum number of files exceeded: 1");

        let mut router = Router::new().with(form_data_with(FormLimits { max_parts: 2, ..Default::default() }));
        router.post("/", echo());
        assert_eq!(block_on(router.fetch(post("application/x-www-form-urlencoded", "a=1&b=2&c=3"))).status(), 413);
        assert_eq!(block_on(router.fetch(post("application/x-www-form-urlencoded", "a=1&&b=2&"))).status(), 200, "empty pairs do not count");

        let malformed = || post("multipart/form-data; boundary=b", "--b\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n1");
        let mut strict = Router::new().with(form_data());
        strict.post("/", echo());
        assert_eq!(block_on(strict.fetch(malformed())).status(), 400);
        let mut lenient = Router::new().with(form_data().suppress_errors(true));
        lenient.post("/", echo());
        let res = block_on(lenient.fetch(malformed()));
        assert_eq!(res.status(), 200);
        assert_eq!(block_on(res.into_body().text()).unwrap(), r#""" []"#);
    }

    #[test]
    fn an_existing_form_is_reused() {
        fn parsed_twice(_: FileUpload) -> std::future::Ready<Option<FormValue>> {
            panic!("parsed twice")
        }
        let mut router = Router::new().with(form_data()).with(form_data().upload_handler(parsed_twice));
        router.post("/", echo());
        let text = block_on(block_on(router.fetch(multipart())).into_body().text()).unwrap();
        assert_eq!(text, r#""title=Dune&cover=dune.png&skip=x.bin" ["cover=dune.png", "skip=x.bin"]"#);
    }

    #[test]
    fn files_stay_files_without_a_handler() {
        let mut router = Router::new().with(form_data());
        router.post("/", echo());
        let text = block_on(block_on(router.fetch(multipart())).into_body().text()).unwrap();
        assert_eq!(text, r#""title=Dune&cover=dune.png&skip=x.bin" ["cover=dune.png", "skip=x.bin"]"#);
    }
}
