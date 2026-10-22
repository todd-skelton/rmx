//! `routes.ts`, `router.ts`, `middleware/render.ts` and `actions/controller.ts`.

use std::sync::LazyLock;

use rmx::fetch::Response;
use rmx::response;
use rmx::file_storage::FileStorage;
use rmx::middleware::render::{render_with, Renderer, RendererExt, ResponseInit};
use rmx::router::routes::{get, Route, RouteMap};
use rmx::router::{action, Context, Router};

use crate::npm::{files_at_path, is_fully_resolved_version, parse_package_path, resolve_version, FileKind, Npm, NpmError};
use crate::package_browser::{render_directory_listing, render_error, render_file_content};
use crate::ui::{create_document, home_page, HtmlDocument};

#[derive(RouteMap)]
pub struct Routes {
    pub home: Route,
    pub package_browser: Route,
}

pub static ROUTES: LazyLock<Routes> = LazyLock::new(|| Routes { home: get("/"), package_browser: get("/*path") });

pub fn router<C: FileStorage + Clone>(npm: Npm<C>) -> Router {
    let mut router = Router::new();
    // middleware/render.ts: renderWith() with a document renderer.
    router.use_middleware(render_with(|_ctx: &Context| {
        |document: HtmlDocument, init: ResponseInit| init.apply(response::html(create_document(&document.title, &document.content).into_string()))
    }));
    router.map(
        &*ROUTES,
        RoutesActions {
            home: action(|ctx: Context| async move {
                ctx.renderer::<HtmlDocument>().render(HtmlDocument { title: "UNPKG - npm package browser".into(), content: home_page() })
            }),
            package_browser: action(move |ctx: Context| {
                let npm = npm.clone();
                async move { package_browser(ctx, npm).await }
            }),
        },
    );
    router
}

async fn package_browser<C: FileStorage>(ctx: Context, npm: Npm<C>) -> Response {
    let path = ctx.param("path").unwrap_or_default().to_string();
    if path.is_empty() {
        return response::redirect("/");
    }
    // Owned, so the future does not borrow the (non-Sync) context across awaits.
    let render = ctx.renderer::<HtmlDocument>().clone();
    let render = &render;
    match browse(render, &npm, &path).await {
        Ok(res) => res,
        Err(NpmError::PackageNotFound(name)) => render_error(render, "Package not found", &format!("The package \"{name}\" was not found on npm.")),
        Err(NpmError::VersionNotFound { package_name, version }) => {
            render_error(render, "Version not found", &format!("Version \"{version}\" of package \"{package_name}\" was not found."))
        }
        Err(NpmError::InvalidPath(path)) => render_error(render, "Invalid path", &format!("The path \"{path}\" is not valid.")),
        Err(NpmError::Other(message)) => {
            eprintln!("unpkg: {message}");
            response::status(http::StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error")
        }
    }
}

async fn browse<C: FileStorage>(render: &Renderer<HtmlDocument>, npm: &Npm<C>, path: &str) -> Result<Response, NpmError> {
    let parsed = parse_package_path(path)?;
    let metadata = npm.metadata(&parsed.name).await?;
    if !is_fully_resolved_version(&metadata, &parsed.version) {
        let resolved = resolve_version(&metadata, &parsed.version)?;
        let file = if parsed.file_path.is_empty() { String::new() } else { format!("/{}", parsed.file_path) };
        return Ok(response::redirect(&format!("/{}@{resolved}{file}", parsed.name)));
    }

    let contents = npm.contents(&metadata, &parsed.version).await?;
    let version = &contents.metadata.version;
    let file = contents.files.get(&parsed.file_path);
    if let Some(file) = file.filter(|f| !parsed.file_path.is_empty() && f.kind == FileKind::File) {
        return Ok(match contents.file_content(&parsed.file_path) {
            Some(data) => render_file_content(render, &parsed.name, version, &parsed.file_path, file, data),
            None => render_error(render, "File not found", &format!("The file \"{}\" was not found in the package.", parsed.file_path)),
        });
    }
    let files = files_at_path(&contents.files, &parsed.file_path);
    Ok(render_directory_listing(render, &parsed.name, version, &parsed.file_path, &files))
}

#[cfg(test)]
mod tests;
