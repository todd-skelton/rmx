//! Reconcile server HTML into the live DOM (Remix's `diff-dom`).
//!
//! Sibling lists are split into units: plain nodes, client entry ranges
//! (`<!--rmx:h:ID-->` .. `<!--/rmx:h-->`) and frame ranges (`<!--rmx:f:ID-->` ..
//! `<!--/rmx:f-->`). Elements with `data-rmx-key` match by key; an entry range matches a
//! live entry of the same component (its DOM and state are kept, new props are applied by
//! hydration afterwards); a frame range matches a live frame with the same src and name
//! (its content is diffed in place). Everything else pairs up by position and type.
//!
//! Live state the user changed wins over server HTML: typed input values, checkboxes,
//! selected options, open `<details>`/`<dialog>` and textarea contents. Elements with
//! `data-rmx-preserve-dom` are never touched, and `data-rmx-preserve-attrs="a b"` keeps
//! the listed attributes.

use std::collections::HashMap;

use wasm_bindgen::{JsCast, JsValue};

use super::{entries, frames};

enum Unit {
    Node(web_sys::Node),
    Entry { start: web_sys::Node, end: web_sys::Node },
    Frame { start: web_sys::Node, end: web_sys::Node },
}

impl Unit {
    fn first(&self) -> &web_sys::Node {
        match self {
            Unit::Node(n) => n,
            Unit::Entry { start, .. } | Unit::Frame { start, .. } => start,
        }
    }

    fn nodes(&self) -> Vec<web_sys::Node> {
        match self {
            Unit::Node(n) => vec![n.clone()],
            Unit::Entry { start, end } | Unit::Frame { start, end } => {
                frames::range_nodes(start.unchecked_ref(), end.unchecked_ref(), true)
            }
        }
    }

    fn key(&self) -> Option<String> {
        match self {
            Unit::Node(n) => n.dyn_ref::<web_sys::Element>()?.get_attribute("data-rmx-key"),
            _ => None,
        }
    }

    fn kind(&self) -> u8 {
        match self {
            Unit::Node(_) => 0,
            Unit::Entry { .. } => 1,
            Unit::Frame { .. } => 2,
        }
    }
}

fn parse_units(nodes: Vec<web_sys::Node>) -> Vec<Unit> {
    let mut units = Vec::with_capacity(nodes.len());
    let mut i = 0;
    while i < nodes.len() {
        let node = &nodes[i];
        let boundary = if entries::is_start(node) {
            matching_end(&nodes, i, entries::is_start, entries::is_end).map(|j| (j, 1))
        } else if frames::is_start(node) {
            matching_end(&nodes, i, frames::is_start, frames::is_end).map(|j| (j, 2))
        } else {
            None
        };
        match boundary {
            Some((j, kind)) => {
                let (start, end) = (node.clone(), nodes[j].clone());
                units.push(if kind == 1 { Unit::Entry { start, end } } else { Unit::Frame { start, end } });
                i = j + 1;
            }
            None => {
                units.push(Unit::Node(node.clone()));
                i += 1;
            }
        }
    }
    units
}

fn matching_end(
    nodes: &[web_sys::Node],
    start: usize,
    is_start: fn(&web_sys::Node) -> bool,
    is_end: fn(&web_sys::Node) -> bool,
) -> Option<usize> {
    let mut depth = 1;
    for (j, n) in nodes.iter().enumerate().skip(start + 1) {
        if is_start(n) {
            depth += 1;
        } else if is_end(n) {
            depth -= 1;
            if depth == 0 {
                return Some(j);
            }
        }
    }
    None
}

fn comparable(a: &web_sys::Node, b: &web_sys::Node) -> bool {
    if a.node_type() != b.node_type() {
        return false;
    }
    match (a.dyn_ref::<web_sys::Element>(), b.dyn_ref::<web_sys::Element>()) {
        (Some(a), Some(b)) => a.tag_name() == b.tag_name(),
        _ => true,
    }
}

fn marker_id(node: &web_sys::Node, prefix: &str) -> Option<String> {
    node.dyn_ref::<web_sys::Comment>()?.data().strip_prefix(prefix).map(str::to_string)
}

/// Whether a live boundary unit stays for the incoming one.
fn preserves(current: &Unit, next: &Unit) -> bool {
    match (current, next) {
        (Unit::Entry { start: c, .. }, Unit::Entry { start: n, .. }) => {
            let live = entries::live_name(c);
            let incoming = marker_id(n, "rmx:h:").and_then(|id| frames::entry_data(&id)).map(|(name, _)| name);
            live.is_some() && live == incoming
        }
        (Unit::Frame { start: c, .. }, Unit::Frame { start: n, .. }) => {
            let Some(frame) = frames::live(c) else { return false };
            let incoming = marker_id(n, "rmx:f:").and_then(|id| frames::frame_data(&id));
            incoming.is_some_and(|(src, name, _)| same_src(&frame.src(), &src) && frame.name() == name)
        }
        _ => false,
    }
}

/// Frame sources match when they resolve to the same URL.
fn same_src(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let base = super::window().location().href().unwrap_or_default();
    let resolve = |s: &str| web_sys::Url::new_with_base(s, &base).map(|u| u.href()).ok();
    resolve(a).is_some() && resolve(a) == resolve(b)
}

/// Reconcile `current` (children of `parent`, before `tail`) to look like `next`. Nodes in
/// `next` that have no match are moved into `parent`.
pub(crate) fn diff_nodes(parent: &web_sys::Node, current: Vec<web_sys::Node>, next: Vec<web_sys::Node>, tail: Option<web_sys::Node>) {
    let current = parse_units(current);
    let next = parse_units(next);

    let mut by_key: HashMap<String, usize> = HashMap::new();
    for (i, unit) in current.iter().enumerate() {
        if let Some(k) = unit.key() {
            by_key.insert(k, i);
        }
    }
    let mut used = vec![false; current.len()];
    let mut matched: Vec<Option<usize>> = vec![None; next.len()];

    // Keyed elements and preserved boundaries first, anywhere in the list.
    for (i, unit) in next.iter().enumerate() {
        let mut found = None;
        if let Some(k) = unit.key() {
            if let Some(&j) = by_key.get(&k) {
                if !used[j] && current[j].kind() == 0 && comparable(current[j].first(), unit.first()) {
                    found = Some(j);
                }
            }
        }
        if found.is_none() && unit.kind() != 0 {
            found = (0..current.len()).find(|&j| !used[j] && current[j].kind() == unit.kind() && preserves(&current[j], unit));
        }
        if let Some(j) = found {
            used[j] = true;
            matched[i] = Some(j);
        }
    }

    // Then pair the remaining plain, unkeyed nodes in order.
    let rest_current: Vec<usize> = (0..current.len()).filter(|&j| !used[j]).collect();
    let rest_next: Vec<usize> = (0..next.len()).filter(|&i| matched[i].is_none()).collect();
    for (&j, &i) in rest_current.iter().zip(&rest_next) {
        let (c, n) = (&current[j], &next[i]);
        if c.kind() == 0 && n.kind() == 0 && c.key().is_none() && n.key().is_none() && comparable(c.first(), n.first()) {
            used[j] = true;
            matched[i] = Some(j);
        }
    }

    // Update matched units in place.
    for (i, unit) in next.iter().enumerate() {
        let Some(j) = matched[i] else { continue };
        match (&current[j], unit) {
            (Unit::Node(c), Unit::Node(n)) => diff_node(c, n),
            (Unit::Entry { start: c, .. }, Unit::Entry { start: n, .. }) => copy_comment(c, n),
            (Unit::Frame { start: c, end: ce }, Unit::Frame { start: n, end: ne }) => {
                copy_comment(c, n);
                let interior = frames::range_nodes(c.unchecked_ref(), ce.unchecked_ref(), false);
                let next_interior = frames::range_nodes(n.unchecked_ref(), ne.unchecked_ref(), false);
                diff_nodes(parent, interior, next_interior, Some(ce.clone()));
            }
            _ => {}
        }
    }

    // Remove what is gone before moving survivors, so they do not lose focus.
    for (j, unit) in current.iter().enumerate() {
        if !used[j] {
            for node in unit.nodes() {
                dispose_subtree(&node);
                if let Some(p) = node.parent_node() {
                    let _ = p.remove_child(&node);
                }
            }
        }
    }

    // Place units in order, back to front.
    let mut anchor = tail;
    for (i, unit) in next.iter().enumerate().rev() {
        let placed = match matched[i] {
            Some(j) => &current[j],
            None => unit,
        };
        let first = placed.first().clone();
        let after = match placed {
            Unit::Node(n) => n.next_sibling(),
            Unit::Entry { end, .. } | Unit::Frame { end, .. } => end.next_sibling(),
        };
        let in_place = first.parent_node().is_some_and(|p| p.is_same_node(Some(parent))) && same(after.as_ref(), anchor.as_ref());
        if !in_place {
            for node in placed.nodes() {
                let _ = parent.insert_before(&node, anchor.as_ref());
            }
        }
        anchor = Some(first);
    }
}

fn same(a: Option<&web_sys::Node>, b: Option<&web_sys::Node>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.is_same_node(Some(b)),
        _ => false,
    }
}

fn copy_comment(current: &web_sys::Node, next: &web_sys::Node) {
    if let (Some(c), Some(n)) = (current.dyn_ref::<web_sys::Comment>(), next.dyn_ref::<web_sys::Comment>()) {
        let data = n.data();
        if c.data() != data {
            c.set_data(&data);
        }
    }
}

fn diff_node(current: &web_sys::Node, next: &web_sys::Node) {
    match current.node_type() {
        web_sys::Node::TEXT_NODE | web_sys::Node::COMMENT_NODE => {
            let text = next.node_value().unwrap_or_default();
            if current.node_value().unwrap_or_default() != text {
                current.set_node_value(Some(&text));
            }
        }
        web_sys::Node::ELEMENT_NODE => {
            let (c, n): (&web_sys::Element, &web_sys::Element) = (current.unchecked_ref(), next.unchecked_ref());
            if n.has_attribute("data-rmx-preserve-dom") {
                if !c.has_attribute("data-rmx-preserve-dom") {
                    let _ = c.set_attribute("data-rmx-preserve-dom", "");
                }
                return;
            }
            diff_attributes(c, n);
            if c.tag_name() == "TEXTAREA" && prop_str(c, "value") != prop_str(n, "value") {
                return;
            }
            diff_nodes(current, frames::child_nodes(current), frames::child_nodes(next), None);
        }
        _ => {}
    }
}

fn prop(el: &web_sys::Element, name: &str) -> JsValue {
    js_sys::Reflect::get(el, &JsValue::from_str(name)).unwrap_or(JsValue::UNDEFINED)
}

fn prop_str(el: &web_sys::Element, name: &str) -> String {
    prop(el, name).as_string().unwrap_or_default()
}

/// Attributes whose live (user-changed) state differs from the server's and should stay.
fn keep_live(current: &web_sys::Element, next: &web_sys::Element, name: &str) -> bool {
    let tag = current.tag_name();
    match name {
        "open" if tag == "DETAILS" || tag == "DIALOG" => prop(current, "open") != prop(next, "open"),
        "checked" if tag == "INPUT" => prop(current, "checked") != prop(next, "checked"),
        "selected" if tag == "OPTION" => prop(current, "selected") != prop(next, "selected"),
        "value" if tag == "INPUT" => {
            let ty = prop_str(current, "type");
            !matches!(ty.as_str(), "button" | "checkbox" | "hidden" | "image" | "radio" | "reset" | "submit")
                && prop_str(current, "value") != prop_str(next, "value")
        }
        _ => false,
    }
}

pub(crate) fn diff_attributes(current: &web_sys::Element, next: &web_sys::Element) {
    let preserved: Vec<String> = next
        .get_attribute("data-rmx-preserve-attrs")
        .map(|s| s.split_ascii_whitespace().map(str::to_string).collect())
        .unwrap_or_default();
    let names = |el: &web_sys::Element| -> Vec<String> {
        el.get_attribute_names().iter().filter_map(|v| v.as_string()).collect()
    };
    let next_names = names(next);
    for name in names(current) {
        if preserved.contains(&name) || next_names.contains(&name) {
            continue;
        }
        if !keep_live(current, next, &name) {
            let _ = current.remove_attribute(&name);
        }
    }
    for name in next_names {
        if preserved.contains(&name) {
            continue;
        }
        let value = next.get_attribute(&name).unwrap_or_default();
        if current.get_attribute(&name).as_deref() != Some(&value) && !keep_live(current, next, &name) {
            let _ = current.set_attribute(&name, &value);
        }
    }
}

/// Head nodes the runtime manages itself: scripts (never re-run) and `css()` styles.
fn managed_head_node(node: &web_sys::Node) -> bool {
    let Some(el) = node.dyn_ref::<web_sys::Element>() else { return false };
    el.tag_name() == "SCRIPT" || el.has_attribute("data-rmx-style")
}

pub(crate) fn diff_head(current: &web_sys::HtmlHeadElement, next: &web_sys::HtmlHeadElement) {
    let current_node: &web_sys::Node = current.as_ref();
    let all = frames::child_nodes(current_node);
    let tail = all.iter().find(|n| managed_head_node(n)).cloned();
    let cur: Vec<_> = all.into_iter().filter(|n| !managed_head_node(n)).collect();
    let nxt: Vec<_> = frames::child_nodes(next.as_ref()).into_iter().filter(|n| !managed_head_node(n)).collect();
    diff_nodes(current_node, cur, nxt, tail);
}

/// Release runtime state (client entries, frames) for a node that is leaving the
/// document, including everything inside it.
pub(crate) fn dispose_subtree(node: &web_sys::Node) {
    if node.node_type() == web_sys::Node::COMMENT_NODE {
        dispose_comment(node);
        return;
    }
    if node.node_type() != web_sys::Node::ELEMENT_NODE || node.first_child().is_none() {
        return;
    }
    let doc = super::document();
    let Ok(walker) = doc.create_tree_walker_with_what_to_show(node, 0x80) else { return };
    while let Ok(Some(c)) = walker.next_node() {
        dispose_comment(&c);
    }
}

fn dispose_comment(node: &web_sys::Node) {
    if entries::is_start(node) {
        entries::dispose(node);
    } else if frames::is_start(node) {
        frames::dispose(node);
    }
}
