//! Browser entry: start the rmx runtime with the app's client entries.
#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    rmx::run(rmx::RunOptions { entries: vec![rmx::entry::<bench_app::Counter>(), rmx::entry::<bench_app::RowsBench>()], ..Default::default() });
}
