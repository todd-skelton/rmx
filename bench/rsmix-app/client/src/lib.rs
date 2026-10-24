//! Browser entry: start the rsmix runtime with the app's client entries.
#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    rsmix::run(rsmix::RunOptions { entries: vec![rsmix::entry::<bench_app::Counter>(), rsmix::entry::<bench_app::RowsBench>()], ..Default::default() });
}
