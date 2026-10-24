//! The `remix new` starter app in rsmix (`reference/remix/template`): a home page rendered on
//! the server, with one interactive client entry, the copy-to-clipboard `PromptButton`.
//!
//! | Remix | rsmix |
//! | --- | --- |
//! | `app/routes.ts` | [`routes`] |
//! | `app/router.ts`, `app/actions/controller.tsx` | `server::router` with `RoutesActions` |
//! | `app/assets.ts` (`createAssetServer`) | `rsmix::assets::AssetServer` |
//! | `app/actions/home-page.tsx`, `document.tsx` | `server::home_page` |
//! | `app/actions/public/prompt-button.tsx` (`clientEntry`) | [`prompt_button::PromptButton`] |
//! | `app/actions/public/entry.ts` | `app/actions/public/entry.js` plus [`start`] |
//! | `app/actions/controller.test.ts` | `server::tests` |

pub mod prompt_button;
pub mod routes;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;

/// Browser entry: hydrate the client entries.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    rsmix::run(rsmix::RunOptions { entries: vec![rsmix::entry::<prompt_button::PromptButton>()], ..Default::default() });
}
