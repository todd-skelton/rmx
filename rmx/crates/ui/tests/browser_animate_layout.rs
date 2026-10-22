//! Ports of Remix's `ui/src/animation/animate-layout-mixin.test.tsx`, with the same mocks:
//! `Element.prototype.animate` records plain mock animations, `requestAnimationFrame` runs on
//! a microtask, and each element's `getBoundingClientRect` returns a fixed rect or a
//! sequence of rects (the last one repeats). Run with
//! `pwsh ./test-browser.ps1 -Crate rmx-ui -Test browser_animate_layout`.
#![cfg(target_arch = "wasm32")]

use rmx::{create_root, rsx, Root};
use rmx_ui::animation::{animate_layout, Layout, LayoutConfig};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn js(body: &str) -> JsValue {
    js_sys::Function::new_no_args(body).call0(&JsValue::NULL).unwrap()
}

/// Remix's `beforeEach`: installs the mocks, restoring the originals first.
fn setup() -> (web_sys::HtmlElement, Root) {
    js("
        if (window.__restoreLayoutMocks) window.__restoreLayoutMocks();
        let originalAnimate = Element.prototype.animate;
        let originalRaf = window.requestAnimationFrame;
        window.__anims = [];
        window.requestAnimationFrame = (callback) => { queueMicrotask(() => callback(performance.now() + 1000)); return 0; };
        Element.prototype.animate = function (keyframes, options) {
            let resolve;
            let finished = new Promise((r) => { resolve = r; });
            let animation = {
                keyframes, options, currentTime: 0, playState: 'running', finished,
                pause() { this.playState = 'paused'; },
                play() { this.playState = 'running'; },
                commitStyles() {},
                cancel() { this.playState = 'idle'; resolve({}); },
            };
            window.__anims.push(animation);
            return animation;
        };
        window.__restoreLayoutMocks = () => {
            Element.prototype.animate = originalAnimate;
            window.requestAnimationFrame = originalRaf;
            window.__restoreLayoutMocks = null;
        };
    ");
    let container: web_sys::HtmlElement = web_sys::window().unwrap().document().unwrap().create_element("div").unwrap().unchecked_into();
    let root = create_root(&container);
    (container, root)
}

fn teardown() {
    js("if (window.__restoreLayoutMocks) window.__restoreLayoutMocks();");
}

type Rect = (f64, f64, f64, f64);

/// `mockBoundingRect` (one rect) and `mockBoundingRectSequence` (several).
fn mock_rects(node: &web_sys::Element, rects: &[Rect]) {
    let list = js_sys::Array::new();
    for &(left, top, right, bottom) in rects {
        list.push(&js_sys::Array::of4(&left.into(), &top.into(), &right.into(), &bottom.into()));
    }
    js_sys::Function::new_with_args(
        "el, rects",
        "let index = 0;
         el.getBoundingClientRect = () => {
             let [left, top, right, bottom] = rects[Math.min(index, rects.length - 1)];
             index++;
             return { left, top, right, bottom, width: right - left, height: bottom - top, x: left, y: top, toJSON() { return this } };
         };",
    )
    .call2(&JsValue::NULL, node, &list)
    .unwrap();
}

fn render(root: &Root, tick: u32, config: impl Into<LayoutConfig>) {
    root.render(rsx! { <div data-tick={tick.to_string()} mix={[animate_layout(config)]}></div> });
    root.flush();
}

fn clear_animations() {
    js("window.__anims = []");
}

fn animation_count() -> u32 {
    js("return window.__anims.length").as_f64().unwrap() as u32
}

fn animation(index: u32, path: &str) -> JsValue {
    js(&format!("return window.__anims[{index}].{path}"))
}

fn play_state(index: u32) -> String {
    animation(index, "playState").as_string().unwrap()
}

/// Renders ticks 0 and 1 at `start`, so the element has a settled layout and no animation.
fn settle(config: impl Fn() -> LayoutConfig, start: Rect) -> (web_sys::HtmlElement, Root, web_sys::Element) {
    let (container, root) = setup();
    render(&root, 0, config());
    let node = container.query_selector("div").unwrap().expect("rendered");
    mock_rects(&node, &[start]);
    render(&root, 1, config());
    clear_animations();
    (container, root, node)
}

const BOX: Rect = (0.0, 0.0, 100.0, 100.0);

fn default_layout() -> LayoutConfig {
    true.into()
}

// animate-layout-mixin.test.tsx: animates when layout geometry changes
#[wasm_bindgen_test]
fn animates_when_layout_geometry_changes() {
    let config = || LayoutConfig::from(Layout::new().duration(350.0).easing("linear"));
    let (_container, root, node) = settle(config, BOX);
    mock_rects(&node, &[BOX, (40.0, 10.0, 140.0, 110.0)]);
    render(&root, 2, config());

    assert_eq!(animation_count(), 1);
    assert_eq!(animation(0, "options.duration").as_f64(), Some(350.0));
    assert_eq!(animation(0, "options.easing").as_string().as_deref(), Some("linear"));
    teardown();
}

// animate-layout-mixin.test.tsx: can animate translation without scale projection
#[wasm_bindgen_test]
fn can_animate_translation_without_scale_projection() {
    let config = || LayoutConfig::from(Layout::new().size(false));
    let (_container, root, node) = settle(config, BOX);
    mock_rects(&node, &[BOX, (20.0, 30.0, 160.0, 170.0)]);
    render(&root, 2, config());

    assert_eq!(animation_count(), 1);
    let transform = animation(0, "keyframes[0].transform").as_string().unwrap();
    assert!(transform.contains("translate3d("), "{transform}");
    assert!(!transform.contains("scale("), "{transform}");
    teardown();
}

// animate-layout-mixin.test.tsx: does not invent translation for size-only changes when size projection is disabled
#[wasm_bindgen_test]
fn does_not_invent_translation_for_size_only_changes_without_size_projection() {
    let config = || LayoutConfig::from(Layout::new().size(false));
    let (_container, root, node) = settle(config, (0.0, 0.0, 100.0, 40.0));
    mock_rects(&node, &[(0.0, 0.0, 100.0, 40.0), (0.0, 0.0, 100.0, 120.0)]);
    render(&root, 2, config());

    assert_eq!(animation_count(), 0);
    teardown();
}

// animate-layout-mixin.test.tsx: does not animate when geometry does not change
#[wasm_bindgen_test]
fn does_not_animate_when_geometry_does_not_change() {
    let start = (5.0, 5.0, 105.0, 105.0);
    let (_container, root, node) = settle(default_layout, start);
    mock_rects(&node, &[start, start]);
    render(&root, 2, default_layout());

    assert_eq!(animation_count(), 0);
    teardown();
}

// animate-layout-mixin.test.tsx: starts interrupted layout animations from the current visual position
#[wasm_bindgen_test]
fn starts_interrupted_layout_animations_from_the_current_visual_position() {
    let (_container, root, node) = settle(default_layout, BOX);
    mock_rects(&node, &[BOX, (30.0, 0.0, 130.0, 100.0)]);
    render(&root, 2, default_layout());
    assert_eq!(play_state(0), "running");

    mock_rects(&node, &[(45.0, 0.0, 145.0, 100.0), (60.0, 0.0, 160.0, 100.0)]);
    render(&root, 3, default_layout());

    assert_eq!(play_state(0), "idle");
    assert_eq!(animation_count(), 2);
    assert_eq!(animation(1, "keyframes[0].transform").as_string().as_deref(), Some("translate3d(-15px, 0px, 0)"));
    teardown();
}

/// Like `mock_rects`, but the element reports `visual` while a projection transform is set
/// inline and its natural rects (a sequence) when the transform is cleared or `none`.
fn mock_projected_rects(node: &web_sys::Element, natural: &[Rect], visual: Rect) {
    let list = js_sys::Array::new();
    for &(left, top, right, bottom) in natural.iter().chain([visual].iter()) {
        list.push(&js_sys::Array::of4(&left.into(), &top.into(), &right.into(), &bottom.into()));
    }
    js_sys::Function::new_with_args(
        "el, rects",
        "let visual = rects.pop();
         let index = 0;
         let box = ([left, top, right, bottom]) => ({ left, top, right, bottom, width: right - left, height: bottom - top, x: left, y: top, toJSON() { return this } });
         el.getBoundingClientRect = () => {
             let transform = el.style.transform;
             if (transform && transform !== 'none') return box(visual);
             let next = rects[Math.min(index, rects.length - 1)];
             index++;
             return box(next);
         };",
    )
    .call2(&JsValue::NULL, node, &list)
    .unwrap();
}

// rmx: an interrupted animation's snapshot is the on-screen box (with its projection
// transform), not the natural box; Remix's sequence mocks cannot tell the two apart.
#[wasm_bindgen_test]
fn snapshots_the_on_screen_box_of_a_running_animation() {
    let (_container, root, node) = settle(default_layout, BOX);
    mock_projected_rects(&node, &[BOX, (30.0, 0.0, 130.0, 100.0)], BOX);
    render(&root, 2, default_layout());
    assert_eq!(animation_count(), 1);

    mock_projected_rects(&node, &[(60.0, 0.0, 160.0, 100.0)], (45.0, 0.0, 145.0, 100.0));
    render(&root, 3, default_layout());
    assert_eq!(animation_count(), 2);
    assert_eq!(animation(1, "keyframes[0].transform").as_string().as_deref(), Some("translate3d(-15px, 0px, 0)"));
    teardown();
}

// animate-layout-mixin.test.tsx: keeps active layout animations when the target geometry does not change
#[wasm_bindgen_test]
fn keeps_active_layout_animations_when_the_target_geometry_does_not_change() {
    let (_container, root, node) = settle(default_layout, BOX);
    mock_rects(&node, &[BOX, (30.0, 0.0, 130.0, 100.0)]);
    render(&root, 2, default_layout());
    assert_eq!(play_state(0), "running");

    mock_rects(&node, &[(45.0, 0.0, 145.0, 100.0), (30.0, 0.0, 130.0, 100.0)]);
    render(&root, 3, default_layout());

    assert_eq!(play_state(0), "running");
    assert_eq!(animation_count(), 1);
    teardown();
}

// animate-layout-mixin.test.tsx: does not reuse completed animation geometry when beforeUpdate is skipped
#[wasm_bindgen_test]
async fn does_not_reuse_completed_animation_geometry_when_before_update_is_skipped() {
    let (_container, root, node) = settle(default_layout, BOX);
    mock_rects(&node, &[BOX, (30.0, 0.0, 130.0, 100.0)]);
    render(&root, 2, default_layout());
    animation(0, "cancel()");
    let finished: js_sys::Promise = animation(0, "finished").unchecked_into();
    JsFuture::from(finished).await.unwrap();
    // Let the mixin's own continuation of `finished` run as well.
    JsFuture::from(js_sys::Promise::resolve(&JsValue::UNDEFINED)).await.unwrap();
    clear_animations();

    mock_rects(&node, &[(30.0, 0.0, 130.0, 100.0)]);
    render(&root, 3, false);

    mock_rects(&node, &[(80.0, 0.0, 180.0, 100.0)]);
    render(&root, 4, default_layout());

    assert_eq!(animation_count(), 0);
    teardown();
}

// animate-layout-mixin.test.tsx: cancels active animation on remove
#[wasm_bindgen_test]
fn cancels_active_animation_on_remove() {
    let (_container, root, node) = settle(default_layout, BOX);
    mock_rects(&node, &[BOX, (30.0, 0.0, 130.0, 100.0)]);
    render(&root, 2, default_layout());
    assert_eq!(play_state(0), "running");

    root.render(());
    root.flush();
    assert_eq!(play_state(0), "idle");
    teardown();
}
