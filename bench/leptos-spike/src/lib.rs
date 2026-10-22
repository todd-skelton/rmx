pub mod app;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    #[cfg(feature = "islands")]
    leptos::mount::hydrate_islands();
    #[cfg(not(feature = "islands"))]
    leptos::mount::hydrate_body(app::App);
}
