//! `actions/account` (profile, settings, orders) and `actions/checkout`, all behind
//! `requireAuth()`.

use rmx::{css, rsx, Node};
use rmx::fetch::{response, Response};
use rmx::middleware::form_data::FormDataExt;
use rmx::middleware::session::SessionExt;
use rmx::router::{middleware, Context, Router};

use super::data::{hash_password, locale_date, Order, ShippingAddress, UserInput};
use super::ui::{Layout, RestfulForm};
use super::{bad_request, current_cart, db, not_found_page, parse_id, render, require_auth, user};
use crate::cart::{money, Cart};
use crate::routes::ROUTES;

pub fn routes(router: &mut Router) {
    router.group(vec![middleware(require_auth())], |r| {
        let a = &ROUTES.account;
        r.route(&a.index, account);
        r.route(&a.settings.index, settings);
        r.route(&a.settings.update, update_settings);
        r.route(&a.orders.index, orders);
        r.route(&a.orders.show, order);
        let c = &ROUTES.checkout;
        r.route(&c.index, checkout);
        r.route(&c.action, place_order);
        r.route(&c.confirmation, confirmation);
    });
}

async fn account(ctx: Context) -> Response {
    let user = user(&ctx);
    let page = rsx! {
        <Layout>
            <h1>"My Account"</h1>
            <div class="card">
                <h2>"Account Information"</h2>
                <p><strong>"Name:"</strong>{format!(" {}", user.name)}</p>
                <p><strong>"Email:"</strong>{format!(" {}", user.email)}</p>
                <p><strong>"Role:"</strong>{format!(" {}", user.role.as_str())}</p>
                <p><strong>"Member Since:"</strong>{format!(" {}", locale_date(user.created_at))}</p>
                <p mix={css("margin-top: 1.5rem;")}><a href={ROUTES.account.settings.index.href(())} class="btn">"Edit Settings"</a></p>
            </div>
            <div class="card" mix={css("margin-top: 1.5rem;")}>
                <h2>"Quick Links"</h2>
                <p>
                    <a href={ROUTES.account.orders.index.href(())} class="btn btn-secondary">"View Orders"</a>
                    <a href={ROUTES.books.index.href(())} class="btn btn-secondary" mix={css("margin-left: 0.5rem;")}>"Browse Books"</a>
                </p>
            </div>
        </Layout>
    };
    render(&ctx, page)
}

async fn settings(ctx: Context) -> Response {
    let user = user(&ctx);
    let page = rsx! {
        <Layout>
            <h1>"Account Settings"</h1>
            <div class="card">
                <RestfulForm method="PUT" action={ROUTES.account.settings.update.href(())}>
                    <div class="form-group">
                        <label for="name">"Name"</label>
                        <input type="text" id="name" name="name" value={user.name.clone()} required/>
                    </div>
                    <div class="form-group">
                        <label for="email">"Email"</label>
                        <input type="email" id="email" name="email" value={user.email.clone()} required/>
                    </div>
                    <div class="form-group">
                        <label for="password">"New Password (leave blank to keep current)"</label>
                        <input type="password" id="password" name="password" autocomplete="new-password"/>
                    </div>
                    <button type="submit" class="btn">"Update Settings"</button>
                    <a href={ROUTES.account.index.href(())} class="btn btn-secondary" mix={css("margin-left: 0.5rem;")}>"Cancel"</a>
                </RestfulForm>
            </div>
        </Layout>
    };
    render(&ctx, page)
}

async fn update_settings(ctx: Context) -> Response {
    let form = ctx.form_data();
    // s.union([s.literal(''), s.string().pipe(minLength(8))])
    let password = form.get("password").unwrap_or_default();
    if !password.is_empty() && password.chars().count() < 8 {
        return bad_request("password must be empty or at least 8 characters");
    }
    let update = UserInput {
        name: Some(form.get("name").unwrap_or_default().into()),
        email: Some(form.get("email").unwrap_or_default().into()),
        password_hash: (!password.is_empty()).then(|| hash_password(password)),
        role: None,
    };
    if let Err(e) = db(&ctx).update_user(user(&ctx).id, update) {
        return bad_request(e.to_string());
    }
    response::redirect(&ROUTES.account.index.href(()))
}

// --- orders -----------------------------------------------------------------------------

/// The orders table shared by the account and admin order lists.
pub fn orders_table(orders: &[Order], show_href: impl Fn(&Order) -> String) -> Node {
    let rows: Vec<Node> = orders
        .iter()
        .map(|order| {
            rsx! {
                <tr>
                    <td>{format!("#{}", order.id)}</td>
                    <td>{locale_date(order.created_at)}</td>
                    <td>{format!("{} item(s)", order.items.len())}</td>
                    <td>{money(order.total)}</td>
                    <td><span class="badge badge-info">{order.status.as_str()}</span></td>
                    <td>
                        <a href={show_href(order)} class="btn btn-secondary" mix={css("font-size: 0.875rem; padding: 0.25rem 0.5rem;")}>"View"</a>
                    </td>
                </tr>
            }
        })
        .collect();
    rsx! {
        <table>
            <thead>
                <tr><th>"Order ID"</th><th>"Date"</th><th>"Items"</th><th>"Total"</th><th>"Status"</th><th>"Actions"</th></tr>
            </thead>
            <tbody>{rows}</tbody>
        </table>
    }
}

/// The order details shared by the account and admin order pages.
pub fn order_details(order: &Order, extra: Option<Node>) -> Node {
    let address = &order.shipping_address;
    let items: Vec<Node> = order
        .items
        .iter()
        .map(|item| {
            rsx! {
                <tr>
                    <td>{item.title.clone()}</td>
                    <td>{item.quantity}</td>
                    <td>{money(item.unit_price)}</td>
                    <td>{money(item.unit_price * item.quantity as f64)}</td>
                </tr>
            }
        })
        .collect();
    rsx! {
        <div class="card">
            <p><strong>"Order Date:"</strong>{format!(" {}", locale_date(order.created_at))}</p>
            {extra}
            <p><strong>"Status:"</strong>" "<span class="badge badge-info">{order.status.as_str()}</span></p>
            <h2 mix={css("margin-top: 2rem;")}>"Items"</h2>
            <table mix={css("margin-top: 1rem;")}>
                <thead><tr><th>"Book"</th><th>"Quantity"</th><th>"Price"</th><th>"Subtotal"</th></tr></thead>
                <tbody>{items}</tbody>
                <tfoot>
                    <tr>
                        <td colspan="3" mix={css("text-align: right; font-weight: bold;")}>"Total:"</td>
                        <td mix={css("font-weight: bold;")}>{money(order.total)}</td>
                    </tr>
                </tfoot>
            </table>
            <h2 mix={css("margin-top: 2rem;")}>"Shipping Address"</h2>
            <p>{address.street.clone()}</p>
            <p>{format!("{}, {} {}", address.city, address.state, address.zip)}</p>
        </div>
    }
}

fn order_href(order: &Order) -> String {
    ROUTES.account.orders.show.href([("orderId", order.id.to_string())])
}

async fn orders(ctx: Context) -> Response {
    let orders = db(&ctx).orders_for_user(user(&ctx).id);
    let body = if orders.is_empty() { rsx! { <p>"You have no orders yet."</p> } } else { orders_table(&orders, order_href) };
    let page = rsx! {
        <Layout>
            <h1>"My Orders"</h1>
            <div class="card">{body}</div>
            <p mix={css("margin-top: 1.5rem;")}><a href={ROUTES.account.index.href(())} class="btn btn-secondary">"Back to Account"</a></p>
        </Layout>
    };
    render(&ctx, page)
}

/// The order, when it belongs to the signed-in user.
fn own_order(ctx: &Context) -> Option<Order> {
    let order = db(ctx).find_order(parse_id(ctx.param("orderId"))?)?;
    (order.user_id == user(ctx).id).then_some(order)
}

async fn order(ctx: Context) -> Response {
    let Some(order) = own_order(&ctx) else {
        return not_found_page(&ctx, rsx! {
            <Layout>
                <div class="card">
                    <h1>"Order Not Found"</h1>
                    <p><a href={ROUTES.account.orders.index.href(())} class="btn">"Back to Orders"</a></p>
                </div>
            </Layout>
        });
    };
    let page = rsx! {
        <Layout>
            <h1>{format!("Order #{}", order.id)}</h1>
            {order_details(&order, None)}
            <p mix={css("margin-top: 1.5rem;")}><a href={ROUTES.account.orders.index.href(())} class="btn btn-secondary">"Back to Orders"</a></p>
        </Layout>
    };
    render(&ctx, page)
}

// --- checkout ---------------------------------------------------------------------------

async fn checkout(ctx: Context) -> Response {
    let cart = current_cart(&ctx);
    if cart.items.is_empty() {
        let page = rsx! {
            <Layout>
                <div class="card">
                    <h1>"Checkout"</h1>
                    <p>"Your cart is empty. Add some books before checking out."</p>
                    <p mix={css("margin-top: 1rem;")}><a href={ROUTES.books.index.href(())} class="btn">"Browse Books"</a></p>
                </div>
            </Layout>
        };
        return render(&ctx, page);
    }
    render(&ctx, checkout_page(&cart))
}

fn checkout_page(cart: &Cart) -> Node {
    let rows: Vec<Node> = cart
        .items
        .iter()
        .map(|item| {
            rsx! {
                <tr>
                    <td>{item.title.clone()}</td>
                    <td>{item.quantity}</td>
                    <td>{money(item.price)}</td>
                    <td>{money(item.price * item.quantity as f64)}</td>
                </tr>
            }
        })
        .collect();
    let input = |id: &'static str, label: &'static str| {
        rsx! {
            <div class="form-group">
                <label for={id}>{label}</label>
                <input type="text" id={id} name={id} required/>
            </div>
        }
    };
    rsx! {
        <Layout>
            <h1>"Checkout"</h1>
            <div class="card">
                <h2>"Order Summary"</h2>
                <table mix={css("margin-top: 1rem;")}>
                    <thead><tr><th>"Book"</th><th>"Quantity"</th><th>"Price"</th><th>"Subtotal"</th></tr></thead>
                    <tbody>{rows}</tbody>
                    <tfoot>
                        <tr>
                            <td colspan="3" mix={css("text-align: right; font-weight: bold;")}>"Total:"</td>
                            <td mix={css("font-weight: bold;")}>{money(cart.total())}</td>
                        </tr>
                    </tfoot>
                </table>
            </div>
            <div class="card" mix={css("margin-top: 1.5rem;")}>
                <h2>"Shipping Information"</h2>
                <form method="POST" action={ROUTES.checkout.action.href(())}>
                    {input("street", "Street Address")}
                    {input("city", "City")}
                    {input("state", "State")}
                    {input("zip", "ZIP Code")}
                    <button type="submit" class="btn">"Place Order"</button>
                    <a href={ROUTES.cart.index.href(())} class="btn btn-secondary" mix={css("margin-left: 0.5rem;")}>"Back to Cart"</a>
                </form>
            </div>
        </Layout>
    }
}

async fn place_order(ctx: Context) -> Response {
    let cart = current_cart(&ctx);
    if cart.items.is_empty() {
        return response::redirect(&ROUTES.cart.index.href(()));
    }
    let form = ctx.form_data();
    let field = |name| form.get(name).unwrap_or_default().to_string();
    let address = ShippingAddress { street: field("street"), city: field("city"), state: field("state"), zip: field("zip") };
    let order = match db(&ctx).create_order(user(&ctx).id, &cart.items, address) {
        Ok(order) => order,
        Err(e) => return bad_request(e.to_string()),
    };
    ctx.session().set("cart", Cart::default());
    response::redirect(&ROUTES.checkout.confirmation.href([("orderId", order.id.to_string())]))
}

async fn confirmation(ctx: Context) -> Response {
    let Some(order) = own_order(&ctx) else {
        return not_found_page(&ctx, rsx! {
            <Layout>
                <div class="card">
                    <h1>"Order Not Found"</h1>
                    <p><a href={ROUTES.account.orders.index.href(())} class="btn">"View My Orders"</a></p>
                </div>
            </Layout>
        });
    };
    let page = rsx! {
        <Layout>
            <div class="alert alert-success">
                <h1 mix={css("margin-bottom: 0.5rem;")}>"Order Confirmed!"</h1>
                <p>"Thank you for your purchase. Your order has been placed successfully."</p>
            </div>
            <div class="card">
                <h2>{format!("Order #{}", order.id)}</h2>
                <p><strong>"Order Date:"</strong>{format!(" {}", locale_date(order.created_at))}</p>
                <p><strong>"Total:"</strong>{format!(" {}", money(order.total))}</p>
                <p><strong>"Status:"</strong>" "<span class="badge badge-info">{order.status.as_str()}</span></p>
                <p mix={css("margin-top: 2rem;")}>
                    "We'll send you a confirmation email shortly. You can track your order status in your account."
                </p>
                <div mix={css("margin-top: 2rem;")}>
                    <a href={order_href(&order)} class="btn">"View Order Details"</a>
                    <a href={ROUTES.books.index.href(())} class="btn btn-secondary" mix={css("margin-left: 0.5rem;")}>"Continue Shopping"</a>
                </div>
            </div>
        </Layout>
    };
    render(&ctx, page)
}
