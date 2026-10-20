use std::path::Path;
use std::sync::LazyLock;

use http::StatusCode;
use rmx::{rsx, Component, Handle, Node};
use rmx::assets::{AssetError, AssetOptions, AssetServer, FileTransform, GlobalTransform, Param, Transformed};
use rmx::fetch::{response, Response};
use rmx::middleware::logger::logger;
use rmx::render::Render;
use rmx::router::routes::{get, Route};
use rmx::router::{Context, Router};

pub fn main() {
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "44104".into());
    let root = std::env::var("ASSETS_ROOT").unwrap_or_else(|_| "examples/assets".into());
    let production = std::env::var("RMX_ENV").is_ok_and(|v| v == "production");
    let assets = asset_server(Path::new(&root), production).expect("asset server");
    let addr = format!("{host}:{port}");
    println!("assets demo is running on http://{addr}");
    println!("Edit a client file and refresh to verify the client code updates.");
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(rmx::serve::listen(&addr, router(assets))).expect("serve");
}

// app/routes.ts
pub const ASSETS_BASE: &str = "/assets";

pub struct Routes {
    pub home: Route,
    pub assets: Route,
}

pub static ROUTES: LazyLock<Routes> = LazyLock::new(|| Routes { home: get("/"), assets: get(&format!("{ASSETS_BASE}/*path")) });

// app/utils/assets.ts

/// The demo's asset server. `fingerprint` is on in production: URLs carry content hashes
/// and are cached forever; in development every request revalidates, so edits show up on
/// the next refresh.
pub fn asset_server(root: &Path, fingerprint: bool) -> Result<AssetServer, AssetError> {
    AssetServer::new(AssetOptions {
        root_dir: root.to_path_buf(),
        base_path: ASSETS_BASE.into(),
        mounts: vec![("app".into(), "app".into())],
        allow_files: vec!["app/**/public/**".into()],
        deny_files: vec!["app/**/*.test.*".into()],
        fingerprint,
        global_transforms: vec![GlobalTransform::new(&[".svg"], |bytes| {
            let svg = String::from_utf8(bytes).map_err(|_| "SVG files must be UTF-8".to_string())?;
            Ok(minify_svg(&svg).into())
        })],
        transforms: vec![("recolor".into(), FileTransform::new(&[".svg"], Param::Required, recolor))],
        ..Default::default()
    })
}

/// The `recolor` request transform: `image.svg?transform=recolor:8B5CF6` replaces
/// `currentColor` with the color.
fn recolor(bytes: Vec<u8>, param: Option<&str>) -> Result<Transformed, String> {
    let param = param.unwrap_or_default();
    let hex = param.strip_prefix('#').unwrap_or(param);
    if !matches!(hex.len(), 3 | 4 | 6 | 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Expected a hex color, with or without a leading #".into());
    }
    let svg = String::from_utf8(bytes).map_err(|_| "SVG files must be UTF-8".to_string())?;
    Ok(svg.replace("currentColor", &format!("#{hex}")).into())
}

/// A small stand-in for svgo: drop comments and collapse whitespace between and inside tags.
pub fn minify_svg(svg: &str) -> String {
    let mut without_comments = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(start) = rest.find("<!--") {
        without_comments.push_str(&rest[..start]);
        rest = rest[start..].find("-->").map_or("", |end| &rest[start + end + 3..]);
    }
    without_comments.push_str(rest);

    let mut out = String::with_capacity(without_comments.len());
    let mut pending_space = false;
    for c in without_comments.chars() {
        if c.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space {
            let prev = out.chars().last();
            let between_tags = prev == Some('>') && c == '<';
            let before_close = c == '>' || (c == '/' && prev != Some('<'));
            if prev.is_some() && !between_tags && !before_close {
                out.push(' ');
            }
            pending_space = false;
        }
        out.push(c);
    }
    out
}

pub fn router(assets: AssetServer) -> Router {
    let mut router = Router::new();
    if std::env::var("RMX_LOG").is_ok() {
        router.use_middleware(logger());
    }

    // app/actions/controller.ts
    let home_assets = assets.clone();
    router.route(&ROUTES.home, move |ctx: Context| {
        let assets = home_assets.clone();
        async move {
            match page_assets(&assets) {
                Ok(props) => ctx.render(Node::component::<HomePage>(None, props)),
                Err(e) => response::status(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}\nBuild the client first: ./build-client.ps1 -Crate assets-demo -Out examples/assets/app/actions/public/pkg")),
            }
        }
    });
    router.route(&ROUTES.assets, move |ctx: Context| {
        let assets = assets.clone();
        async move { assets.fetch(&ctx).unwrap_or_else(not_found) }
    });
    router
}

fn not_found() -> Response {
    response::status(StatusCode::NOT_FOUND, "Not found")
}

const ENTRY_FILE: &str = "app/actions/public/entry.js";
const STYLE_FILE: &str = "app/actions/public/styles/app.css";
const IMAGE_FILE: &str = "app/actions/public/images/image.svg";

fn page_assets(assets: &AssetServer) -> Result<HomePageProps, AssetError> {
    let entry = assets.script_entry(ENTRY_FILE)?;
    Ok(HomePageProps {
        script_href: entry.href,
        preloads: entry.preloads,
        style_href: assets.href(STYLE_FILE)?,
        image_href: assets.href(IMAGE_FILE)?,
        transformed_image_href: assets.href_with(IMAGE_FILE, &["recolor:#8B5CF6"])?,
    })
}

#[derive(Clone, Default)]
pub struct HomePageProps {
    pub script_href: String,
    pub preloads: Vec<String>,
    pub style_href: String,
    pub image_href: String,
    pub transformed_image_href: String,
}

pub struct HomePage;

impl Component for HomePage {
    type Props = HomePageProps;

    fn setup(_: &HomePageProps, _: &Handle<Self>) -> Self {
        HomePage
    }

    fn render(&self, props: &HomePageProps, _: &Handle<Self>) -> Node {
        let preloads: Vec<Node> = props.preloads.iter().map(|href| rsx! { <link rel="modulepreload" href={href.clone()}/> }).collect();
        rsx! {
            <html lang="en">
                <head>
                    <meta charset="utf-8"/>
                    <meta name="viewport" content="width=device-width, initial-scale=1"/>
                    <title>"rmx-assets demo"</title>
                    <link rel="stylesheet" href={props.style_href.clone()}/>
                    {preloads}
                </head>
                <body>
                    <main class="page-shell">
                        <h1>"rmx-assets watch mode"</h1>
                        <p>
                            "This server stays running while " <code>"rmx-assets"</code> " reads client files on request. "
                            "Edit a client module, refresh the page, and the updated code is served without restarting the server."
                        </p>
                        <p>
                            "This page serves a static SVG directly in HTML and also references the same SVG from CSS with "
                            <code>"url(...)"</code> "."
                        </p>
                        <p>
                            "It also requests a transformed SVG variant through the " <code>"transform"</code>
                            " query parameter to demonstrate request transforms and transformed-output caching."
                        </p>
                        <p>
                            "Try editing " <code>"app/actions/public/content.js"</code> ", "
                            <code>"app/actions/public/styles/app.css"</code> ", or "
                            <code>"app/actions/public/images/image.svg"</code>
                            ". You can also tweak the transforms in " <code>"src/server.rs"</code> "."
                        </p>
                        <div id="app-root" class="app" aria-live="polite">"Loading client app..."</div>
                        <div class="asset-previews" aria-label="Static asset previews">
                            <figure class="asset-preview">
                                <img class="asset-image" src={props.image_href.clone()} alt="Simple demo image" width="120" height="120"/>
                                <figcaption>"Direct HTML image"</figcaption>
                            </figure>
                            <figure class="asset-preview">
                                <img class="asset-image" src={props.transformed_image_href.clone()} alt="Purple transformed demo image" width="120" height="120"/>
                                <figcaption>"Request-transformed image"</figcaption>
                            </figure>
                            <figure class="asset-preview">
                                <div class="asset-background" aria-hidden="true"></div>
                                <figcaption>"CSS background image"</figcaption>
                            </figure>
                            <figure class="asset-preview">
                                <div class="asset-background asset-background-request-transformed" aria-hidden="true"></div>
                                <figcaption>"CSS request-transformed background image"</figcaption>
                            </figure>
                        </div>
                        <script type="module" src={props.script_href.clone()}></script>
                    </main>
                </body>
            </html>
        }
    }
}

#[cfg(test)]
mod tests;
