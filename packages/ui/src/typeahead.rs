//! Type-to-search for lists (Remix's `lib/typeahead.ts`).

use std::rc::Rc;

use rmx::{mixin, on, Element, HostMixin, Mixin, MixinHandle};

/// How long typed characters accumulate before the search text resets.
pub const HIDDEN_TYPEAHEAD_TIMEOUT_MS: i32 = 750;

/// Whether any of `values` starts with `text`, ignoring case.
pub fn matches_search_text(values: &[String], text: &str) -> bool {
    let text = text.to_lowercase();
    values.iter().any(|v| v.to_lowercase().starts_with(&text))
}

/// The next item after `from_index` (wrapping, `None` for "before the first") whose
/// search values start with `text`.
pub fn match_next_by_search_text<'a, T>(text: &str, items: &'a [T], from_index: Option<usize>, values: impl Fn(&T) -> Vec<String>) -> Option<&'a T> {
    if text.is_empty() || items.is_empty() {
        return None;
    }
    let len = items.len() as isize;
    let from = from_index.map_or(-1, |i| i as isize);
    (1..=len).map(|offset| &items[((from + offset + len) % len) as usize]).find(|item| matches_search_text(&values(item), text))
}

/// Called with the accumulated search text.
pub type OnTypeahead = Rc<dyn Fn(&str)>;

/// Collect typed characters on the host and report the search text (Remix's
/// `hiddenTypeahead`). Browser only.
pub fn hidden_typeahead(on_typeahead: impl Fn(&str) + 'static) -> Mixin {
    mixin::<HiddenTypeahead>(Rc::new(on_typeahead))
}

pub struct HiddenTypeahead {
    text: String,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    timeout: i32,
    on_typeahead: Option<OnTypeahead>,
}

impl HiddenTypeahead {
    fn clear(&mut self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(w) = web_sys::window() {
            w.clear_timeout_with_handle(self.timeout);
        }
        self.timeout = 0;
        self.text.clear();
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    fn set(&mut self, text: String, h: &MixinHandle<Self>) {
        self.text = text;
        if let Some(f) = self.on_typeahead.clone() {
            f(&self.text);
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(w) = web_sys::window() {
            use wasm_bindgen::JsCast;
            w.clear_timeout_with_handle(self.timeout);
            let h = h.clone();
            let reset = wasm_bindgen::closure::Closure::once_into_js(move || {
                h.with(|this, _| this.clear());
            });
            self.timeout = w.set_timeout_with_callback_and_timeout_and_arguments_0(reset.unchecked_ref(), HIDDEN_TYPEAHEAD_TIMEOUT_MS).unwrap_or(0);
        }
    }
}

impl HostMixin for HiddenTypeahead {
    type Args = OnTypeahead;
    const SSR: bool = false;

    fn setup(h: &MixinHandle<Self>, _: &'static str) -> Self {
        h.on_remove(|this, _| this.clear());
        HiddenTypeahead { text: String::new(), timeout: 0, on_typeahead: None }
    }

    fn render(&mut self, on_typeahead: &OnTypeahead, host: &mut Element, h: &MixinHandle<Self>) {
        self.on_typeahead = Some(on_typeahead.clone());
        host.mix(on(
            "focusout",
            h.on_event(|this, _, event| {
                #[cfg(target_arch = "wasm32")]
                {
                    use wasm_bindgen::JsCast;
                    let related = js_sys::Reflect::get(event.raw(), &"relatedTarget".into()).ok().and_then(|t| t.dyn_into::<web_sys::Node>().ok());
                    if let (Some(current), Some(related)) = (event.current_element(), related) {
                        if current.contains(Some(&related)) {
                            return;
                        }
                    }
                }
                let _ = event;
                this.clear();
            }),
        ));
        host.mix(on(
            "keydown",
            h.on_event(|this, h, event| {
                let key = event.key();
                let modified = event.prop_bool("ctrlKey") || event.prop_bool("altKey") || event.prop_bool("metaKey");
                if key.chars().count() == 1 && !modified {
                    let text = format!("{}{}", this.text, key.to_lowercase());
                    this.set(text, h);
                } else if key == "Escape" {
                    this.clear();
                } else if key == "Backspace" && !this.text.is_empty() {
                    let mut text = this.text.clone();
                    text.pop();
                    this.set(text, h);
                }
            }),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // typeahead.test.tsx: matches search text case-insensitively against string and array values
    #[test]
    fn matches_search_text_case_insensitively_against_string_and_array_values() {
        let string_item = ["Remix framework".to_string()];
        let array_item = ["Beta".to_string(), "Workbench".to_string()];
        assert!(matches_search_text(&string_item, "re"));
        assert!(!matches_search_text(&string_item, "FRAME"));
        assert!(matches_search_text(&array_item, "be"));
        assert!(matches_search_text(&array_item, "work"));
        assert!(!matches_search_text(&array_item, "prod"));
    }

    // typeahead.test.tsx: starts after fromIndex, wraps around, and returns null for empty or
    // missing matches
    #[test]
    fn starts_after_from_index_wraps_around_and_returns_none_for_empty_or_missing_matches() {
        let items: [(&str, &[&str]); 3] = [("Remix", &["remix"]), ("Staging", &["beta", "staging"]), ("Local", &["dev", "local"])];
        let values = |item: &(&str, &[&str])| item.1.iter().map(|v| v.to_string()).collect();
        assert_eq!(match_next_by_search_text("de", &items, Some(1), values).map(|i| i.0), Some("Local"));
        assert_eq!(match_next_by_search_text("re", &items, Some(2), values).map(|i| i.0), Some("Remix"));
        assert_eq!(match_next_by_search_text("", &items, Some(0), values), None);
        assert_eq!(match_next_by_search_text("zz", &items, Some(0), values), None);
    }

    #[test]
    fn matches_from_the_next_item_and_wraps() {
        let items = ["Remix", "React Router", "React", "Archived"].map(String::from);
        let values = |s: &String| vec![s.clone()];
        assert_eq!(match_next_by_search_text("re", &items, None, values), Some(&items[0]));
        assert_eq!(match_next_by_search_text("re", &items, Some(0), values), Some(&items[1]));
        assert_eq!(match_next_by_search_text("re", &items, Some(2), values), Some(&items[0]));
        assert_eq!(match_next_by_search_text("REACT", &items, Some(1), values), Some(&items[2]));
        assert_eq!(match_next_by_search_text("", &items, None, values), None);
        assert_eq!(match_next_by_search_text("x", &items, None, values), None);
    }
}
