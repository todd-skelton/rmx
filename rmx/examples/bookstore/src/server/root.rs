//! `actions/controller.tsx` (home, about, search, uploads), `actions/contact`,
//! `actions/fragments` and `actions/api`.

use http::{Method, StatusCode};
use rmx::{css, rsx, IntoNode, Node};
use rmx::fetch::{response, Response};
use rmx::file_storage::FileStorage;
use rmx::middleware::form_data::FormDataExt;
use rmx::middleware::session::SessionExt;
use rmx::middleware::static_files::{file_response_for, StaticOptions};
use rmx::render::ResponseInit;
use rmx::router::{Context, Router};

use super::data::Book;
use super::ui::{book_card, Layout};
use super::{current_cart, current_user, db, parse_id, render, render_with};
use crate::routes::ROUTES;
use crate::{CartButton, CartItems};

pub fn routes<U: FileStorage + Clone>(router: &mut Router, uploads: U) {
    let r = &*ROUTES;
    router.route(&r.uploads, move |ctx: Context| {
        let uploads = uploads.clone();
        let key = ctx.param("key").unwrap_or_default().to_string();
        let headers = ctx.headers().clone();
        let head = ctx.method() == Method::HEAD;
        async move { serve_upload(key, headers, head, uploads).await }
    });
    router.route(&r.home, home);
    router.route(&r.about, |ctx: Context| async move { render(&ctx, about_page()) });
    router.route(&r.search, search);
    router.route(&r.contact.index, |ctx: Context| async move { render(&ctx, contact_page()) });
    router.route(&r.contact.action, |ctx: Context| async move { render(&ctx, contact_success_page()) });
    router.route(&r.fragments.cart_button, cart_button);
    router.route(&r.fragments.cart_items, cart_items);
    router.route(&r.api.cart_toggle, cart_toggle);
}

async fn serve_upload<U: FileStorage>(key: String, headers: http::HeaderMap, head: bool, uploads: U) -> Response {
    match uploads.get(&key).await {
        Ok(Some(file)) => {
            let options = StaticOptions { cache_control: Some("public, max-age=31536000".into()), ..Default::default() };
            file_response_for(&file, &headers, head, &options)
        }
        _ => response::status(StatusCode::NOT_FOUND, "File not found"),
    }
}

async fn home(ctx: Context) -> Response {
    let featured: Vec<Book> = ["bbq", "heavy-metal", "three-ways"].iter().filter_map(|slug| db(&ctx).find_book_by_slug(slug)).collect();
    let page = rsx! {
        <Layout>
            <div class="card">
                <h1>"Welcome to the Bookstore"</h1>
                <p mix={css("margin: 1rem 0;")}>
                    "Discover your next favorite book from our curated collection of fiction, non-fiction, and more."
                </p>
                <p><a href={ROUTES.books.index.href(())} class="btn">"Browse Books"</a></p>
            </div>
            <h2 mix={css("margin: 2rem 0 1rem;")}>"Featured Books"</h2>
            <div class="grid">{featured.iter().map(book_card).collect::<Vec<_>>()}</div>
        </Layout>
    };
    render_with(&ctx, page, ResponseInit::default().header("cache-control", "no-store"))
}

fn about_page() -> Node {
    let item = |title: &str, text: &str| rsx! { <li><strong>{format!("{title}:")}</strong>{format!(" {text}")}</li> };
    rsx! {
        <Layout>
            <div class="card">
                <h1>"About Our Bookstore"</h1>
                <p mix={css("margin: 1rem 0;")}>
                    "Welcome to our online bookstore, a demo application built to showcase the capabilities of "
                    <strong>"rmx"</strong>
                    " - a Rust port of Remix's routing, middleware and component model."
                </p>
                <h2 mix={css("margin: 1.5rem 0 0.5rem;")}>"What This Demo Shows"</h2>
                <ul mix={css("margin-left: 2rem; line-height: 2;")}>
                    {item("Resource Routes", "Full RESTful CRUD operations")}
                    {item("Nested Routes", "Deep route hierarchies with type safety")}
                    {item("Custom Parameters", "Flexible parameter naming (slug, orderId, etc.)")}
                    {item("HTTP Methods", "GET, POST, PUT, DELETE properly used")}
                    {item("Middleware", "Authentication and authorization")}
                    {item("Type Safety", "End-to-end type checking for routes and handlers")}
                </ul>
                <h2 mix={css("margin: 1.5rem 0 0.5rem;")}>"Try It Out"</h2>
                <p mix={css("margin: 1rem 0;")}>
                    "Explore the site to see all these features in action. You can browse books, create an account, add items to your cart, and even access the admin panel (login as admin@bookstore.com / admin123)."
                </p>
                <p mix={css("margin-top: 2rem;")}>
                    <a href={ROUTES.books.index.href(())} class="btn">"Explore Books"</a>
                    <a href={ROUTES.auth.register.index.href(())} class="btn btn-secondary" mix={css("margin-left: 1rem;")}>"Create Account"</a>
                </p>
            </div>
        </Layout>
    }
}

async fn search(ctx: Context) -> Response {
    let query = ctx.url().search_params().get("q").unwrap_or_default().to_string();
    let matching = if query.is_empty() { Vec::new() } else { db(&ctx).search_books(&query) };
    let results = if matching.is_empty() {
        rsx! { <p>"No books found matching your search."</p> }
    } else {
        matching.iter().map(book_card).collect::<Vec<_>>().into_node()
    };
    let page = rsx! {
        <Layout>
            <h1>"Search Results"</h1>
            <div class="card" mix={css("margin-bottom: 2rem;")}>
                <form action={ROUTES.search.href(())} method="GET" mix={css("display: flex; gap: 0.5rem;")}>
                    <input type="search" name="q" placeholder="Search books..." value={query.clone()} mix={css("flex: 1; padding: 0.5rem;")}/>
                    <button type="submit" class="btn">"Search"</button>
                </form>
            </div>
            {(!query.is_empty()).then(|| rsx! {
                <p mix={css("margin-bottom: 1rem;")}>{format!("Found {} result(s) for \"{query}\"", matching.len())}</p>
            })}
            <div class="grid">{results}</div>
        </Layout>
    };
    render(&ctx, page)
}

fn contact_page() -> Node {
    rsx! {
        <Layout>
            <div class="card">
                <h1>"Contact Us"</h1>
                <p mix={css("margin: 1rem 0;")}>"Have a question or feedback? We'd love to hear from you!"</p>
                <form method="POST" action={ROUTES.contact.action.href(())}>
                    <div class="form-group">
                        <label for="name">"Name"</label>
                        <input type="text" id="name" name="name" required/>
                    </div>
                    <div class="form-group">
                        <label for="email">"Email"</label>
                        <input type="email" id="email" name="email" required/>
                    </div>
                    <div class="form-group">
                        <label for="message">"Message"</label>
                        <textarea id="message" name="message" required></textarea>
                    </div>
                    <button type="submit" class="btn">"Send Message"</button>
                </form>
            </div>
        </Layout>
    }
}

fn contact_success_page() -> Node {
    rsx! {
        <Layout>
            <div class="alert alert-success">"Thank you for your message! We'll get back to you soon."</div>
            <div class="card">
                <p><a href={ROUTES.home.href(())} class="btn">"Return Home"</a></p>
            </div>
        </Layout>
    }
}

// ---------------------------------------------------------------------------------------
// actions/fragments/controller.tsx
// ---------------------------------------------------------------------------------------

/// Fragments are never cached.
fn fragment_init(status: StatusCode) -> ResponseInit {
    ResponseInit::status(status).header("cache-control", "no-store")
}

async fn cart_button(ctx: Context) -> Response {
    let Some(book) = parse_id(ctx.param("bookId")).and_then(|id| db(&ctx).find_book(id)) else {
        return render_with(&ctx, rsx! { <p>"Book not found"</p> }, fragment_init(StatusCode::NOT_FOUND));
    };
    let in_cart = current_cart(&ctx).contains(book.id);
    render_with(&ctx, rsx! { <CartButton in_cart={in_cart} id={book.id} slug={book.slug.clone()}/> }, fragment_init(StatusCode::OK))
}

async fn cart_items(ctx: Context) -> Response {
    let cart = current_cart(&ctx);
    if cart.items.is_empty() {
        let empty = rsx! {
            <div mix={css("margin-top: 2rem;")}>
                <p>"Your cart is empty."</p>
                <p mix={css("margin-top: 1rem;")}><a href={ROUTES.books.index.href(())} class="btn">"Browse Books"</a></p>
            </div>
        };
        return render_with(&ctx, empty, fragment_init(StatusCode::OK));
    }
    let total = cart.total();
    let can_checkout = current_user(&ctx).is_some();
    render_with(&ctx, rsx! { <CartItems items={cart.items} total={total} can_checkout={can_checkout}/> }, fragment_init(StatusCode::OK))
}

// ---------------------------------------------------------------------------------------
// actions/api/controller.tsx
// ---------------------------------------------------------------------------------------

async fn cart_toggle(ctx: Context) -> Response {
    let Some(book) = parse_id(ctx.form_data().get("bookId")).and_then(|id| db(&ctx).find_book(id)) else {
        return response::status(StatusCode::NOT_FOUND, "Book not found");
    };
    let mut cart = current_cart(&ctx);
    if cart.contains(book.id) {
        cart.remove(book.id);
    } else {
        cart.add(book.id, &book.slug, &book.title, book.price, 1);
    }
    ctx.session().set("cart", &cart);
    super::no_content()
}
