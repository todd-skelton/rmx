//! `actions/cart`: the cart page (a frame around the cart items fragment) and the cart API.

use std::time::Duration;

use http::StatusCode;
use rmx::{rsx, Frame};
use rmx::fetch::Response;
use rmx::response;
use rmx::data_schema::{self as s, form_data as f};
use rmx::middleware::session::SessionExt;
use rmx::router::{action, Context, Router};

use super::schemas::{optional_text, parse_form};
use super::ui::Layout;
use super::{bad_request, current_cart, db, no_content, parse_id, render};
use crate::routes::{CartActions, CartApiActions, ROUTES};

pub fn routes(router: &mut Router, delay: Duration) {
    let r = &ROUTES.cart;
    router.map(
        r,
        CartActions {
            index: action(|ctx: Context| async move {
                let page = rsx! {
                    <Layout>
                        <h1>"Shopping Cart"</h1>
                        <div class="card"><Frame name="cart" src={ROUTES.fragments.cart_items.href(())}/></div>
                    </Layout>
                };
                render(&ctx, page)
            }),
        },
    );
    router.map(
        &r.api,
        CartApiActions {
            add: action(move |ctx: Context| change(ctx, delay, Change::Add)),
            update: action(move |ctx: Context| change(ctx, delay, Change::Update)),
            remove: action(move |ctx: Context| change(ctx, delay, Change::Remove)),
        },
    );
}

#[derive(Clone, Copy)]
enum Change {
    Add,
    Update,
    Remove,
}

struct CartForm {
    book_id: Option<String>,
    quantity: String,
    redirect: Option<String>,
}

async fn change(ctx: Context, delay: Duration, change: Change) -> Response {
    // A pause so the client entries can show pending states.
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    // `cartActionSchema` and `cartUpdateSchema`
    let schema = f::object(|o| CartForm {
        book_id: o.field("bookId", optional_text()),
        quantity: o.field("quantity", s::defaulted(s::string(), "1".to_string())),
        redirect: o.field("redirect", optional_text()),
    });
    let form = match parse_form(&ctx, schema) {
        Ok(form) => form,
        Err(issues) => return bad_request(issues),
    };
    let Some(book) = parse_id(form.book_id.as_deref()).and_then(|id| db(&ctx).find_book(id)) else {
        return response::status(StatusCode::NOT_FOUND, "Book not found");
    };
    let mut cart = current_cart(&ctx);
    match change {
        Change::Add => cart.add(book.id, &book.slug, &book.title, book.price, 1),
        Change::Update => {
            // parseInt(quantity, 10), defaulting to 1
            let quantity = form.quantity.trim();
            let digits: String = quantity.chars().enumerate().take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && *c == '-')).map(|(_, c)| c).collect();
            let quantity = digits.parse().unwrap_or(0);
            cart.update(book.id, quantity);
        }
        Change::Remove => cart.remove(book.id),
    }
    ctx.session().set("cart", &cart);
    if form.redirect.as_deref() == Some("none") {
        return no_content();
    }
    response::redirect(&ROUTES.cart.index.href(()))
}
