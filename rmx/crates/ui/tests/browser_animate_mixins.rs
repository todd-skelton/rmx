//! Ports of Remix's `ui/src/animation/animate-mixins.test.tsx`. Where Remix mocks
//! `HTMLElement.prototype.animate`, the mock here returns an object built on
//! `Animation.prototype` (rmx checks that what it tracked is an `Animation`) with the same
//! `playState`, `reverse`, `commitStyles`, `cancel` and never-settling `finished`. Run with
//! `pwsh ./test-browser.ps1 -Crate rmx-ui -Test browser_animate_mixins`.
#![cfg(target_arch = "wasm32")]

use rmx::{create_root, rsx, Mixin};
use rmx_ui::animation::{animate_entrance, animate_exit, Animate};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn js(body: &str) -> JsValue {
    js_sys::Function::new_no_args(body).call0(&JsValue::NULL).unwrap()
}

fn container() -> web_sys::HtmlElement {
    web_sys::window().unwrap().document().unwrap().create_element("div").unwrap().unchecked_into()
}

/// Replaces `HTMLElement.prototype.animate` with a spy that counts calls (and calls to the
/// returned animation's methods) in `window.__spy`.
fn mock_animate() {
    js("
        if (window.__restoreAnimate) window.__restoreAnimate();
        let original = HTMLElement.prototype.animate;
        let spy = { animate: 0, reverse: 0, commitStyles: 0, cancel: 0 };
        window.__spy = spy;
        HTMLElement.prototype.animate = function () {
            spy.animate++;
            let animation = Object.create(Animation.prototype);
            Object.defineProperties(animation, {
                playState: { value: 'running' },
                reverse: { value: () => { spy.reverse++; } },
                commitStyles: { value: () => { spy.commitStyles++; } },
                cancel: { value: () => { spy.cancel++; } },
                finished: { value: new Promise(() => {}) },
            });
            return animation;
        };
        window.__restoreAnimate = () => { HTMLElement.prototype.animate = original; window.__restoreAnimate = null; };
    ");
}

fn restore_animate() {
    js("if (window.__restoreAnimate) window.__restoreAnimate();");
}

fn spy(name: &str) -> f64 {
    js(&format!("return window.__spy.{name}")).as_f64().unwrap()
}

async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms).unwrap();
    });
    JsFuture::from(promise).await.unwrap();
}

/// Remix's `await Promise.resolve()`.
async fn microtask() {
    JsFuture::from(js_sys::Promise::resolve(&JsValue::UNDEFINED)).await.unwrap();
}

fn fade(ms: f64) -> Animate {
    Animate::new(ms).opacity(0.0)
}

fn exit_only(id: &'static str) -> rmx::Node {
    rsx! { <div key="item" id={id} mix={[animate_exit(fade(150.0))]}></div> }
}

fn find(el: &web_sys::HtmlElement, id: &str) -> Option<web_sys::Element> {
    el.query_selector(&format!("#{id}")).unwrap()
}

// animate-mixins.test.tsx: reclaims persisted nodes by type/key and reuses the same DOM element
#[wasm_bindgen_test]
fn reclaims_persisted_nodes_by_type_and_key_and_reuses_the_same_dom_element() {
    restore_animate();
    let el = container();
    let root = create_root(&el);
    root.render(exit_only("reclaim-target"));
    root.flush();
    let first = find(&el, "reclaim-target").expect("rendered");

    root.render(());
    root.flush();
    assert!(find(&el, "reclaim-target").is_some_and(|n| n == first), "the exiting node persists");

    root.render(exit_only("reclaim-target"));
    root.flush();
    assert!(find(&el, "reclaim-target").is_some_and(|n| n == first), "the persisted node is reused");
}

// animate-mixins.test.tsx: retargets reclaim to natural styles instead of reversing exit animation
#[wasm_bindgen_test]
async fn retargets_reclaim_to_natural_styles_instead_of_reversing_exit_animation() {
    mock_animate();
    let el = container();
    let root = create_root(&el);
    root.render(exit_only("reverse-target"));
    root.flush();

    root.render(());
    root.flush();
    microtask().await;
    assert_eq!(spy("animate"), 1.0);

    root.render(exit_only("reverse-target"));
    root.flush();
    microtask().await;
    assert_eq!(spy("reverse"), 0.0);
    assert_eq!(spy("commitStyles"), 1.0);
    assert_eq!(spy("cancel"), 1.0);
    assert_eq!(spy("animate"), 2.0);
    restore_animate();
}

// animate-mixins.test.tsx: does not reverse on initial insert when entrance and exit mixins are both present
#[wasm_bindgen_test]
fn does_not_reverse_on_initial_insert_when_entrance_and_exit_mixins_are_both_present() {
    mock_animate();
    let el = container();
    let root = create_root(&el);
    root.render(rsx! {
        <div key="both" id="both-mixins-target" mix={[animate_entrance(fade(150.0)), animate_exit(fade(150.0))]}></div>
    });
    root.flush();
    assert_eq!(spy("animate"), 1.0);
    assert_eq!(spy("reverse"), 0.0);
    restore_animate();
}

// animate-mixins.test.tsx: keeps persist behavior after reclaim interruption completes
#[wasm_bindgen_test]
async fn keeps_persist_behavior_after_reclaim_interruption_completes() {
    restore_animate();
    let el = container();
    let root = create_root(&el);
    let view = || rsx! { <div key="item" id="persist-after-interrupt" mix={both(40.0, true)}></div> };

    root.render(view());
    root.flush();
    root.render(());
    root.flush();
    assert!(find(&el, "persist-after-interrupt").is_some());

    root.render(view());
    root.flush();
    sleep(80).await;

    root.render(());
    root.flush();
    assert!(find(&el, "persist-after-interrupt").is_some());
}

fn both(ms: f64, initial: bool) -> Vec<Mixin> {
    vec![animate_entrance(fade(ms).initial(initial)), animate_exit(fade(ms))]
}

// animate-mixins.test.tsx: skips first entrance when initial is false, but animates on reclaimed add
#[wasm_bindgen_test]
async fn skips_first_entrance_when_initial_is_false_but_animates_on_reclaimed_add() {
    mock_animate();
    let el = container();
    let root = create_root(&el);
    let view = || rsx! { <div key="initial-false" id="initial-false-target" mix={both(100.0, false)}></div> };

    root.render(view());
    root.flush();
    assert_eq!(spy("animate"), 0.0);

    root.render(());
    root.flush();
    microtask().await;
    assert_eq!(spy("animate"), 1.0);

    root.render(view());
    root.flush();
    microtask().await;
    assert_eq!(spy("animate"), 2.0);
    restore_animate();
}
