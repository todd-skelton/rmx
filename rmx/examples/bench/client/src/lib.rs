//! Browser entry: hydrate the client entries the server rendered.

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    rmx::hydrate(&[rmx::entry::<bench_app::Counter>(), rmx::entry::<bench_app::RowsBench>()]);
}
