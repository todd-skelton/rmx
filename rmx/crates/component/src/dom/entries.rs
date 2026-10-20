//! Client entries: the registry of hydratable components and the live, hydrated ones.
//!
//! A live entry is found from its `<!--rmx:h:ID-->` start comment through an expando
//! property, so a frame reload that keeps the comment keeps the entry (and its state).

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::{JsCast, JsValue};

use super::vdom::{hydrate_instance, unmount_root};
use super::{frames, warn, AnyInstance, Scope};
use crate::component::{Component, Instance};

type HydrateFn = fn(&str, &web_sys::Node, &mut Option<web_sys::Node>, &Scope) -> Result<Rc<dyn AnyInstance>, String>;
type UpdateFn = fn(&dyn AnyInstance, &str) -> Result<(), String>;

/// A client entry the browser runtime can hydrate, created with [`entry`].
pub struct Entry {
    name: &'static str,
    hydrate: HydrateFn,
    update: UpdateFn,
}

/// Register a component marked with `rmx::client_entry!` for hydration.
pub fn entry<C: Component>() -> Entry
where
    C::Props: serde::de::DeserializeOwned,
{
    Entry {
        name: C::CLIENT_ENTRY.expect("rmx::entry::<C>() needs rmx::client_entry! in C's Component impl"),
        hydrate: hydrate_entry::<C>,
        update: update_entry::<C>,
    }
}

fn hydrate_entry<C: Component>(
    props_json: &str,
    parent: &web_sys::Node,
    cursor: &mut Option<web_sys::Node>,
    scope: &Scope,
) -> Result<Rc<dyn AnyInstance>, String>
where
    C::Props: serde::de::DeserializeOwned,
{
    let props: C::Props = serde_json::from_str(props_json).map_err(|e| e.to_string())?;
    let inst = super::new_instance::<C>(props, scope);
    hydrate_instance(&*inst, parent, cursor);
    Ok(inst)
}

/// New props from a frame reload: store them and re-render, keeping component state.
fn update_entry<C: Component>(inst: &dyn AnyInstance, props_json: &str) -> Result<(), String>
where
    C::Props: serde::de::DeserializeOwned,
{
    let props: C::Props = serde_json::from_str(props_json).map_err(|e| e.to_string())?;
    let inst = inst.as_any().downcast_ref::<Instance<C>>().ok_or("entry type changed")?;
    *inst.props.borrow_mut() = props;
    if let Some(rc) = inst.handle.inst.upgrade() {
        super::schedule_update(rc);
    }
    Ok(())
}

struct LiveEntry {
    name: String,
    props: String,
    inst: Rc<dyn AnyInstance>,
}

thread_local! {
    static DEFS: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
    static LIVE: RefCell<Vec<Option<LiveEntry>>> = const { RefCell::new(Vec::new()) };
    static KEY: JsValue = JsValue::from_str("__rmx_entry");
}

pub(crate) fn register(entries: Vec<Entry>) {
    DEFS.with(|d| d.borrow_mut().extend(entries));
}

fn slot_of(start: &web_sys::Node) -> Option<usize> {
    KEY.with(|k| js_sys::Reflect::get(start, k).ok()?.as_f64()).map(|v| v as usize)
}

/// The component name of the live entry starting at `start`.
pub(crate) fn live_name(start: &web_sys::Node) -> Option<String> {
    let slot = slot_of(start)?;
    LIVE.with(|l| l.borrow().get(slot)?.as_ref().map(|e| e.name.clone()))
}

/// Hydrate the entry whose start comment is `start`, or hand new props to it when it is
/// already live. Returns the end comment.
pub(crate) fn hydrate_or_update(start: &web_sys::Comment, frame: Option<Rc<frames::FrameState>>) -> Option<web_sys::Node> {
    let start_node: &web_sys::Node = start.as_ref();
    let end = find_end(start_node)?;
    let id = start.data().strip_prefix("rmx:h:")?.to_string();
    let (name, props) = frames::entry_data(&id)?;

    if let Some(slot) = slot_of(start_node) {
        let update = LIVE.with(|l| {
            let mut live = l.borrow_mut();
            let entry = live.get_mut(slot)?.as_mut()?;
            if entry.name != name || entry.props == props {
                return None;
            }
            entry.props = props.clone();
            Some(entry.inst.clone())
        });
        if let Some(inst) = update {
            let def_update = DEFS.with(|d| d.borrow().iter().find(|e| e.name == name).map(|e| e.update));
            if let Some(f) = def_update {
                if let Err(e) = f(&*inst, &props) {
                    warn(&format!("rmx: could not update {name}: {e}"));
                }
            }
        }
        return Some(end);
    }

    let hydrate = DEFS.with(|d| d.borrow().iter().find(|e| e.name == name).map(|e| e.hydrate));
    let Some(hydrate) = hydrate else {
        warn(&format!("rmx: no client entry registered for {name}"));
        return Some(end);
    };
    let parent = start_node.parent_node()?;
    let mut cursor = start_node.next_sibling();
    let scope = Scope { parent: None, frame, depth: 0, errors: None, svg: false };
    match hydrate(&props, &parent, &mut cursor, &scope) {
        Ok(inst) => {
            // Drop server DOM the client render did not claim, up to the end marker.
            while let Some(node) = cursor.take() {
                if node.is_same_node(Some(&end)) {
                    break;
                }
                cursor = node.next_sibling();
                let _ = parent.remove_child(&node);
            }
            let slot = LIVE.with(|l| {
                let mut live = l.borrow_mut();
                let entry = Some(LiveEntry { name, props, inst });
                match live.iter().position(Option::is_none) {
                    Some(i) => {
                        live[i] = entry;
                        i
                    }
                    None => {
                        live.push(entry);
                        live.len() - 1
                    }
                }
            });
            KEY.with(|k| js_sys::Reflect::set(start_node, k, &JsValue::from(slot as u32)).ok());
        }
        Err(e) => warn(&format!("rmx: could not hydrate {name}: {e}")),
    }
    Some(end)
}

/// Unmount the live entry starting at `start`, if any. The DOM is left to the caller.
pub(crate) fn dispose(start: &web_sys::Node) {
    let Some(slot) = slot_of(start) else { return };
    KEY.with(|k| js_sys::Reflect::delete_property(start.unchecked_ref::<js_sys::Object>(), k).ok());
    let entry = LIVE.with(|l| l.borrow_mut().get_mut(slot).and_then(Option::take));
    if let Some(entry) = entry {
        unmount_root(&*entry.inst);
    }
}

pub(crate) fn is_start(node: &web_sys::Node) -> bool {
    node.dyn_ref::<web_sys::Comment>().is_some_and(|c| c.data().starts_with("rmx:h:"))
}

pub(crate) fn is_end(node: &web_sys::Node) -> bool {
    node.dyn_ref::<web_sys::Comment>().is_some_and(|c| c.data() == "/rmx:h")
}

/// The matching `<!--/rmx:h-->` after `start`.
pub(crate) fn find_end(start: &web_sys::Node) -> Option<web_sys::Node> {
    let mut depth = 1;
    let mut node = start.next_sibling();
    while let Some(n) = node {
        if is_start(&n) {
            depth += 1;
        } else if is_end(&n) {
            depth -= 1;
            if depth == 0 {
                return Some(n);
            }
        }
        node = n.next_sibling();
    }
    None
}
