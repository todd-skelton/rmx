//! Browser entry: hydrate the client entries the server rendered.
#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    rmx::hydrate(&[rmx::entry::<bench_app::Counter>(), rmx::entry::<bench_app::RowsBench>()]);
}
