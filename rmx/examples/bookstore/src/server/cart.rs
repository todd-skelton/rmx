//! `actions/cart`: the cart page (a frame around the cart items fragment) and the cart API.

use std::time::Duration;

use http::StatusCode;
use rmx::{rsx, Frame};
use rmx_fetch::{response, Response};
use rmx_middleware::form_data::FormDataExt;
use rmx_middleware::session::SessionExt;
use rmx_router::{Context, Router};

use super::ui::Layout;
use super::{current_cart, db, no_content, parse_id, render};
use crate::routes::ROUTES;

pub fn routes(router: &mut Router, delay: Duration) {
    let r = &ROUTES.cart;
    router.route(&r.index, |ctx: Context| async move {
        let page = rsx! {
            <Layout>
                <h1>"Shopping Cart"</h1>
                <div class="card"><Frame name="cart" src={ROUTES.fragments.cart_items.href(())}/></div>
            </Layout>
        };
        render(&ctx, page)
    });
    router.route(&r.api.add, move |ctx: Context| change(ctx, delay, Change::Add));
    router.route(&r.api.update, move |ctx: Context| change(ctx, delay, Change::Update));
    router.route(&r.api.remove, move |ctx: Context| change(ctx, delay, Change::Remove));
}

#[derive(Clone, Copy)]
enum Change {
    Add,
    Update,
    Remove,
}

async fn change(ctx: Context, delay: Duration, change: Change) -> Response {
    // A pause so the client entries can show pending states.
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    let form = ctx.form_data();
    let Some(book) = parse_id(form.get("bookId")).and_then(|id| db(&ctx).find_book(id)) else {
        return response::status(StatusCode::NOT_FOUND, "Book not found");
    };
    let mut cart = current_cart(&ctx);
    match change {
        Change::Add => cart.add(book.id, &book.slug, &book.title, book.price, 1),
        Change::Update => {
            // parseInt(quantity, 10), defaulting to 1
            let quantity = form.get("quantity").unwrap_or("1").trim();
            let digits: String = quantity.chars().enumerate().take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && *c == '-')).map(|(_, c)| c).collect();
            let quantity = digits.parse().unwrap_or(0);
            cart.update(book.id, quantity);
        }
        Change::Remove => cart.remove(book.id),
    }
    ctx.session().set("cart", &cart);
    if form.get("redirect") == Some("none") {
        return no_content();
    }
    response::redirect(&ROUTES.cart.index.href(()))
}
