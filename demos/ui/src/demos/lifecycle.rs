//! rsmix's own browser check for stateful mixins (not a Remix demo). Every lifecycle event
//! is appended to a log shown on the page and mirrored to `<html data-mixin-log>`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rsmix::{css, mixin, on, rsx, Component, Element, Handle, HostMixin, Mixin, MixinHandle, Node, NodeHandle};

/// Event log shared through context.
#[derive(Default)]
pub struct Log(RefCell<Vec<String>>);

impl Log {
    fn push(&self, line: String) {
        self.0.borrow_mut().push(line);
        rsmix::browser::set_document_data("mixinLog", &self.0.borrow().join("|"));
    }
}

/// The docs' `clickCount`: state in the mixin, `update()` re-runs only the host's mixins.
pub struct ClickCount {
    count: u32,
}

impl HostMixin for ClickCount {
    type Args = &'static str;

    fn setup(h: &MixinHandle<Self>, tag: &'static str) -> Self {
        let log = h.context::<Log>();
        h.on_insert(move |this, _, event| {
            if let Some(log) = &log {
                log.push(format!("insert:{tag}:{}:{}", this.count, event.node.tag_name()));
            }
        });
        ClickCount { count: 0 }
    }

    fn render(&mut self, label: &&'static str, host: &mut Element, h: &MixinHandle<Self>) {
        host.set_attr("aria-label", format!("{label}: {} clicks", self.count));
        host.mix(on("click", h.on(|this, h| {
            this.count += 1;
            h.update();
        })));
    }
}

/// Logs `insert`, `remove`, `beforeUpdate` and `commit` with its name.
pub struct Tracer {
    name: &'static str,
    log: Option<Rc<Log>>,
}

impl HostMixin for Tracer {
    type Args = &'static str;

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        h.on_insert(|this, _, event| {
            let tag = event.node.tag_name();
            this.log(format!("insert:{}:{tag}", this.name));
        });
        h.on_remove(|this, _| this.log(format!("remove:{}", this.name)));
        h.on_before_update(|this, _, _| this.log(format!("beforeUpdate:{}", this.name)));
        h.on_commit(|this, _, _| this.log(format!("commit:{}", this.name)));
        Tracer { name: "", log: h.context::<Log>() }
    }

    fn render(&mut self, name: &&'static str, host: &mut Element, h: &MixinHandle<Self>) {
        self.name = name;
        host.set_attr("data-mixin-id", h.id().to_string());
    }
}

impl Tracer {
    fn log(&self, line: String) {
        if let Some(log) = &self.log {
            log.push(line);
        }
    }
}

/// Keeps a removed row in the document for 300 ms (`persistNode`), marked `data-leaving`.
pub struct FadeOut {
    log: Option<Rc<Log>>,
    name: String,
    node: Option<NodeHandle>,
}

impl HostMixin for FadeOut {
    type Args = String;
    const SSR: bool = false;

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        h.on_insert(|this, _, event| this.node = Some(event.node.clone()));
        h.on_before_remove(|this, _, event| {
            this.push(format!("beforeRemove:{}", this.name));
            #[cfg(target_arch = "wasm32")]
            {
                let (node, log, name) = (this.node.clone(), this.log.clone(), this.name.clone());
                event.persist_node(move |signal| async move {
                    if let Some(node) = &node {
                        let _ = node.element().set_attribute("data-leaving", "");
                    }
                    rsmix::browser::sleep(300).await;
                    if let Some(log) = log {
                        log.push(format!("persisted:{name}:{}", signal.aborted()));
                    }
                });
            }
            #[cfg(not(target_arch = "wasm32"))]
            let _ = event;
        });
        h.on_remove(|this, _| this.push(format!("remove:{}", this.name)));
        FadeOut { log: h.context::<Log>(), name: String::new(), node: None }
    }

    fn render(&mut self, name: &String, _: &mut Element, _: &MixinHandle<Self>) {
        self.name.clone_from(name);
    }
}

impl FadeOut {
    fn push(&self, line: String) {
        if let Some(log) = &self.log {
            log.push(line);
        }
    }
}

pub struct MixinLifecycle {
    renders: Cell<u32>,
    traced: bool,
    rows: Vec<u32>,
    next_row: u32,
}

impl Component for MixinLifecycle {
    type Props = ();
    rsmix::client_entry!("MixinLifecycle");

    fn setup(_: &(), h: &Handle<Self>) -> Self {
        h.set_context(Log::default());
        MixinLifecycle { renders: Cell::new(0), traced: true, rows: vec![1, 2, 3], next_row: 4 }
    }

    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        self.renders.set(self.renders.get() + 1);
        let tracer: Option<Mixin> = self.traced.then(|| mixin::<Tracer>("box"));
        let rows: Vec<Node> = self
            .rows
            .iter()
            .map(|&id| {
                let name = format!("row{id}");
                rsx! {
                    <li key={id} data-fade={name.clone()} mix={mixin::<FadeOut>(name.clone())}>
                        {name.clone()}
                        " "
                        <button class="remove" mix={on("click", h.on(move |this, h| {
                            this.rows.retain(|r| *r != id);
                            h.update();
                        }))}>"remove"</button>
                    </li>
                }
            })
            .collect();
        rsx! {
            <main mix={css("font-family: system-ui, sans-serif; padding: 24px; display: grid; gap: 12px; max-width: 640px;")}>
                <h1>"Mixin lifecycle"</h1>
                <p>"Owner renders: " <strong id="owner-renders">{self.renders.get()}</strong></p>
                <div>
                    <button id="save" type="button" mix={mixin::<ClickCount>("Save")}>"Save"</button>
                </div>
                <div id="traced" mix={tracer}>"traced box"</div>
                <div mix={css("display: flex; gap: 8px;")}>
                    <button id="toggle-tracer" mix={on("click", h.on(|this, h| { this.traced = !this.traced; h.update(); }))}>"Toggle tracer"</button>
                    <button id="rerender" mix={on("click", h.on(|_, h| h.update()))}>"Re-render owner"</button>
                    <button id="add-row" mix={on("click", h.on(|this, h| { this.rows.push(this.next_row); this.next_row += 1; h.update(); }))}>"Add row"</button>
                    <button id="clear-rows" mix={on("click", h.on(|this, h| { this.rows.clear(); h.update(); }))}>"Clear rows"</button>
                </div>
                <ul id="rows">{rows}</ul>
            </main>
        }
    }
}
