//! The Remix i18n demo in rmx: request-scoped translation, `Accept-Language` negotiation,
//! a locale preference cookie, plural rules, `Intl` formatting and right-to-left layout.
//!
//! What maps to what:
//!
//! | Remix | rmx |
//! | --- | --- |
//! | i18next with request-local instances | [`i18n::Translator`] over `locales/*.json` |
//! | Node's `Intl` on the server | ICU4X ([`intl`]) |
//! | the browser's `Intl` in client entries | the browser's `Intl` through js-sys ([`format`]) |
//! | `AcceptLanguage.from(..).getPreferred(..)` | `AcceptLanguage::parse(..).preferred(..)` |
//! | `handle.context.set(cart)` / `.get(CartPreview)` | `h.set_context(cart)` / `h.context::<Cart>()` |
//!
//! This module holds the two client entries (`actions/public/*.tsx`); [`server`] has the rest.

pub mod format;
pub mod i18n;
#[cfg(not(target_arch = "wasm32"))]
pub mod intl;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;

use std::cell::Cell;
use std::rc::Rc;

use rmx::{css, on, rsx, Component, Handle, Mixin, Node};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use i18n::{Language, Translator};

// ---------------------------------------------------------------------------------------
// actions/public/cart-preview.tsx
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct CartPreviewProps {
    pub locale: Language,
    /// The `pluralization` section of the active language's table (`t.get('pluralization')`).
    pub translations: Value,
}

/// `CartContext`: the cart state the provider shares with its descendants. Setting the
/// quantity updates the provider, which renders the whole cart again.
#[derive(Clone)]
pub struct Cart(Rc<CartState>);

struct CartState {
    interactive: Cell<bool>,
    quantity: Cell<u64>,
    locale: Language,
    resources: Value,
    provider: Handle<CartPreview>,
}

impl Cart {
    pub fn interactive(&self) -> bool {
        self.0.interactive.get()
    }

    pub fn quantity(&self) -> u64 {
        self.0.quantity.get()
    }

    /// The cart's own translator: only its section, with no fallback language.
    pub fn t(&self) -> Translator<'_> {
        Translator::new(self.0.locale, &self.0.resources, None)
    }

    pub fn set_quantity(&self, next: f64) {
        let next = if next.is_finite() { next.trunc().max(0.0) as u64 } else { 0 };
        if next == self.quantity() {
            return;
        }
        self.0.quantity.set(next);
        self.0.provider.update();
    }
}

pub struct CartPreview {
    cart: Cart,
}

impl Component for CartPreview {
    type Props = CartPreviewProps;
    rmx::client_entry!("CartPreview");

    fn setup(props: &CartPreviewProps, h: &Handle<Self>) -> Self {
        let cart = Cart(Rc::new(CartState {
            interactive: Cell::new(false),
            quantity: Cell::new(0),
            locale: props.locale,
            resources: serde_json::json!({ "pluralization": props.translations }),
            provider: h.clone(),
        }));
        h.set_context(cart.clone());
        h.queue_task(|this, h| {
            this.cart.0.interactive.set(true);
            h.update();
        });
        CartPreview { cart }
    }

    fn render(&self, _: &CartPreviewProps, _: &Handle<Self>) -> Node {
        rsx! {
            <div mix={cart_style()}>
                <CartHeading/>
                <CartQuantity/>
                <CartSummary/>
            </div>
        }
    }
}

fn cart_context<C: Component>(h: &Handle<C>) -> Rc<Cart> {
    h.context::<Cart>().expect("rendered inside CartPreview")
}

pub struct CartHeading {
    cart: Rc<Cart>,
}

impl Component for CartHeading {
    type Props = ();

    fn setup(_: &(), h: &Handle<Self>) -> Self {
        CartHeading { cart: cart_context(h) }
    }

    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        rsx! { <h3 mix={heading_style()}>{self.cart.t().t("pluralization.cart_demo.title")}</h3> }
    }
}

pub struct CartQuantity {
    cart: Rc<Cart>,
}

impl Component for CartQuantity {
    type Props = ();

    fn setup(_: &(), h: &Handle<Self>) -> Self {
        CartQuantity { cart: cart_context(h) }
    }

    fn render(&self, _: &(), h: &Handle<Self>) -> Node {
        let cart = &self.cart;
        let t = cart.t();
        let (interactive, quantity) = (cart.interactive(), cart.quantity());
        rsx! {
            <div mix={quantity_style()}>
                <button type="button" aria-label={t.t("pluralization.cart_demo.decrease")} disabled={!interactive || quantity == 0}
                    mix={[quantity_button_style(), on("click", h.on(|this, _| this.cart.set_quantity(this.cart.quantity() as f64 - 1.0)))]}>
                    "−"
                </button>
                <label mix={quantity_label_style()}>
                    <span>{t.t("pluralization.cart_demo.quantity_label")}</span>
                    <input type="number" min="0" step="1" value={quantity.to_string()} disabled={!interactive}
                        mix={[quantity_input_style(), on("input", h.on_event(|this, _, event| {
                            // `valueAsNumber`: NaN for an empty or invalid entry.
                            this.cart.set_quantity(event.target_value().trim().parse().unwrap_or(f64::NAN));
                        }))]}/>
                </label>
                <button type="button" aria-label={t.t("pluralization.cart_demo.increase")} disabled={!interactive}
                    mix={[quantity_button_style(), on("click", h.on(|this, _| this.cart.set_quantity(this.cart.quantity() as f64 + 1.0)))]}>
                    "+"
                </button>
            </div>
        }
    }
}

pub struct CartSummary {
    cart: Rc<Cart>,
}

impl Component for CartSummary {
    type Props = ();

    fn setup(_: &(), h: &Handle<Self>) -> Self {
        CartSummary { cart: cart_context(h) }
    }

    fn render(&self, _: &(), _: &Handle<Self>) -> Node {
        let t = self.cart.t();
        rsx! {
            <output aria-live="polite" aria-label={t.t("pluralization.cart_demo.summary_label")}>
                {t.t_count("pluralization.cart", self.cart.quantity())}
            </output>
        }
    }
}

fn cart_style() -> Mixin {
    css("display: grid; gap: 16px; margin-block-start: 20px; padding: 18px; border: 1px solid light-dark(#d7dde5, #343b45); border-radius: 10px; background-color: light-dark(#f8fafc, #171b21);")
}

fn heading_style() -> Mixin {
    css("margin: 0; color: light-dark(#111827, #f3f4f6); font-size: 16px;")
}

fn quantity_style() -> Mixin {
    css("display: flex; align-items: end; gap: 10px;")
}

fn quantity_label_style() -> Mixin {
    css("display: grid; gap: 6px; color: light-dark(#4b5563, #b4b8c0); font-size: 13px; font-weight: 600;")
}

fn quantity_input_style() -> Mixin {
    css("inline-size: 88px; min-height: 38px; padding-inline: 10px; border: 1px solid light-dark(#c7ced8, #4b5563); border-radius: 8px; background-color: light-dark(#ffffff, #11151a); color: inherit; font: inherit; text-align: center;")
}

fn quantity_button_style() -> Mixin {
    css("inline-size: 38px; min-height: 38px; border: 1px solid currentColor; border-radius: 8px; background: transparent; color: inherit; font: inherit; font-size: 20px; cursor: pointer; &:focus-visible { outline: 2px solid light-dark(#0578be, #2dacf9); outline-offset: 2px; } &:disabled { opacity: 0.45; cursor: default; }")
}

// ---------------------------------------------------------------------------------------
// actions/public/number-preview.tsx
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberPreviewProps {
    pub locale: Language,
    pub button_label: String,
    pub value_label: String,
}

/// Receives translated labels and a locale, not a translator, and formats with the
/// browser's `Intl` without a server request.
pub struct NumberPreview {
    value: i64,
    interactive: bool,
}

impl Component for NumberPreview {
    type Props = NumberPreviewProps;
    rmx::client_entry!("NumberPreview");

    fn setup(_: &NumberPreviewProps, h: &Handle<Self>) -> Self {
        h.queue_task(|this, h| {
            this.interactive = true;
            h.update();
        });
        NumberPreview { value: 1_250_000, interactive: false }
    }

    fn render(&self, props: &NumberPreviewProps, h: &Handle<Self>) -> Node {
        rsx! {
            <div mix={preview_style()}>
                <button type="button" disabled={!self.interactive} mix={[button_style(), on("click", h.on(|this, h| {
                    this.value += 1_000;
                    h.update();
                }))]}>
                    {props.button_label.clone()}
                </button>
                <output aria-label={props.value_label.clone()} aria-live="polite">
                    <bdi>{format::format_number(props.locale.code(), self.value)}</bdi>
                </output>
            </div>
        }
    }
}

fn preview_style() -> Mixin {
    css("display: flex; align-items: center; flex-wrap: wrap; gap: 12px;")
}

fn button_style() -> Mixin {
    css("min-height: 36px; padding: 4px 12px; border: 1px solid currentColor; border-radius: 8px; background: transparent; color: inherit; font: inherit; cursor: pointer; &:focus-visible { outline: 2px solid light-dark(#0578be, #2dacf9); outline-offset: 2px; } &:disabled { opacity: 0.5; cursor: default; }")
}

/// Browser entry: hydrate the client entries.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    rmx::run(rmx::RunOptions { entries: vec![rmx::entry::<CartPreview>(), rmx::entry::<NumberPreview>()], ..Default::default() });
}
