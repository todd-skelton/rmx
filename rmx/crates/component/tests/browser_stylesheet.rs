//! Ports of Remix's `stylesheet.test.ts` (`createStyleManager`). Run with
//! `pwsh ./test-browser.ps1 -Test browser_stylesheet`.
#![cfg(target_arch = "wasm32")]

extern crate rmx_component as rmx;

use std::cell::RefCell;
use std::rc::Rc;

use rmx::StyleManager;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn document() -> web_sys::Document {
    web_sys::window().unwrap().document().unwrap()
}

fn adopted() -> Vec<web_sys::CssStyleSheet> {
    let list: js_sys::Array = js_sys::Reflect::get(&document(), &"adoptedStyleSheets".into()).unwrap().unchecked_into();
    list.iter().map(|s| s.unchecked_into()).collect()
}

fn last_sheet() -> web_sys::CssStyleSheet {
    adopted().pop().expect("an adopted sheet")
}

fn rule_texts(sheet: &web_sys::CssStyleSheet) -> Vec<String> {
    let rules = sheet.css_rules().unwrap();
    (0..rules.length()).map(|i| rules.item(i).unwrap().css_text()).collect()
}

fn rule_count(sheet: &web_sys::CssStyleSheet) -> u32 {
    sheet.css_rules().unwrap().length()
}

fn all_adopted_text() -> String {
    adopted().iter().map(|s| rule_texts(s).join("\n")).collect::<Vec<_>>().join("\n")
}

fn style_tag(class: &str, css: &str) -> web_sys::Element {
    let style = document().create_element("style").unwrap();
    style.set_attribute("data-rmx-style", class).unwrap();
    style.set_text_content(Some(css));
    style
}

fn color(el: &web_sys::Element) -> String {
    let style = web_sys::window().unwrap().get_computed_style(el).unwrap().unwrap();
    style.get_property_value("color").unwrap()
}

fn has_server_tag() -> bool {
    document().query_selector("style[data-rmx-style]").unwrap().is_some()
}

#[wasm_bindgen_test]
fn does_not_adopt_an_empty_stylesheet_until_styles_are_inserted() {
    let before = adopted().len();
    let mgr = StyleManager::new("rmx-test");
    assert_eq!(adopted().len(), before);
    mgr.dispose();
}

#[wasm_bindgen_test]
fn inserts_a_rule_once_and_increments_count_on_repeat() {
    let mgr = StyleManager::new("rmx-test");
    mgr.insert("rmx-a", ".rmx-a { color: red; }");
    let sheet = last_sheet();
    assert!(mgr.has("rmx-a"));
    assert_eq!(rule_count(&sheet), 1);

    mgr.insert("rmx-a", ".rmx-a { color: red; }");
    assert!(mgr.has("rmx-a"));
    assert_eq!(rule_count(&sheet), 1);

    let text = &rule_texts(&sheet)[0];
    assert!(text.contains("@layer rmx-test"));
    assert_eq!(text.lines().filter(|l| l.trim_start().starts_with(".rmx-a")).count(), 1, "{text}");
    mgr.dispose();
}

#[wasm_bindgen_test]
fn uses_default_layer_name_when_not_provided() {
    let mgr = StyleManager::new(rmx::STYLE_LAYER);
    mgr.insert("rmx-a", ".rmx-a { color: red; }");
    assert!(rule_texts(&last_sheet())[0].contains("@layer rmx"));
    mgr.dispose();
}

#[wasm_bindgen_test]
fn returns_false_for_has_when_class_was_never_inserted() {
    let mgr = StyleManager::new("rmx-test");
    assert!(!mgr.has("rmx-never-inserted"));
    mgr.dispose();
}

#[wasm_bindgen_test]
fn safely_handles_removing_a_class_that_was_never_inserted() {
    let mgr = StyleManager::new("rmx-test");
    let before = adopted().len();
    mgr.remove("rmx-never-inserted");
    assert_eq!(adopted().len(), before);
    assert!(!mgr.has("rmx-never-inserted"));
    mgr.dispose();
}

#[wasm_bindgen_test]
fn handles_multiple_managers_with_different_layers_independently() {
    let before = adopted().len();
    let mgr1 = StyleManager::new("layer-1");
    let mgr2 = StyleManager::new("layer-2");
    mgr1.insert("rmx-a", ".rmx-a { color: red; }");
    mgr2.insert("rmx-a", ".rmx-a { color: blue; }");
    let sheets = adopted();
    assert_eq!(rule_count(&sheets[before]), 1);
    assert_eq!(rule_count(&sheets[before + 1]), 1);
    assert!(rule_texts(&sheets[before])[0].contains("@layer layer-1.rmx-a"));
    assert!(rule_texts(&sheets[before + 1])[0].contains("@layer layer-2.rmx-a"));
    mgr1.dispose();
    mgr2.dispose();
}

#[wasm_bindgen_test]
fn handles_complex_css_rules_with_nested_selectors_and_at_rules() {
    let mgr = StyleManager::new("rmx-test");
    mgr.insert(
        "rmx-complex",
        ".rmx-complex { color: red; } .rmx-complex:hover { color: blue; } @media (min-width: 768px) { .rmx-complex { font-size: 16px; } }",
    );
    let sheet = last_sheet();
    assert_eq!(rule_count(&sheet), 1);
    let text = &rule_texts(&sheet)[0];
    assert!(text.contains(".rmx-complex") && text.contains(":hover") && text.contains("@media"), "{text}");
    mgr.dispose();
}

#[wasm_bindgen_test]
fn removes_only_when_count_reaches_zero_and_reindexes() {
    let mgr = StyleManager::new("rmx-test");
    mgr.insert("rmx-a", ".rmx-a { color: red; }");
    mgr.insert("rmx-b", ".rmx-b { color: blue; }");
    mgr.insert("rmx-a", ".rmx-a { color: red; }");
    let sheet = last_sheet();

    mgr.remove("rmx-a");
    assert!(mgr.has("rmx-a"));
    assert_eq!(rule_count(&sheet), 2);
    let both = rule_texts(&sheet).join("");
    assert!(both.contains(".rmx-a") && both.contains(".rmx-b"));

    mgr.remove("rmx-b");
    assert!(!mgr.has("rmx-b"));
    assert_eq!(rule_count(&sheet), 1);
    assert!(rule_texts(&sheet)[0].contains(".rmx-a"));

    mgr.remove("rmx-a");
    assert!(!mgr.has("rmx-a"));
    assert_eq!(rule_count(&sheet), 0);
    mgr.dispose();
}

#[wasm_bindgen_test]
fn keeps_other_rules_stable_when_removing_from_the_middle() {
    let mgr = StyleManager::new("rmx-test");
    mgr.insert("rmx-a", ".rmx-a { color: red; }");
    mgr.insert("rmx-b", ".rmx-b { color: blue; }");
    mgr.insert("rmx-c", ".rmx-c { color: green; }");
    let sheet = last_sheet();
    assert_eq!(rule_count(&sheet), 3);

    mgr.remove("rmx-b");
    assert_eq!(rule_count(&sheet), 2);
    let combined = rule_texts(&sheet).join("\n");
    assert!(combined.contains(".rmx-a") && combined.contains(".rmx-c"));

    mgr.remove("rmx-c");
    assert_eq!(rule_count(&sheet), 1);
    assert!(rule_texts(&sheet)[0].contains(".rmx-a"));
    mgr.dispose();
}

#[wasm_bindgen_test]
fn styles_apply_while_present_and_stop_after_full_removal() {
    let mgr = StyleManager::new("rmx-test");
    let el = document().create_element("div").unwrap();
    document().body().unwrap().append_child(&el).unwrap();

    mgr.insert("rmx-a", ".rmx-a { color: rgb(255, 0, 0); }");
    el.set_class_name("rmx-a");
    assert_eq!(color(&el), "rgb(255, 0, 0)");

    mgr.insert("rmx-a", ".rmx-a { color: rgb(255, 0, 0); }");
    mgr.remove("rmx-a");
    assert_eq!(color(&el), "rgb(255, 0, 0)");

    mgr.remove("rmx-a");
    assert_ne!(color(&el), "rgb(255, 0, 0)");
    el.remove();
    mgr.dispose();
}

#[wasm_bindgen_test]
fn keeps_duplicate_selectors_in_their_original_cascade_layer_order_across_managers() {
    let mgr1 = StyleManager::new("rmx-test");
    let mgr2 = StyleManager::new("rmx-test");
    let el = document().create_element("button").unwrap();
    document().body().unwrap().append_child(&el).unwrap();

    mgr1.insert("rmx-base", ".rmx-base { color: rgb(0, 0, 0); }");
    mgr1.insert("rmx-danger", ".rmx-danger { color: rgb(255, 0, 0); }");
    mgr2.insert("rmx-base", ".rmx-base { color: rgb(0, 0, 0); }");

    el.set_class_name("rmx-base rmx-danger");
    assert_eq!(color(&el), "rgb(255, 0, 0)");
    el.remove();
    mgr1.dispose();
    mgr2.dispose();
}

#[wasm_bindgen_test]
fn adopts_server_rendered_style_tag() {
    let tag = style_tag("rmxc-server1", "@layer rmx { .rmxc-server1 { color: blue; } }");
    document().head().unwrap().append_child(&tag).unwrap();
    assert!(has_server_tag());

    let mgr = StyleManager::new("rmx");
    mgr.adopt_server_styles(&document());

    assert!(!has_server_tag());
    assert!(mgr.has("rmxc-server1"));
    assert!(all_adopted_text().contains("rmxc-server1"));

    // Pinned: insert/remove cycles from hydrated elements never drop it.
    mgr.insert("rmxc-server1", ".rmxc-server1 { color: blue; }");
    mgr.remove("rmxc-server1");
    mgr.remove("rmxc-server1");
    assert!(mgr.has("rmxc-server1"));
    mgr.dispose();
}

async fn next_frame() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().request_animation_frame(&resolve).unwrap();
    });
    JsFuture::from(promise).await.unwrap();
}

async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms).unwrap();
    });
    JsFuture::from(promise).await.unwrap();
}

#[wasm_bindgen_test]
async fn does_not_start_transitions_when_adopting_server_rendered_styles() {
    let host = document().create_element("div").unwrap();
    let tag = style_tag(
        "rmxc-adopt-transition",
        r#"
      @layer rmx-test.rmxc-adopt-transition {
        .rmxc-adopt-transition { --offset: 8px; display: block; width: 30px; height: 18px; }
        .rmxc-adopt-transition::before {
          content: ""; display: block; width: 10px; height: 14px;
          transform: translateX(0); transition: transform 80ms ease, width 80ms ease;
        }
        .rmxc-adopt-transition[data-state="checked"]::before { width: 18px; transform: translateX(var(--offset)); }
      }"#,
    );
    let el = document().create_element("div").unwrap();
    el.set_class_name("rmxc-adopt-transition");
    el.set_attribute("data-state", "checked").unwrap();
    host.append_child(&tag).unwrap();
    host.append_child(&el).unwrap();
    document().body().unwrap().append_child(&host).unwrap();

    let before = |el: &web_sys::Element| {
        let s = web_sys::window().unwrap().get_computed_style_with_pseudo_elt(el, "::before").unwrap().unwrap();
        (s.get_property_value("transform").unwrap(), s.get_property_value("width").unwrap())
    };
    let (transform, width) = before(&el);
    assert_eq!(transform, "matrix(1, 0, 0, 1, 8, 0)");
    assert_eq!(width, "18px");

    let runs = Rc::new(RefCell::new(Vec::<String>::new()));
    let r = runs.clone();
    let listener = Closure::<dyn Fn(web_sys::Event)>::new(move |e: web_sys::Event| {
        let pseudo = js_sys::Reflect::get(&e, &"pseudoElement".into()).unwrap().as_string().unwrap_or_default();
        let prop = js_sys::Reflect::get(&e, &"propertyName".into()).unwrap().as_string().unwrap_or_default();
        r.borrow_mut().push(format!("{pseudo}:{prop}"));
    });
    el.add_event_listener_with_callback("transitionrun", listener.as_ref().unchecked_ref()).unwrap();

    let mgr = StyleManager::new("rmx-test");
    mgr.adopt_server_styles(&host);
    next_frame().await;
    next_frame().await;
    sleep(120).await;

    assert_eq!(before(&el), (transform, width));
    assert!(runs.borrow().is_empty(), "{:?}", runs.borrow());
    host.remove();
    mgr.dispose();
}

#[wasm_bindgen_test]
fn adopts_server_style_tags_added_after_manager_creation() {
    let mgr = StyleManager::new("rmx");
    let tag = style_tag("rmxc-stream1", "@layer rmx { .rmxc-stream1 { color: green; } }");
    document().head().unwrap().append_child(&tag).unwrap();
    mgr.adopt_server_styles(&document());
    assert!(!has_server_tag());
    assert!(mgr.has("rmxc-stream1"));
    mgr.dispose();
}

#[wasm_bindgen_test]
fn deduplicates_streamed_server_style_tags_by_selector_identity() {
    let mgr = StyleManager::new("rmx");
    let append = || {
        let tag = style_tag("rmxc-stream1", "@layer rmx { .rmxc-stream1 { color: green; } }");
        document().head().unwrap().append_child(&tag).unwrap();
    };
    let count = |selector: &str| adopted().iter().flat_map(rule_texts).filter(|t| t.contains(selector)).count();

    append();
    mgr.adopt_server_styles(&document());
    assert_eq!(count("rmxc-stream1"), 1);

    append();
    mgr.adopt_server_styles(&document());
    assert_eq!(document().query_selector_all(r#"style[data-rmx-style="rmxc-stream1"]"#).unwrap().length(), 0);
    assert_eq!(count("rmxc-stream1"), 1);
    assert!(mgr.has("rmxc-stream1"));
    mgr.dispose();
}

#[wasm_bindgen_test]
fn adopts_server_style_tags_in_document_order() {
    let mgr = StyleManager::new("rmx");
    let container = document().create_element("div").unwrap();
    for selector in ["rmxc-base", "rmxc-trigger", "rmxc-user"] {
        container.append_child(&style_tag(selector, &format!("@layer rmx {{ .{selector} {{ color: red; }} }}"))).unwrap();
    }
    document().body().unwrap().append_child(&container).unwrap();
    mgr.adopt_server_styles(&container);

    let text = rule_texts(&last_sheet()).join("\n");
    assert!(text.find(".rmxc-base").unwrap() < text.find(".rmxc-trigger").unwrap());
    assert!(text.find(".rmxc-trigger").unwrap() < text.find(".rmxc-user").unwrap());
    assert!(container.query_selector("style[data-rmx-style]").unwrap().is_none());
    container.remove();
    mgr.dispose();
}

#[wasm_bindgen_test]
fn removes_the_adopted_stylesheet_when_reset_leaves_the_manager_empty() {
    let before = adopted().len();
    let mgr = StyleManager::new("rmx-test");
    mgr.insert("rmx-a", ".rmx-a { color: red; }");
    assert_eq!(adopted().len(), before + 1);
    mgr.reset();
    assert_eq!(adopted().len(), before);
    assert!(!mgr.has("rmx-a"));
    mgr.dispose();
}

#[wasm_bindgen_test]
fn tracks_manager_generations_when_stylesheet_state_is_cleared() {
    let mgr = StyleManager::new("rmx-test");
    let initial = mgr.generation();
    mgr.insert("rmx-a", ".rmx-a { color: red; }");
    assert_eq!(mgr.generation(), initial);
    mgr.reset();
    assert_eq!(mgr.generation(), initial + 1);
    mgr.dispose();
    assert_eq!(mgr.generation(), initial + 2);
}

#[wasm_bindgen_test]
fn adopt_server_styles_returns_the_set_of_selectors_it_processed() {
    let mgr = StyleManager::new("rmx-test");
    let container = document().create_element("div").unwrap();
    for selector in ["rmxc-first", "rmxc-second"] {
        container.append_child(&style_tag(selector, &format!("@layer rmx-test {{ .{selector} {{ color: red; }} }}"))).unwrap();
    }
    document().body().unwrap().append_child(&container).unwrap();

    let mut first: Vec<String> = mgr.adopt_server_styles(&container).into_iter().collect();
    first.sort();
    assert_eq!(first, ["rmxc-first", "rmxc-second"]);

    container.append_child(&style_tag("rmxc-first", "@layer rmx-test { .rmxc-first { color: red; } }")).unwrap();
    let second: Vec<String> = mgr.adopt_server_styles(&container).into_iter().collect();
    assert_eq!(second, ["rmxc-first"]);
    container.remove();
    mgr.dispose();
}

#[wasm_bindgen_test]
fn selectors_iterates_the_currently_tracked_selectors() {
    let mgr = StyleManager::new("rmx-test");
    assert!(mgr.selectors().is_empty());
    mgr.insert("rmx-a", ".rmx-a { color: red; }");
    mgr.insert("rmx-b", ".rmx-b { color: blue; }");
    let mut list = mgr.selectors();
    list.sort();
    assert_eq!(list, ["rmx-a", "rmx-b"]);
    mgr.dispose();
}

#[wasm_bindgen_test]
fn keeps_server_adopted_rules_across_later_adoptions() {
    let mgr = StyleManager::new("rmx-test");
    let page = |selectors: [&str; 2], color: &str| {
        let el = document().create_element("div").unwrap();
        for selector in selectors {
            el.append_child(&style_tag(selector, &format!("@layer rmx-test {{ .{selector} {{ color: {color}; }} }}"))).unwrap();
        }
        document().body().unwrap().append_child(&el).unwrap();
        el
    };
    let first = page(["rmxc-only-first", "rmxc-shared"], "red");
    mgr.adopt_server_styles(&first);
    first.remove();
    assert!(mgr.has("rmxc-only-first") && mgr.has("rmxc-shared"));

    let second = page(["rmxc-only-second", "rmxc-shared"], "blue");
    mgr.adopt_server_styles(&second);
    second.remove();
    assert!(mgr.has("rmxc-only-first"));
    assert!(mgr.has("rmxc-shared"));
    assert!(mgr.has("rmxc-only-second"));
    mgr.dispose();
}

#[wasm_bindgen_test]
fn pins_a_client_inserted_rule_when_a_server_tag_for_the_same_selector_is_adopted() {
    let mgr = StyleManager::new("rmx-test");
    mgr.insert("rmxc-upgrade", ".rmxc-upgrade { color: red; }");
    let host = document().create_element("div").unwrap();
    host.append_child(&style_tag("rmxc-upgrade", "@layer rmx-test { .rmxc-upgrade { color: red; } }")).unwrap();
    document().body().unwrap().append_child(&host).unwrap();
    mgr.adopt_server_styles(&host);
    host.remove();

    mgr.remove("rmxc-upgrade");
    assert!(mgr.has("rmxc-upgrade"));
    mgr.dispose();
}

#[wasm_bindgen_test]
fn drops_client_only_rules_when_their_last_ref_is_removed() {
    let mgr = StyleManager::new("rmx-test");
    mgr.insert("rmxc-dynamic", ".rmxc-dynamic { width: 41px; }");
    mgr.insert("rmxc-dynamic", ".rmxc-dynamic { width: 41px; }");
    mgr.remove("rmxc-dynamic");
    assert!(mgr.has("rmxc-dynamic"));
    mgr.remove("rmxc-dynamic");
    assert!(!mgr.has("rmxc-dynamic"));
    mgr.dispose();
}
