//! The benchmark app. Every page and component here has a line-for-line counterpart in
//! `bench/remix-app` so the two frameworks render the same HTML and do the same work.

use rsmix::{browser, rsx, Children, Component, EventHandler, Handle, Node, Str};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------------------
// Data shared by both frameworks: the same word lists and generator as bench/remix-app.
// ---------------------------------------------------------------------------------------

const ADJECTIVES: [&str; 25] = [
    "pretty", "large", "big", "small", "tall", "short", "long", "handsome", "plain", "quaint",
    "clean", "elegant", "easy", "angry", "crazy", "helpful", "mushy", "odd", "unsightly",
    "adorable", "important", "inexpensive", "cheap", "expensive", "fancy",
];
const COLOURS: [&str; 11] =
    ["red", "yellow", "blue", "green", "pink", "brown", "purple", "brown", "white", "black", "orange"];
const NOUNS: [&str; 13] = [
    "table", "chair", "house", "bbq", "desk", "car", "pony", "cookie", "sandwich", "burger",
    "pizza", "mouse", "keyboard",
];

/// Deterministic 32-bit LCG so both apps produce identical rows.
fn next_rand(seed: &mut u32, max: u32) -> u32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*seed >> 8) % max
}

fn label(seed: &mut u32) -> String {
    let a = ADJECTIVES[next_rand(seed, 25) as usize];
    let c = COLOURS[next_rand(seed, 11) as usize];
    let n = NOUNS[next_rand(seed, 13) as usize];
    format!("{a} {c} {n}")
}

// ---------------------------------------------------------------------------------------
// Document shell
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct DocumentProps {
    pub title: Str,
    pub children: Children,
}

pub struct Document;

impl Component for Document {
    type Props = DocumentProps;

    fn setup(_: &DocumentProps, _: &Handle<Self>) -> Self {
        Document
    }

    fn render(&self, props: &DocumentProps, _: &Handle<Self>) -> Node {
        rsx! {
            <!DOCTYPE html>
            <html lang="en">
                <head>
                    <meta charset="utf-8"/>
                    <meta name="viewport" content="width=device-width, initial-scale=1"/>
                    <title>{props.title.clone()}</title>
                    <link rel="stylesheet" href="/app.css"/>
                    <script type="module" src="/boot.js"></script>
                </head>
                <body>
                    <nav>
                        <a href="/">"Tasks"</a>
                        " | "
                        <a href="/ssr/100">"SSR rows"</a>
                        " | "
                        <a href="/client">"Client bench"</a>
                    </nav>
                    <main>{&props.children}</main>
                </body>
            </html>
        }
    }
}

// ---------------------------------------------------------------------------------------
// Home: a loader-style task list, a form action and one island
// ---------------------------------------------------------------------------------------

#[derive(Clone)]
pub struct Task {
    pub id: u32,
    pub title: String,
}

pub fn home_page(tasks: &[Task]) -> Node {
    let open = tasks.len() as i32;
    rsx! {
        <Document title="Tasks">
            <h1>"Tasks"</h1>
            <ul class="tasks">
                {tasks.iter().map(|t| rsx! { <li key={t.id}>{t.title.clone()}</li> }).collect::<Vec<_>>()}
            </ul>
            <form method="post" action="/tasks">
                <input name="title" placeholder="New task" required/>
                <button type="submit">"Add"</button>
            </form>
            <Counter initial={open}/>
        </Document>
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct CounterProps {
    pub initial: i32,
}

/// The Remix 3 README counter: plain state, explicit update().
pub struct Counter {
    count: i32,
}

impl Component for Counter {
    type Props = CounterProps;
    rsmix::client_entry!("Counter");

    fn setup(props: &CounterProps, _: &Handle<Self>) -> Self {
        Counter { count: props.initial }
    }

    fn render(&self, _: &CounterProps, h: &Handle<Self>) -> Node {
        rsx! {
            <button id="counter" type="button" on:click={h.on(|this, h| {
                this.count += 1;
                h.update();
            })}>
                "Count: " {self.count}
            </button>
        }
    }
}

// ---------------------------------------------------------------------------------------
// SSR benchmark page: N rows through a nested component per row
// ---------------------------------------------------------------------------------------

pub fn ssr_page(n: usize) -> Node {
    let mut seed = 42;
    let rows: Vec<Node> = (1..=n as u32)
        .map(|id| {
            let label = label(&mut seed);
            let done = id % 3 == 0;
            rsx! { <RowView key={id} id={id} label={label} done={done}/> }
        })
        .collect();
    rsx! {
        <Document title="SSR rows">
            <h1>"Rows: " {n}</h1>
            <table class="rows">
                <tbody>{rows}</tbody>
            </table>
        </Document>
    }
}

#[derive(Clone, Default)]
pub struct RowViewProps {
    pub id: u32,
    pub label: String,
    pub done: bool,
}

pub struct RowView;

impl Component for RowView {
    type Props = RowViewProps;

    fn setup(_: &RowViewProps, _: &Handle<Self>) -> Self {
        RowView
    }

    fn render(&self, p: &RowViewProps, _: &Handle<Self>) -> Node {
        rsx! {
            <tr class={p.done.then_some("done")}>
                <td class="id">{p.id}</td>
                <td><a href={format!("/rows/{}", p.id)}>{p.label.clone()}</a></td>
                <td><span class="badge">{if p.done { "done" } else { "open" }}</span></td>
            </tr>
        }
    }
}

// ---------------------------------------------------------------------------------------
// Client benchmark island (js-framework-benchmark style operations)
// ---------------------------------------------------------------------------------------

pub fn client_page() -> Node {
    rsx! {
        <Document title="Client bench">
            <h1>"Client bench"</h1>
            <RowsBench/>
        </Document>
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct RowsBenchProps {}

struct BenchRow {
    id: u32,
    label: String,
}

pub struct RowsBench {
    rows: Vec<BenchRow>,
    next_id: u32,
    seed: u32,
    selected: Option<u32>,
}

impl RowsBench {
    fn build(&mut self, count: usize) -> Vec<BenchRow> {
        (0..count)
            .map(|_| {
                self.next_id += 1;
                BenchRow { id: self.next_id, label: label(&mut self.seed) }
            })
            .collect()
    }

    /// Run `f`, render, then record the elapsed time on `<html data-last-op>`.
    fn op(h: &Handle<Self>, name: &'static str, f: fn(&mut RowsBench)) -> EventHandler {
        h.on(move |this, h| {
            let start = browser::now();
            f(this);
            h.update();
            h.queue_task(move |_, _| {
                browser::set_document_data("lastOp", &format!("{name}:{}", browser::now() - start));
            });
        })
    }
}

impl Component for RowsBench {
    type Props = RowsBenchProps;
    rsmix::client_entry!("RowsBench");

    fn setup(_: &RowsBenchProps, h: &Handle<Self>) -> Self {
        h.queue_task(|_, _| browser::set_document_data("hydrated", &browser::now().to_string()));
        RowsBench { rows: Vec::new(), next_id: 0, seed: 7, selected: None }
    }

    fn render(&self, _: &RowsBenchProps, h: &Handle<Self>) -> Node {
        let rows: Vec<Node> = self
            .rows
            .iter()
            .map(|row| {
                let id = row.id;
                rsx! {
                    <tr key={id} class={(self.selected == Some(id)).then_some("danger")}>
                        <td class="id">{id}</td>
                        <td><a class="select" on:click={h.on(move |this, h| {
                            this.selected = Some(id);
                            h.update();
                        })}>{row.label.clone()}</a></td>
                        <td><a class="remove" on:click={h.on(move |this, h| {
                            this.rows.retain(|r| r.id != id);
                            h.update();
                        })}>"x"</a></td>
                    </tr>
                }
            })
            .collect();

        rsx! {
            <div class="bench">
                <div class="controls">
                    <button id="create" on:click={Self::op(h, "create", |s| s.rows = s.build(1_000))}>"Create 1,000"</button>
                    <button id="create10k" on:click={Self::op(h, "create10k", |s| s.rows = s.build(10_000))}>"Create 10,000"</button>
                    <button id="append" on:click={Self::op(h, "append", |s| {
                        let more = s.build(1_000);
                        s.rows.extend(more);
                    })}>"Append 1,000"</button>
                    <button id="update" on:click={Self::op(h, "update", |s| {
                        for row in s.rows.iter_mut().step_by(10) {
                            row.label.push_str(" !!!");
                        }
                    })}>"Update every 10th"</button>
                    <button id="swap" on:click={Self::op(h, "swap", |s| {
                        if s.rows.len() > 998 {
                            s.rows.swap(1, 998);
                        }
                    })}>"Swap rows"</button>
                    <button id="select" on:click={Self::op(h, "select", |s| {
                        s.selected = s.rows.get(1).map(|r| r.id);
                    })}>"Select row 2"</button>
                    <button id="clear" on:click={Self::op(h, "clear", |s| s.rows.clear())}>"Clear"</button>
                </div>
                <table class="rows">
                    <tbody>{rows}</tbody>
                </table>
            </div>
        }
    }
}
