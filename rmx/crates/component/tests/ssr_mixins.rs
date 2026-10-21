//! Ports of the server cases of Remix's `mixin-ssr.test.tsx`. A Remix `createMixin(setup,
//! { ssr })` factory is `mixin_with::<M>(args, MixinOptions { ssr })` here, with the
//! default taken from `HostMixin::SSR`. Remix's `createMixin(browser().type)` makes a new
//! factory whose default is server rendering; in rmx the default belongs to the type, so
//! reusing a browser-only type without options stays browser-only.

extern crate rmx_component as rmx;

use std::cell::{Cell, RefCell};

use rmx::{mixin, mixin_with, render_to_string, rsx, AttrValue, Element, HostMixin, Mixin, MixinHandle, MixinOptions};

const BROWSER: MixinOptions = MixinOptions { ssr: Some(false) };
const SERVER: MixinOptions = MixinOptions { ssr: Some(true) };

thread_local! {
    static SETUPS: Cell<u32> = const { Cell::new(0) };
    static RENDERED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn setups() -> u32 {
    SETUPS.with(Cell::get)
}

fn count_setup() {
    SETUPS.with(|s| s.set(s.get() + 1));
}

/// Sets `title`, counting setups and recording each render.
struct Title;

impl HostMixin for Title {
    type Args = &'static str;

    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        count_setup();
        Title
    }

    fn render(&mut self, title: &&'static str, host: &mut Element, _: &MixinHandle<Self>) {
        RENDERED.with(|r| r.borrow_mut().push(title.to_string()));
        host.set_attr("title", *title);
    }
}

/// A browser-only mixin whose server render would be a bug.
struct BrowserOnly;

impl HostMixin for BrowserOnly {
    type Args = ();
    const SSR: bool = false;

    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        count_setup();
        BrowserOnly
    }

    fn render(&mut self, _: &(), _: &mut Element, _: &MixinHandle<Self>) {
        panic!("browser render should be skipped");
    }
}

fn browser() -> Mixin {
    mixin::<BrowserOnly>(())
}

/// A browser-only type that sets `title="enabled"` when the server runs it.
struct Enabled;

impl HostMixin for Enabled {
    type Args = ();
    const SSR: bool = false;

    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        count_setup();
        Enabled
    }

    fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
        host.set_attr("title", "enabled");
    }
}

/// Appends its argument to the current `title`.
struct Suffix;

impl HostMixin for Suffix {
    type Args = &'static str;

    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        Suffix
    }

    fn render(&mut self, value: &&'static str, host: &mut Element, _: &MixinHandle<Self>) {
        let title = match host.attr("title") {
            Some(AttrValue::Str(s)) => s.to_string(),
            _ => String::new(),
        };
        host.set_attr("title", format!("{title}{value}"));
    }
}

#[test]
fn keeps_options_separate_for_factories_using_the_same_setup_function() {
    let html = render_to_string(&rsx! {
        <>
            <div mix={mixin::<Title>("default")}></div>
            <div mix={mixin_with::<Title>("browser", BROWSER)}></div>
            <div mix={mixin_with::<Title>("enabled", SERVER)}></div>
        </>
    });
    assert_eq!(html, r#"<div title="default"></div><div></div><div title="enabled"></div>"#);
    assert_eq!(setups(), 2);
    assert_eq!(RENDERED.with(|r| r.borrow().clone()), ["default", "enabled"]);
}

#[test]
fn reads_options_when_the_factory_is_created() {
    let mut options = BROWSER;
    let browser = mixin_with::<Title>("browser", options);
    options.ssr = Some(true);
    assert_eq!(options, SERVER);
    assert_eq!(render_to_string(&rsx! { <div mix={browser}></div> }), "<div></div>");
    assert_eq!(setups(), 0);
}

#[test]
fn can_enable_server_rendering_when_reusing_a_browser_only_type() {
    let enabled = mixin_with::<Enabled>((), SERVER);
    assert_eq!(render_to_string(&rsx! { <div mix={enabled}></div> }), r#"<div title="enabled"></div>"#);
    assert_eq!(render_to_string(&rsx! { <div mix={mixin::<Enabled>(())}></div> }), "<div></div>");
    let default = mixin_with::<Enabled>((), MixinOptions::default());
    assert_eq!(render_to_string(&rsx! { <div mix={default}></div> }), "<div></div>");
    assert_eq!(setups(), 1);
}

#[test]
fn preserves_host_content_with_conditional_and_nested_browser_only_mixins() {
    let mixins: Vec<Option<Vec<Option<Mixin>>>> = vec![None, Some(vec![Some(browser()), None, Some(browser())])];
    let html = render_to_string(&rsx! {
        <div id="host" title="unchanged" mix={mixins}>"Child content"</div>
    });
    assert_eq!(html, r#"<div id="host" title="unchanged">Child content</div>"#);
    assert_eq!(setups(), 0);
}

#[test]
fn keeps_server_mixins_in_order_around_browser_only_mixins() {
    let html = render_to_string(&rsx! {
        <div mix={[browser(), mixin::<Title>("hello"), browser(), mixin::<Suffix>("-world"), browser()]}></div>
    });
    assert_eq!(html, r#"<div title="hello-world"></div>"#);
    assert_eq!(setups(), 1, "only the server mixin ran its setup");
}

/// A server mixin that adds browser-only mixins (and a server one) to its host.
struct Nested;

impl HostMixin for Nested {
    type Args = ();

    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        Nested
    }

    fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
        host.mix(vec![Some(vec![browser()]), None, Some(vec![mixin::<Title>("nested"), browser()])]);
    }
}

#[test]
fn skips_browser_only_descriptors_returned_by_server_mixins() {
    assert_eq!(render_to_string(&rsx! { <div mix={mixin::<Nested>(())}></div> }), r#"<div title="nested"></div>"#);
    assert_eq!(setups(), 1, "only the nested server mixin ran its setup");
}

/// Sets a title and adds a browser-only mixin while rendering the host.
struct AddsBrowser;

impl HostMixin for AddsBrowser {
    type Args = ();

    fn setup(_: &MixinHandle<Self>, _: &'static str) -> Self {
        AddsBrowser
    }

    fn render(&mut self, _: &(), host: &mut Element, _: &MixinHandle<Self>) {
        host.set_attr("title", "nested");
        host.mix(browser());
    }
}

#[test]
fn skips_browser_only_mixins_added_while_rendering_the_host() {
    assert_eq!(render_to_string(&rsx! { <div mix={mixin::<AddsBrowser>(())}></div> }), r#"<div title="nested"></div>"#);
    assert_eq!(setups(), 0);
}
