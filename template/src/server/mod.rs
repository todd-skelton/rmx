//! `app/router.ts`, `app/assets.ts` and `app/actions/controller.tsx`.

pub mod home_page;
#[cfg(test)]
mod tests;

use std::path::Path;

use http::StatusCode;
use rsmix::assets::{AssetError, AssetOptions, AssetServer};
use rsmix::response;
use rsmix::middleware::live_reload::live_reload;
use rsmix::middleware::static_files::{static_files, StaticOptions};
use rsmix::middleware::render::Render;
use rsmix::router::{action, Context, Router};

use crate::routes::{RoutesActions, ROUTES};

/// The browser entry, served through the asset server.
pub const ENTRY_FILE: &str = "app/actions/public/entry.js";

/// `app/assets.ts`: `/assets` serves `app/**/public/**` (never tests); production
/// fingerprints the URLs.
pub fn asset_server(root: &Path, production: bool) -> Result<AssetServer, AssetError> {
    AssetServer::new(AssetOptions {
        root_dir: root.to_path_buf(),
        base_path: "/assets".into(),
        mounts: vec![("app".into(), "app".into())],
        allow_files: vec!["app/**/public/**".into()],
        deny_files: vec!["app/**/*.test.*".into()],
        fingerprint: production,
        ..Default::default()
    })
}

/// `app/router.ts`: root `public/` files served unchanged, then the root controller. Under
/// `rsmix dev`, open pages reload when the server restarts (`live_reload`).
pub fn router(root: &Path, production: bool) -> Result<Router, AssetError> {
    let assets = asset_server(root, production)?;
    let mut router = Router::new();
    router.use_middleware(live_reload());
    router.use_middleware(static_files(root.join("public"), StaticOptions { index: vec![], ..Default::default() }));

    // app/actions/controller.tsx
    let home_assets = assets.clone();
    router.map(
        &*ROUTES,
        RoutesActions {
            assets: action(move |ctx: Context| {
                let assets = assets.clone();
                async move { assets.fetch(&ctx).unwrap_or_else(|| response::status(StatusCode::NOT_FOUND, "Not Found")) }
            }),
            home: action(move |ctx: Context| {
                let script = home_assets.script_entry(ENTRY_FILE).ok();
                async move { ctx.render(home_page::home_page(script.as_ref())) }
            }),
        },
    );
    Ok(router)
}
