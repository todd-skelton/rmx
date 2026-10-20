//! Parse form bodies once per request into a [`FormData`] in the request context.

use std::future::Future;
use std::sync::Arc;

use http::header;
use rmx_fetch::{File, FormData, FormLimits, FormValue};
use rmx_router::{BoxFuture, Context, Error, IntoResponse, Middleware, Next};

/// A file from a multipart body, before it reaches the form (`FileUpload`).
#[derive(Clone, Debug)]
pub struct FileUpload {
    /// The form field the file was sent under.
    pub field_name: String,
    pub file: File,
}

type UploadHandler = Arc<dyn Fn(FileUpload) -> BoxFuture<Option<FormValue>> + Send + Sync>;

/// The `formData()` middleware. Parses URL-encoded and multipart bodies.
/// Other requests (GET, HEAD, JSON and any other media type) get an empty `FormData` and
/// keep their body for the handler. Malformed bodies and bodies over the limits get a 400.
#[derive(Clone)]
pub struct FormDataParser {
    limits: FormLimits,
    upload_handler: Option<UploadHandler>,
}

pub fn form_data() -> FormDataParser {
    FormDataParser { limits: FormLimits::default(), upload_handler: None }
}

pub fn form_data_with(limits: FormLimits) -> FormDataParser {
    form_data().limits(limits)
}

impl FormDataParser {
    pub fn limits(mut self, limits: FormLimits) -> FormDataParser {
        self.limits = limits;
        self
    }

    /// Decide what each uploaded file becomes in the form (`uploadHandler`): store it and
    /// return a reference such as a URL, keep it as a file, or return `None` to drop it.
    /// Without a handler, files stay in the form as they are.
    ///
    /// ```
    /// use rmx_fetch::FormValue;
    /// use rmx_middleware::form_data::{form_data, FileUpload};
    ///
    /// let parser = form_data().upload_handler(|upload: FileUpload| async move {
    ///     // Save upload.file somewhere, then put its URL in the form instead.
    ///     Some(FormValue::Text(format!("/uploads/{}", upload.field_name)))
    /// });
    /// ```
    pub fn upload_handler<F, Fut>(mut self, handler: F) -> FormDataParser
    where
        F: Fn(FileUpload) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Option<FormValue>> + Send + 'static,
    {
        self.upload_handler = Some(Arc::new(move |upload| Box::pin(handler(upload))));
        self
    }
}

impl Middleware for FormDataParser {
    fn call(&self, mut ctx: Context, next: Next) -> BoxFuture<rmx_fetch::Response> {
        let (limits, handler) = (self.limits, self.upload_handler.clone());
        Box::pin(async move {
            let content_type = ctx.header(header::CONTENT_TYPE.as_str()).map(str::to_string);
            if !is_form_request(ctx.method(), content_type.as_deref()) {
                ctx.set(FormData::new());
                return next.run(ctx).await;
            }
            let body = ctx.take_body();
            let form = match FormData::parse(content_type.as_deref(), body, limits).await {
                Ok(form) => form,
                Err(e) => return Error::bad_request(format!("invalid form data: {e}")).into_response(),
            };
            let form = match handler {
                Some(handler) => handle_uploads(form, &handler).await,
                None => form,
            };
            ctx.set(form);
            next.run(ctx).await
        })
    }
}

async fn handle_uploads(form: FormData, handler: &UploadHandler) -> FormData {
    let mut out = FormData::new();
    for (name, value) in form.iter() {
        match value {
            FormValue::Text(text) => out.append(name, text.clone()),
            FormValue::File(file) => match handler(FileUpload { field_name: name.to_string(), file: file.clone() }).await {
                Some(FormValue::Text(text)) => out.append(name, text),
                Some(FormValue::File(file)) => out.append_file(name, file),
                None => {}
            },
        }
    }
    out
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
    use rmx_router::Router;

    use super::*;

    fn multipart() -> Request {
        let body = "--b\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nDune\r\n\
                    --b\r\nContent-Disposition: form-data; name=\"cover\"; filename=\"dune.png\"\r\nContent-Type: image/png\r\n\r\nPNG\r\n\
                    --b\r\nContent-Disposition: form-data; name=\"skip\"; filename=\"x.bin\"\r\n\r\nX\r\n--b--\r\n";
        http::Request::post("http://x/").header("content-type", "multipart/form-data; boundary=b").body(Body::from(body)).unwrap()
    }

    fn echo() -> impl rmx_router::Handler {
        |ctx: Context| async move {
            let form = ctx.form_data();
            let files: Vec<String> = form.iter().filter_map(|(k, v)| v.as_file().map(|f| format!("{k}={}", f.name))).collect();
            rmx_fetch::response::text(format!("{:?} {:?}", form.to_search_params().to_string(), files))
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
            rmx_fetch::response::text(format!("{fields} {body}"))
        });
        let req = http::Request::post("http://x/").header("content-type", "application/json").body(Body::from("{\"a\":1}")).unwrap();
        let text = block_on(block_on(router.fetch(req)).into_body().text()).unwrap();
        assert_eq!(text, r#"0 {"a":1}"#);
    }

    #[test]
    fn files_stay_files_without_a_handler() {
        let mut router = Router::new().with(form_data());
        router.post("/", echo());
        let text = block_on(block_on(router.fetch(multipart())).into_body().text()).unwrap();
        assert_eq!(text, r#""title=Dune&cover=dune.png&skip=x.bin" ["cover=dune.png", "skip=x.bin"]"#);
    }
}
