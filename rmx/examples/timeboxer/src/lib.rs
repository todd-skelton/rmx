//! The Remix timeboxer demo in rmx.
//!
//! What maps to what:
//!
//! | Remix | rmx |
//! | --- | --- |
//! | `csrf()` + `getCsrfToken(context)` | `csrf()` + `csrf_token(&ctx)` |
//! | `createSessionAuthScheme`, `createCredentialsAuthProvider` | `session_scheme`, `credentials_provider` |
//! | `node:crypto` scrypt | `rmx_crypto::scrypt` |
//! | `remix/data-schema` request parsing | [`server::schedules`] validators with the same messages |
//! | SQLite through `remix/data-table` | an in-memory `server::data::Db` with the same constraints |
//! | `createAssetServer` at `/assets/*path` | `rmx_assets::AssetServer` |
//! | client entries `ScheduleSidebar`, `NewScheduleAction`, `ScheduleGrid` | the same, compiled to wasm |
//! | `animateEntrance`, `animateExit`, `animateLayout`, `spring` from `@remix-run/ui/animation` | the same from `rmx_ui::animation` |

mod dom_util;
pub mod grid;
pub mod layout;
pub mod routes;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;
pub mod sidebar;
pub mod theme;

/// Browser entry: hydrate the client entries and adopt the frames.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    rmx::run(rmx::RunOptions {
        entries: vec![rmx::entry::<sidebar::ScheduleSidebar>(), rmx::entry::<grid::ScheduleGrid>()],
        ..Default::default()
    });
}
