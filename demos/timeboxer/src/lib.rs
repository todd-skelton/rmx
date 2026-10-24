//! The Remix timeboxer demo in rsmix.
//!
//! What maps to what:
//!
//! | Remix | rsmix |
//! | --- | --- |
//! | `csrf()` + `getCsrfToken(context)` | `csrf()` + `csrf_token(&ctx)` |
//! | `createSessionAuthScheme`, `createCredentialsAuthProvider` | `session_scheme`, `credentials_provider` |
//! | `node:crypto` scrypt | `rsmix::crypto::scrypt` |
//! | `remix/data-schema` request parsing | [`server::schedules`] validators with the same messages |
//! | SQLite through `remix/data-table` | an in-memory `server::data::Db` with the same constraints |
//! | `createAssetServer` at `/assets/*path` | `rsmix::assets::AssetServer` |
//! | client entries `ScheduleSidebar`, `NewScheduleAction`, `ScheduleGrid` | the same, compiled to wasm |
//! | `animateEntrance`, `animateExit`, `animateLayout`, `spring` from `@remix-run/ui/animation` | the same from `rsmix::ui::animation` |

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
    rsmix::run(rsmix::RunOptions {
        entries: vec![rsmix::entry::<sidebar::ScheduleSidebar>(), rsmix::entry::<grid::ScheduleGrid>()],
        ..Default::default()
    });
}
