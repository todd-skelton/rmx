//! `actions/admin`: the dashboard and CRUD on books, users and orders, behind
//! `requireAuth()` and `requireAdmin()`.

use rmx::{css, rsx, Mixin, Node};
use rmx::fetch::{response, Response};
use rmx::middleware::form_data::FormDataExt;
use rmx::router::{middleware, Context, Router};

use super::account::{order_details, orders_table};
use super::data::{locale_date, Book, BookInput, Role, User, UserInput, WriteError, PLACEHOLDER_COVER};
use super::ui::{Layout, RestfulForm};
use super::{bad_request, db, not_found_page, parse_id, render, require_admin, require_auth, user};
use crate::cart::money;
use crate::routes::ROUTES;

pub fn routes(router: &mut Router) {
    router.group(vec![middleware(require_auth()), middleware(require_admin())], |r| {
        let a = &ROUTES.admin;
        r.route(&a.index, dashboard);
        r.route(&a.books.index, books);
        r.route(&a.books.new, new_book);
        r.route(&a.books.create, create_book);
        r.route(&a.books.show, show_book);
        r.route(&a.books.edit, edit_book);
        r.route(&a.books.update, update_book);
        r.route(&a.books.destroy, destroy_book);
        r.route(&a.users.index, users);
        r.route(&a.users.show, show_user);
        r.route(&a.users.edit, edit_user);
        r.route(&a.users.update, update_user);
        r.route(&a.users.destroy, destroy_user);
        r.route(&a.orders.index, orders);
        r.route(&a.orders.show, show_order);
    });
}

fn small_button() -> Mixin {
    css("font-size: 0.875rem; padding: 0.25rem 0.5rem;")
}

fn back_to_dashboard() -> Node {
    rsx! { <a href={ROUTES.admin.index.href(())} class="btn btn-secondary">"Back to Dashboard"</a> }
}

fn not_found(ctx: &Context, title: &str) -> Response {
    not_found_page(ctx, rsx! { <Layout><div class="card"><h1>{title.to_string()}</h1></div></Layout> })
}

fn write_failed(e: WriteError) -> Response {
    bad_request(e.to_string())
}

async fn dashboard(ctx: Context) -> Response {
    let card = |title: &str, text: &str, href: String, label: &str| {
        rsx! {
            <div class="card">
                <h2>{title.to_string()}</h2>
                <p>{text.to_string()}</p>
                <a href={href} class="btn" mix={css("margin-top: 1rem;")}>{label.to_string()}</a>
            </div>
        }
    };
    let page = rsx! {
        <Layout>
            <h1>"Admin Dashboard"</h1>
            <div mix={css("display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 1.5rem;")}>
                {card("Manage Books", "Add, edit, or remove books from the catalog.", ROUTES.admin.books.index.href(()), "View Books")}
                {card("Manage Users", "View and manage user accounts.", ROUTES.admin.users.index.href(()), "View Users")}
                {card("View Orders", "Monitor and manage customer orders.", ROUTES.admin.orders.index.href(()), "View Orders")}
            </div>
        </Layout>
    };
    render(&ctx, page)
}

// --- books ------------------------------------------------------------------------------

fn book_href(route: &rmx::router::Route, book: &Book) -> String {
    route.href([("bookId", book.id.to_string())])
}

fn stock_badge(in_stock: bool) -> Node {
    let class = if in_stock { "badge badge-success" } else { "badge badge-warning" };
    rsx! { <span class={class}>{if in_stock { "Yes" } else { "No" }}</span> }
}

async fn books(ctx: Context) -> Response {
    let rows: Vec<Node> = db(&ctx)
        .books()
        .iter()
        .map(|book| {
            rsx! {
                <tr>
                    <td>{book.title.clone()}</td>
                    <td>{book.author.clone()}</td>
                    <td>{book.genre.clone()}</td>
                    <td>{money(book.price)}</td>
                    <td>{stock_badge(book.in_stock)}</td>
                    <td class="actions">
                        <a href={book_href(&ROUTES.admin.books.edit, book)} class="btn btn-secondary" mix={small_button()}>"Edit"</a>
                        <RestfulForm method="DELETE" action={book_href(&ROUTES.admin.books.destroy, book)} style={css("display: inline;")}>
                            <button type="submit" class="btn btn-danger" mix={small_button()}>"Delete"</button>
                        </RestfulForm>
                    </td>
                </tr>
            }
        })
        .collect();
    let page = rsx! {
        <Layout>
            <h1>"Manage Books"</h1>
            <p mix={css("margin-bottom: 1rem;")}>
                <a href={ROUTES.admin.books.new.href(())} class="btn">"Add New Book"</a>
                <a href={ROUTES.admin.index.href(())} class="btn btn-secondary" mix={css("margin-left: 0.5rem;")}>"Back to Dashboard"</a>
            </p>
            <div class="card">
                <table>
                    <thead><tr><th>"Title"</th><th>"Author"</th><th>"Genre"</th><th>"Price"</th><th>"Stock"</th><th>"Actions"</th></tr></thead>
                    <tbody>{rows}</tbody>
                </table>
            </div>
        </Layout>
    };
    render(&ctx, page)
}

async fn show_book(ctx: Context) -> Response {
    let Some(book) = parse_id(ctx.param("bookId")).and_then(|id| db(&ctx).find_book(id)) else { return not_found(&ctx, "Book Not Found") };
    let line = |label: &str, value: String| rsx! { <p><strong>{format!("{label}:")}</strong>{format!(" {value}")}</p> };
    let page = rsx! {
        <Layout>
            <h1>"Book Details"</h1>
            <div class="card">
                {line("Title", book.title.clone())}
                {line("Author", book.author.clone())}
                {line("Slug", book.slug.clone())}
                {line("Description", book.description.clone())}
                {line("Price", money(book.price))}
                {line("Genre", book.genre.clone())}
                {line("ISBN", book.isbn.clone())}
                {line("Published", book.published_year.to_string())}
                <p><strong>"In Stock:"</strong>" "{stock_badge(book.in_stock)}</p>
                <div mix={css("margin-top: 2rem;")}>
                    <a href={book_href(&ROUTES.admin.books.edit, &book)} class="btn">"Edit"</a>
                    <a href={ROUTES.admin.books.index.href(())} class="btn btn-secondary" mix={css("margin-left: 0.5rem;")}>"Back to List"</a>
                </div>
            </div>
        </Layout>
    };
    render(&ctx, page)
}

/// `AdminBookFormPage`
fn book_form(title: &str, action: String, submit_label: &str, method: &str, book: Option<&Book>) -> Node {
    let value = |f: fn(&Book) -> String| book.map(f).unwrap_or_default();
    let text = |id: &'static str, label: &'static str, kind: &'static str, value: String| {
        rsx! {
            <div class="form-group">
                <label for={id}>{label}</label>
                <input type={kind} id={id} name={id} value={value} required/>
            </div>
        }
    };
    let in_stock = book.is_none_or(|b| b.in_stock);
    let current_cover = book.filter(|b| b.cover_url != PLACEHOLDER_COVER).map(|b| {
        rsx! {
            <div mix={css("margin-bottom: 0.5rem;")}>
                <img src={b.cover_url.clone()} alt={b.title.clone()} mix={css("max-width: 200px; height: auto; border-radius: 4px;")}/>
                <p mix={css("font-size: 0.875rem; color: #666;")}>"Current cover image"</p>
            </div>
        }
    });
    let cover_hint = if book.is_some() {
        "Optional. Upload a new cover image to replace the current one."
    } else {
        "Optional. Upload a cover image for this book."
    };
    rsx! {
        <Layout>
            <h1>{title.to_string()}</h1>
            <div class="card">
                <RestfulForm method={method.to_string()} action={action} enc_type="multipart/form-data">
                    {text("title", "Title", "text", value(|b| b.title.clone()))}
                    {text("author", "Author", "text", value(|b| b.author.clone()))}
                    {text("slug", "Slug (URL-friendly name)", "text", value(|b| b.slug.clone()))}
                    <div class="form-group">
                        <label for="description">"Description"</label>
                        <textarea id="description" name="description" required>{value(|b| b.description.clone())}</textarea>
                    </div>
                    <div class="form-group">
                        <label for="price">"Price"</label>
                        <input type="number" id="price" name="price" step="0.01" value={value(|b| b.price.to_string())} required/>
                    </div>
                    {text("genre", "Genre", "text", value(|b| b.genre.clone()))}
                    {text("isbn", "ISBN", "text", value(|b| b.isbn.clone()))}
                    {text("publishedYear", "Published Year", "number", book.map_or("2024".into(), |b| b.published_year.to_string()))}
                    <div class="form-group">
                        <label for="inStock">"In Stock"</label>
                        <select id="inStock" name="inStock">
                            <option value="true" selected={in_stock}>"Yes"</option>
                            <option value="false" selected={!in_stock}>"No"</option>
                        </select>
                    </div>
                    <div class="form-group">
                        <label for="cover">"Book Cover Image"</label>
                        {current_cover}
                        <input type="file" id="cover" name="cover" accept="image/*"/>
                        <small mix={css("color: #666;")}>{cover_hint}</small>
                    </div>
                    <button type="submit" class="btn">{submit_label.to_string()}</button>
                    <a href={ROUTES.admin.books.index.href(())} class="btn btn-secondary" mix={css("margin-left: 0.5rem;")}>"Cancel"</a>
                </RestfulForm>
            </div>
        </Layout>
    }
}

async fn new_book(ctx: Context) -> Response {
    render(&ctx, book_form("Add New Book", ROUTES.admin.books.create.href(()), "Create Book", "POST", None))
}

async fn edit_book(ctx: Context) -> Response {
    let Some(book) = parse_id(ctx.param("bookId")).and_then(|id| db(&ctx).find_book(id)) else { return not_found(&ctx, "Book Not Found") };
    render(&ctx, book_form("Edit Book", book_href(&ROUTES.admin.books.update, &book), "Update Book", "PUT", Some(&book)))
}

/// `bookSchema`: the form fields as a book. `cover` is the upload handler's
/// `/uploads/..` URL when a file was chosen.
fn book_input(ctx: &Context) -> (BookInput, Option<String>) {
    let form = ctx.form_data();
    let text = |name: &str| form.get(name).unwrap_or_default().to_string();
    let float = |name: &str, default: &str| form.get(name).unwrap_or(default).trim().parse::<f64>().unwrap_or(f64::NAN);
    let input = BookInput {
        slug: text("slug"),
        title: text("title"),
        author: text("author"),
        description: text("description"),
        price: float("price", "0"),
        genre: text("genre"),
        cover_url: String::new(),
        image_urls: None,
        isbn: text("isbn"),
        published_year: float("publishedYear", "2024").trunc() as i64,
        // coerce.boolean(), defaulting to false
        in_stock: form.get("inStock").is_some_and(|v| v == "true" || v == "on" || v == "1"),
    };
    (input, form.get("cover").filter(|c| !c.is_empty()).map(str::to_string))
}

async fn create_book(ctx: Context) -> Response {
    let (mut input, cover) = book_input(&ctx);
    input.cover_url = cover.unwrap_or_else(|| PLACEHOLDER_COVER.into());
    input.image_urls = Some(Vec::new());
    if let Err(e) = db(&ctx).create_book(input) {
        return write_failed(e);
    }
    response::redirect(&ROUTES.admin.books.index.href(()))
}

async fn update_book(ctx: Context) -> Response {
    let Some(book) = parse_id(ctx.param("bookId")).and_then(|id| db(&ctx).find_book(id)) else {
        return response::status(http::StatusCode::NOT_FOUND, "Book not found");
    };
    let (mut input, cover) = book_input(&ctx);
    input.cover_url = cover.unwrap_or(book.cover_url);
    if let Err(e) = db(&ctx).update_book(book.id, input) {
        return write_failed(e);
    }
    response::redirect(&ROUTES.admin.books.index.href(()))
}

async fn destroy_book(ctx: Context) -> Response {
    if let Some(id) = parse_id(ctx.param("bookId")) {
        if let Err(e) = db(&ctx).delete_book(id) {
            return response::status(http::StatusCode::CONFLICT, e.to_string());
        }
    }
    response::redirect(&ROUTES.admin.books.index.href(()))
}

// --- users ------------------------------------------------------------------------------

fn user_href(route: &rmx::router::Route, user: &User) -> String {
    route.href([("userId", user.id.to_string())])
}

fn role_badge(role: Role) -> Node {
    let class = if role == Role::Admin { "badge badge-info" } else { "badge badge-success" };
    rsx! { <span class={class}>{role.as_str()}</span> }
}

async fn users(ctx: Context) -> Response {
    let current = user(&ctx).id;
    let rows: Vec<Node> = db(&ctx)
        .users()
        .iter()
        .map(|u| {
            let delete = (u.id != current).then(|| {
                rsx! {
                    <RestfulForm method="DELETE" action={user_href(&ROUTES.admin.users.destroy, u)} style={css("display: inline;")}>
                        <button type="submit" class="btn btn-danger" mix={small_button()}>"Delete"</button>
                    </RestfulForm>
                }
            });
            rsx! {
                <tr>
                    <td>{u.name.clone()}</td>
                    <td>{u.email.clone()}</td>
                    <td>{role_badge(u.role)}</td>
                    <td>{locale_date(u.created_at)}</td>
                    <td class="actions">
                        <a href={user_href(&ROUTES.admin.users.edit, u)} class="btn btn-secondary" mix={small_button()}>"Edit"</a>
                        {delete}
                    </td>
                </tr>
            }
        })
        .collect();
    let page = rsx! {
        <Layout>
            <h1>"Manage Users"</h1>
            <p mix={css("margin-bottom: 1rem;")}>{back_to_dashboard()}</p>
            <div class="card">
                <table>
                    <thead><tr><th>"Name"</th><th>"Email"</th><th>"Role"</th><th>"Created"</th><th>"Actions"</th></tr></thead>
                    <tbody>{rows}</tbody>
                </table>
            </div>
        </Layout>
    };
    render(&ctx, page)
}

fn find_user(ctx: &Context) -> Option<User> {
    db(ctx).find_user(parse_id(ctx.param("userId"))?)
}

async fn show_user(ctx: Context) -> Response {
    let Some(u) = find_user(&ctx) else { return not_found(&ctx, "User Not Found") };
    let page = rsx! {
        <Layout>
            <h1>"User Details"</h1>
            <div class="card">
                <p><strong>"Name:"</strong>{format!(" {}", u.name)}</p>
                <p><strong>"Email:"</strong>{format!(" {}", u.email)}</p>
                <p><strong>"Role:"</strong>" "{role_badge(u.role)}</p>
                <p><strong>"Created:"</strong>{format!(" {}", locale_date(u.created_at))}</p>
                <div mix={css("margin-top: 2rem;")}>
                    <a href={user_href(&ROUTES.admin.users.edit, &u)} class="btn">"Edit"</a>
                    <a href={ROUTES.admin.users.index.href(())} class="btn btn-secondary" mix={css("margin-left: 0.5rem;")}>"Back to List"</a>
                </div>
            </div>
        </Layout>
    };
    render(&ctx, page)
}

async fn edit_user(ctx: Context) -> Response {
    let Some(u) = find_user(&ctx) else { return not_found(&ctx, "User Not Found") };
    let page = rsx! {
        <Layout>
            <h1>"Edit User"</h1>
            <div class="card">
                <RestfulForm method="PUT" action={user_href(&ROUTES.admin.users.update, &u)}>
                    <div class="form-group">
                        <label for="name">"Name"</label>
                        <input type="text" id="name" name="name" value={u.name.clone()} required/>
                    </div>
                    <div class="form-group">
                        <label for="email">"Email"</label>
                        <input type="email" id="email" name="email" value={u.email.clone()} required/>
                    </div>
                    <div class="form-group">
                        <label for="role">"Role"</label>
                        <select id="role" name="role">
                            <option value="customer" selected={u.role == Role::Customer}>"Customer"</option>
                            <option value="admin" selected={u.role == Role::Admin}>"Admin"</option>
                        </select>
                    </div>
                    <button type="submit" class="btn">"Update User"</button>
                    <a href={ROUTES.admin.users.index.href(())} class="btn btn-secondary" mix={css("margin-left: 0.5rem;")}>"Cancel"</a>
                </RestfulForm>
            </div>
        </Layout>
    };
    render(&ctx, page)
}

async fn update_user(ctx: Context) -> Response {
    let form = ctx.form_data();
    let Some(role) = Role::parse(form.get("role").unwrap_or("customer")) else {
        return bad_request("role must be customer or admin");
    };
    if let Some(u) = find_user(&ctx) {
        let update = UserInput {
            name: Some(form.get("name").unwrap_or_default().into()),
            email: Some(form.get("email").unwrap_or_default().into()),
            role: Some(role),
            password_hash: None,
        };
        if let Err(e) = db(&ctx).update_user(u.id, update) {
            return write_failed(e);
        }
    }
    response::redirect(&ROUTES.admin.users.index.href(()))
}

async fn destroy_user(ctx: Context) -> Response {
    if let Some(u) = find_user(&ctx) {
        if let Err(e) = db(&ctx).delete_user(u.id) {
            return response::status(http::StatusCode::CONFLICT, e.to_string());
        }
    }
    response::redirect(&ROUTES.admin.users.index.href(()))
}

// --- orders -----------------------------------------------------------------------------

async fn orders(ctx: Context) -> Response {
    let orders = db(&ctx).orders();
    let page = rsx! {
        <Layout>
            <h1>"Manage Orders"</h1>
            <p mix={css("margin-bottom: 1rem;")}>{back_to_dashboard()}</p>
            <div class="card">{orders_table(&orders, |o| ROUTES.admin.orders.show.href([("orderId", o.id.to_string())]))}</div>
        </Layout>
    };
    render(&ctx, page)
}

async fn show_order(ctx: Context) -> Response {
    let Some(order) = parse_id(ctx.param("orderId")).and_then(|id| db(&ctx).find_order(id)) else { return not_found(&ctx, "Order Not Found") };
    let user_line = rsx! { <p><strong>"User ID:"</strong>{format!(" {}", order.user_id)}</p> };
    let page = rsx! {
        <Layout>
            <h1>{format!("Order #{}", order.id)}</h1>
            {order_details(&order, Some(user_line))}
            <p mix={css("margin-top: 1.5rem;")}><a href={ROUTES.admin.orders.index.href(())} class="btn btn-secondary">"Back to Orders"</a></p>
        </Layout>
    };
    render(&ctx, page)
}
