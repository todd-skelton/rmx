//! The `formData()` middleware with streaming uploads (Remix's `form-data-middleware/future`).
//!
//! The same middleware as the crate root, parsing with [`rsmix_fetch::form_data::future`]: the
//! upload handler gets each [FileUpload] as soon as its headers arrive and reads its content
//! from the request as it streams in, so large uploads never sit in memory. Without a
//! handler, files are read into the form as they are.
//!
//! ```
//! use rsmix_form_data_middleware::future::{form_data, FileUpload};
//!
//! let parser = form_data().upload_handler(|mut upload: FileUpload| async move {
//!     let mut size = 0;
//!     while let Some(chunk) = upload.chunk().await {
//!         size += chunk?.len(); // write the chunk to storage here
//!     }
//!     Ok::<_, rsmix_fetch::FormDataError>(format!("/uploads/{}?size={size}", upload.field_name))
//! });
//! ```

use std::future::Future;
use std::sync::Arc;

use rsmix_fetch::form_data::future::{parse, parse_with};
pub use rsmix_fetch::form_data::future::{FileUpload, UploadOutput, UploadValue};
use rsmix_fetch::{BoxError, FormLimits};
use rsmix_fetch_router::{BoxFuture, Context, Middleware, Next};

use crate::parse_into_context;

type UploadHandler = Arc<dyn Fn(FileUpload) -> BoxFuture<Result<Option<UploadValue>, BoxError>> + Send + Sync>;

/// The streaming `formData()` middleware. Answers like [`crate::FormDataParser`]: 413 for
/// limits (even when errors are suppressed), 400 for malformed bodies and 500 when the upload
/// handler fails.
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

    /// Handle each uploaded file as it streams in (`uploadHandler`); see [UploadOutput]
    /// for what the handler may return.
    pub fn upload_handler<F, Fut>(mut self, handler: F) -> FormDataParser
    where
        F: Fn(FileUpload) -> Fut + Send + Sync + 'static,
        Fut: Future + Send + 'static,
        Fut::Output: UploadOutput,
    {
        self.upload_handler = Some(Arc::new(move |upload| {
            let fut = handler(upload);
            Box::pin(async move { fut.await.into_upload_output() })
        }));
        self
    }
}

impl Middleware for FormDataParser {
    fn call(&self, ctx: Context, next: Next) -> BoxFuture<rsmix_fetch::Response> {
        let this = self.clone();
        Box::pin(parse_into_context(ctx, next, this.suppress_errors, move |content_type, body| async move {
            match &this.upload_handler {
                Some(handler) => parse_with(content_type.as_deref(), body, this.limits, |upload| handler(upload)).await,
                None => parse(content_type.as_deref(), body, this.limits).await,
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use rsmix_fetch::{block_on, Body, Request};
    use rsmix_fetch_router::Router;

    use super::*;
    use crate::FormDataExt;

    fn request(body: &'static str) -> Request {
        http::Request::post("http://x/").header("content-type", "multipart/form-data; boundary=b").body(Body::from(body)).unwrap()
    }

    const UPLOAD: &str = "--b\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nDune\r\n\
                          --b\r\nContent-Disposition: form-data; name=\"cover\"; filename=\"dune.png\"\r\nContent-Type: image/png\r\n\r\nPNG\r\n--b--\r\n";

    fn router(parser: FormDataParser) -> Router {
        let mut router = Router::new().with(parser);
        router.post("/", |ctx: Context| async move {
            let form = ctx.form_data();
            let cover = match form.get_value("cover") {
                Some(rsmix_fetch::FormValue::File(f)) => format!("file {} {}", f.name, f.bytes.len()),
                Some(rsmix_fetch::FormValue::Text(t)) => t.clone(),
                None => "none".into(),
            };
            rsmix_response::text(format!("{} {cover}", form.get("title").unwrap_or("-")))
        });
        router
    }

    fn text(router: &Router, request: Request) -> (u16, String) {
        block_on(async {
            let response = router.fetch(request).await;
            (response.status().as_u16(), response.into_body().text().await.unwrap())
        })
    }

    #[test]
    fn streams_uploads_to_the_handler_and_keeps_files_without_one() {
        let streamed = router(form_data().upload_handler(|mut upload: FileUpload| async move {
            let mut size = 0;
            while let Some(chunk) = upload.chunk().await {
                size += chunk?.len();
            }
            Ok::<_, rsmix_fetch::FormDataError>(format!("stored {} {size}", upload.name))
        }));
        assert_eq!(text(&streamed, request(UPLOAD)), (200, "Dune stored dune.png 3".into()));
        assert_eq!(text(&router(form_data()), request(UPLOAD)), (200, "Dune file dune.png 3".into()));
    }

    #[test]
    fn answers_limits_with_413_and_handler_errors_with_500() {
        let small = router(form_data_with(FormLimits { max_file_size: 2, ..Default::default() }).suppress_errors(true));
        assert_eq!(text(&small, request(UPLOAD)).0, 413);
        let failing = router(form_data().upload_handler(|_upload: FileUpload| async { Err::<(), _>(std::io::Error::other("disk full")) }));
        assert_eq!(text(&failing, request(UPLOAD)).0, 500);
        let malformed = router(form_data());
        assert_eq!(text(&malformed, request("--b\r\nnot a part")).0, 400);
    }
}
