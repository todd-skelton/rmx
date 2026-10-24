//! Server rendering of three pages: the table and the prose page are built once and rendered
//! (escaping happens while rendering), and the price list is built and rendered each time
//! (floats become text when the tree is built): the bench's 1,000-row table (short labels and
//! ids), a page of prose paragraphs, and a price list (floats in text and attributes). Prints
//! the fastest of 15 runs of 200 ms, in microseconds per page, which holds up better than the
//! median on a busy machine.
//!
//! `cargo run --release -p rmx-component --example ssr_bench`

extern crate rmx_component as rmx;

use std::hint::black_box;
use std::time::Instant;

use rmx::{render_to_string, rsx, Node};

const WORDS: [&str; 12] = ["pretty", "large", "red", "blue", "table", "chair", "house", "cheap", "fancy", "pony", "desk", "odd"];

fn table(rows: usize) -> Node {
    let mut seed = 1u32;
    let mut next = move || {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (seed >> 16) as usize
    };
    let rows: Vec<Node> = (1..=rows as u32)
        .map(|id| {
            let label = format!("{} {} {}", WORDS[next() % 12], WORDS[next() % 12], WORDS[next() % 12]);
            rsx! {
                <tr class={(id % 10 == 0).then_some("done")}>
                    <td class="id">{id}</td>
                    <td><a href={format!("/rows/{id}")}>{label}</a></td>
                </tr>
            }
        })
        .collect();
    rsx! { <table><tbody>{rows}</tbody></table> }
}

fn prose(paragraphs: usize) -> Node {
    let text = "Rewriting it in Rust is fashionable, and this paragraph is the plain product copy a page carries: a few sentences of text, a comma or two, and nothing that needs escaping until the very end, where Tom & Jerry appear. ";
    let items: Vec<Node> = (0..paragraphs).map(|i| rsx! { <p title={format!("Paragraph {i} of the article")}>{text.repeat(2)}</p> }).collect();
    rsx! { <article>{items}</article> }
}

fn prices(rows: usize) -> Node {
    let items: Vec<Node> = (0..rows)
        .map(|i| {
            let price = (i * 37 % 10_000) as f64 / 100.0 + 0.49;
            rsx! { <li data-price={price}>"Card "{i as u32}": $"{price}</li> }
        })
        .collect();
    rsx! { <ul>{items}</ul> }
}

fn bench(name: &str, run: impl Fn() -> String) {
    let bytes = run().len();
    let mut best = f64::MAX;
    for _ in 0..15 {
        let start = Instant::now();
        let mut n = 0;
        while start.elapsed().as_millis() < 200 {
            black_box(run());
            n += 1;
        }
        best = best.min(start.elapsed().as_secs_f64() * 1e6 / n as f64);
    }
    println!("{name:<36} {bytes:>8} B  {best:>9.1} us");
}

fn main() {
    let rows = table(1000);
    let article = prose(200);
    bench("table, 1,000 rows (render)", || render_to_string(black_box(&rows)));
    bench("prose, 200 paragraphs (render)", || render_to_string(black_box(&article)));
    bench("prices, 1,000 items (build + render)", || render_to_string(&prices(1000)));
}
