//! The Remix bookstore demo in rmx.
//!
//! What maps to what:
//!
//! | Remix | rmx |
//! | --- | --- |
//! | `route({ ... })` with `form()` and `resources()` | [`routes::ROUTES`] with `form()` and `resources_with_param()` |
//! | `formData({ uploadHandler })` + `createFsFileStorage` | `form_data().upload_handler(..)` + `FsFileStorage` |
//! | `createFileResponse(file, request)` | `file_response_for(&file, method, headers, &options)` |
//! | `methodOverride()` and `RestfulForm` | `method_override()` and [`server::ui::RestfulForm`] |
//! | `createCredentialsAuthProvider` / `verifyCredentials` | `credentials_provider` / `verify_credentials` |
//! | `asyncContext()` + `getContext()` in the layout | a `Viewer` provided to the tree by `server::render` |
//! | `<Frame src>` fragments and `handle.frame.reload()` | the same |
//! | SQLite through `remix/data-table` | an in-memory `server::data::Db` with the same rules |
//!
//! This module holds the shared route map and cart, and the client entries
//! (`CartButton`, `CartItems`, `ImageCarousel`); [`server`] has the rest.

pub mod cart;
pub mod routes;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;

use rmx::{browser, css, on, rsx, Component, Handle, Mixin, Node, ReloadOptions};
use serde::{Deserialize, Serialize};

use cart::{money, CartItem};
use routes::ROUTES;

// ---------------------------------------------------------------------------------------
// actions/fragments/public/cart-button.tsx
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CartButtonProps {
    pub in_cart: bool,
    pub id: i64,
    pub slug: String,
}

/// Toggles a book in the cart, then reloads its own frame to show the new state.
pub struct CartButton {
    pending: bool,
}

impl Component for CartButton {
    type Props = CartButtonProps;
    rmx::client_entry!("CartButton");

    fn setup(_: &CartButtonProps, _: &Handle<Self>) -> Self {
        CartButton { pending: false }
    }

    fn render(&self, props: &CartButtonProps, h: &Handle<Self>) -> Node {
        let fields = vec![("bookId".to_string(), props.id.to_string()), ("slug".to_string(), props.slug.clone())];
        let toggle = h.on_event(move |this, h, event| {
            this.pending = true;
            h.update();
            let (h, signal, fields) = (h.clone(), event.signal(), fields.clone());
            h.clone().spawn(async move {
                let _ = browser::fetch_form("POST", &ROUTES.api.cart_toggle.href(()), &fields, Some(&signal)).await;
                let _ = h.frame().reload_async(ReloadOptions::default()).await;
                browser::sleep(500).await;
                if signal.aborted() {
                    return;
                }
                h.with(|this, h| {
                    this.pending = false;
                    h.update();
                });
            });
        });
        let label = if self.pending {
            "Saving..."
        } else if props.in_cart {
            "Remove from Cart"
        } else {
            "Add to Cart"
        };
        rsx! { <button type="button" class="btn" mix={on("click", toggle)}>{label}</button> }
    }
}

// ---------------------------------------------------------------------------------------
// actions/fragments/public/cart-items.tsx
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CartItemsProps {
    pub items: Vec<CartItem>,
    pub total: f64,
    pub can_checkout: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum PendingAction {
    Update(i64),
    Remove(i64),
}

/// The cart table. Its forms work without JavaScript; once hydrated they post in the
/// background and reload the cart frame.
pub struct CartItems {
    pending: Option<PendingAction>,
}

impl CartItems {
    /// A form submit handler that posts the form with `redirect=none`, then reloads the frame.
    fn submit(h: &Handle<Self>, action: PendingAction) -> rmx::EventHandler {
        h.on_event(move |this, h, event| {
            event.prevent_default();
            if this.pending.is_some() {
                return;
            }
            // Read the fields before the update disables the inputs.
            let mut fields: Vec<(String, String)> = event.form_fields().into_iter().filter(|(k, _)| k != "redirect").collect();
            fields.push(("redirect".into(), "none".into()));
            let url = event.current_attr("action").unwrap_or_default();
            this.pending = Some(action);
            h.update();
            let (h, signal) = (h.clone(), event.signal());
            h.clone().spawn(async move {
                let posted = browser::fetch_form("POST", &url, &fields, Some(&signal)).await;
                if posted.is_ok() && !signal.aborted() {
                    let _ = h.frame().reload_async(ReloadOptions::default()).await;
                }
                h.with(|this, h| {
                    this.pending = None;
                    h.update();
                });
            });
        })
    }
}

fn small_button(min_width: &str) -> Mixin {
    css(&format!("font-size: 0.875rem; padding: 0.25rem 0.5rem; min-width: {min_width}; text-align: center;"))
}

impl Component for CartItems {
    type Props = CartItemsProps;
    rmx::client_entry!("CartItems");

    fn setup(_: &CartItemsProps, _: &Handle<Self>) -> Self {
        CartItems { pending: None }
    }

    fn render(&self, props: &CartItemsProps, h: &Handle<Self>) -> Node {
        let is_pending = self.pending.is_some();
        let total_label = if is_pending { "---".to_string() } else { money(props.total) };
        let rows: Vec<Node> = props
            .items
            .iter()
            .map(|item| {
                let updating = self.pending == Some(PendingAction::Update(item.book_id));
                let removing = self.pending == Some(PendingAction::Remove(item.book_id));
                rsx! {
                    <tr key={item.book_id}>
                        <td><a href={ROUTES.books.show.href([("slug", item.slug.as_str())])}>{item.title.clone()}</a></td>
                        <td>{money(item.price)}</td>
                        <td>
                            <form method="POST" action={ROUTES.cart.api.update.href(())}
                                mix={[on("submit", CartItems::submit(h, PendingAction::Update(item.book_id))), css("display: inline-flex; gap: 0.5rem; align-items: center;")]}>
                                <input type="hidden" name="_method" value="PUT"/>
                                <input type="hidden" name="bookId" value={item.book_id.to_string()}/>
                                <input type="number" name="quantity" defaultValue={item.quantity.to_string()} min="1" disabled={is_pending} mix={css("width: 70px;")}/>
                                <button type="submit" disabled={is_pending} class="btn btn-secondary" mix={small_button("6.25rem")}>
                                    {if updating { "Saving..." } else { "Update" }}
                                </button>
                            </form>
                        </td>
                        <td>{money(item.price * item.quantity as f64)}</td>
                        <td>
                            <form method="POST" action={ROUTES.cart.api.remove.href(())}
                                mix={[on("submit", CartItems::submit(h, PendingAction::Remove(item.book_id))), css("display: inline;")]}>
                                <input type="hidden" name="_method" value="DELETE"/>
                                <input type="hidden" name="bookId" value={item.book_id.to_string()}/>
                                <button type="submit" disabled={is_pending} class="btn btn-danger" mix={small_button("7rem")}>
                                    {if removing { "Removing..." } else { "Remove" }}
                                </button>
                            </form>
                        </td>
                    </tr>
                }
            })
            .collect();
        let checkout = if props.can_checkout {
            rsx! { <a href={ROUTES.checkout.index.href(())} class="btn">"Proceed to Checkout"</a> }
        } else {
            rsx! { <a href={ROUTES.auth.login.index.href(())} class="btn">"Login to Checkout"</a> }
        };
        rsx! {
            <>
                {is_pending.then(|| rsx! { <p mix={css("margin-bottom: 1rem; font-size: 0.9rem; color: #666;")}>"Updating your cart..."</p> })}
                <table>
                    <thead>
                        <tr><th>"Book"</th><th>"Price"</th><th>"Quantity"</th><th>"Subtotal"</th><th>"Actions"</th></tr>
                    </thead>
                    <tbody>{rows}</tbody>
                </table>
                <div mix={css("margin-top: 2rem; display: flex; align-items: center; gap: 1rem;")}>
                    <p mix={css("margin: 0; font-size: 1.25rem; font-weight: bold; margin-right: auto;")}>{format!("Total: {total_label}")}</p>
                    <a href={ROUTES.books.index.href(())} class="btn btn-secondary">"Continue Shopping"</a>
                    {checkout}
                </div>
            </>
        }
    }
}

// ---------------------------------------------------------------------------------------
// actions/books/public/image-carousel.tsx
// ---------------------------------------------------------------------------------------

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageCarouselProps {
    pub images: Vec<String>,
    pub start_index: Option<usize>,
}

pub struct ImageCarousel {
    index: usize,
}

fn carousel_button() -> Mixin {
    css("position: absolute; top: 50%; transform: translateY(-50%); width: 40px; height: 40px; display: flex; align-items: center; justify-content: center; background: transparent; color: white; border: none; border-radius: 999px; cursor: pointer; outline: none; transition: background-color 150ms ease, opacity 150ms ease; opacity: 0.9; &:disabled { opacity: 0.4; }")
}

impl Component for ImageCarousel {
    type Props = ImageCarouselProps;
    rmx::client_entry!("ImageCarousel");

    fn setup(props: &ImageCarouselProps, _: &Handle<Self>) -> Self {
        ImageCarousel { index: props.start_index.unwrap_or(0) }
    }

    fn render(&self, props: &ImageCarouselProps, h: &Handle<Self>) -> Node {
        let total = props.images.len();
        if total == 0 {
            return rsx! { <></> };
        }
        let index = self.index.min(total - 1);
        let slides: Vec<Node> = props
            .images
            .iter()
            .enumerate()
            .map(|(i, src)| {
                rsx! {
                    <div key={format!("{src}{i}")} mix={css("min-width: 100%; height: 100%; position: relative;")}>
                        <img src={src.clone()} alt={format!("Image {} of {total}", i + 1)} draggable="false"
                            mix={css("width: 100%; height: 100%; object-fit: cover; display: block; user-select: none; pointer-events: none;")}/>
                    </div>
                }
            })
            .collect();
        let prev = h.on(|this, h| {
            if this.index > 0 {
                this.index -= 1;
                h.update();
            }
        });
        let next = h.on(move |this, h| {
            if this.index + 1 < total {
                this.index += 1;
                h.update();
            }
        });
        rsx! {
            <div mix={css("position: relative; width: 100%; height: 100%; overflow: hidden; background-color: #f5f5f5;")}>
                <div mix={css("display: flex; height: 100%; width: 100%; transition: transform 350ms cubic-bezier(0.22, 1, 0.36, 1); will-change: transform;")}
                    style={format!("transform: translateX(-{}%)", index * 100)}>
                    {slides}
                </div>
                <button aria-label="Previous image" disabled={index == 0} mix={[on("click", prev), carousel_button(), css("left: 8px;")]}>
                    <span mix={css("font-size: 22px; line-height: 1;")}>"\u{2039}"</span>
                </button>
                <button aria-label="Next image" disabled={index + 1 == total} mix={[on("click", next), carousel_button(), css("right: 8px;")]}>
                    <span mix={css("font-size: 22px; line-height: 1;")}>"\u{203a}"</span>
                </button>
            </div>
        }
    }
}

/// Browser entry: hydrate the client entries and adopt the frames.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    rmx::run(rmx::RunOptions {
        entries: vec![rmx::entry::<CartButton>(), rmx::entry::<CartItems>(), rmx::entry::<ImageCarousel>()],
        ..Default::default()
    });
}
