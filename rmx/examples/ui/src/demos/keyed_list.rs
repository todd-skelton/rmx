//! `packages/component/src/runtime/demos/keyed-list.demo.tsx`

use rmx::browser::{random, Interval};
use rmx::{on, rsx, Component, Handle, Node};

#[derive(Clone)]
struct ListItem {
    id: &'static str,
    label: &'static str,
}

pub struct KeyedList {
    items: Vec<ListItem>,
    shuffle_interval: Option<Interval>,
}

impl KeyedList {
    fn move_up(&mut self, index: usize) {
        if index > 0 {
            self.items.swap(index - 1, index);
        }
    }

    fn move_down(&mut self, index: usize) {
        if index + 1 < self.items.len() {
            self.items.swap(index, index + 1);
        }
    }

    fn shuffle(&mut self) {
        for i in (1..self.items.len()).rev() {
            let j = (random() * (i + 1) as f64) as usize;
            self.items.swap(i, j);
        }
    }
}

impl Component for KeyedList {
    type Props = ();
    rmx::client_entry!("KeyedList");

    fn setup(_: &(), _: &Handle<Self>) -> Self {
        let items = [("a", "Item A"), ("b", "Item B"), ("c", "Item C"), ("d", "Item D")];
        KeyedList { items: items.into_iter().map(|(id, label)| ListItem { id, label }).collect(), shuffle_interval: None }
    }

    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let rows: Vec<Node> = self
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                rsx! {
                    <div key={item.id} class="list-item">
                        <input type="text" placeholder={item.label} defaultValue={item.label}/>
                        <button mix={[on("click", h.on(move |this, h| { this.move_up(index); h.update(); }))]}>"\u{2191}"</button>
                        <button mix={[on("click", h.on(move |this, h| { this.move_down(index); h.update(); }))]}>"\u{2193}"</button>
                    </div>
                }
            })
            .collect();
        rsx! {
            <div>
                <div class="controls">
                    <button mix={[on("click", h.on(|this, h| { this.items.reverse(); h.update(); }))]}>"Reverse List"</button>
                    <button mix={[on("click", h.on(|this, h| { this.shuffle(); h.update(); }))]}>"Shuffle List"</button>
                    <button mix={[on("click", h.on(|this, h| {
                        if this.shuffle_interval.take().is_none() {
                            let h2 = h.clone();
                            this.shuffle_interval = Some(Interval::new(1000, move || {
                                h2.with(|this, h| { this.shuffle(); h.update(); });
                            }));
                        }
                        h.update();
                    }))]}>
                        {if self.shuffle_interval.is_some() { "Stop Auto-Shuffle" } else { "Start Auto-Shuffle" }}
                    </button>
                </div>
                {rows}
            </div>
        }
    }
}
