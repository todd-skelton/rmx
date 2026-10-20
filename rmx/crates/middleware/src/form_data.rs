//! Parse form bodies once per request into a [`FormData`] in the request context.

use http::header;
use rmx_fetch::{FormData, FormLimits};
use rmx_router::{Context, Error, IntoResponse, Middleware, Next};

/// Parses URL-encoded, multipart and text/plain bodies. Requests without a form body
/// (including GET and HEAD) get an empty `FormData`. Malformed bodies get a 400.
pub fn form_data() -> impl Middleware {
    form_data_with(FormLimits::default())
}

pub fn form_data_with(limits: FormLimits) -> impl Middleware {
    move |mut ctx: Context, next: Next| async move {
        let content_type = ctx.header(header::CONTENT_TYPE.as_str()).map(str::to_string);
        let body = ctx.take_body();
        match FormData::parse(content_type.as_deref(), body, limits).await {
            Ok(form) => {
                ctx.set(form);
                next.run(ctx).await
            }
            Err(e) => Error::bad_request(format!("invalid form data: {e}")).into_response(),
        }
    }
}

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
