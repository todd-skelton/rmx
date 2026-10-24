//! `ui/*`: the document shell, the layout, book cards and `RestfulForm`.

use rsmix::{css, rsx, Children, Component, Frame, Handle, Mixin, Node};

use super::data::{Book, User};
use crate::cart::money;
use crate::routes::ROUTES;

/// What the layout needs to know about the request. Remix reads it through the
/// async-context middleware; here [`super::render`] provides it to the tree.
#[derive(Clone, Debug, Default)]
pub struct Viewer {
    pub user: Option<User>,
}

#[derive(Clone, Default)]
pub struct ViewerProviderProps {
    pub viewer: Viewer,
    pub children: Children,
}

pub struct ViewerProvider;

impl Component for ViewerProvider {
    type Props = ViewerProviderProps;

    fn setup(props: &ViewerProviderProps, h: &Handle<Self>) -> Self {
        h.set_context(props.viewer.clone());
        ViewerProvider
    }

    fn render(&self, props: &ViewerProviderProps, _: &Handle<Self>) -> Node {
        rsx! { <>{&props.children}</> }
    }
}

#[derive(Clone, Default)]
pub struct DocumentProps {
    pub title: Option<String>,
    pub children: Children,
}

pub struct Document;

impl Component for Document {
    type Props = DocumentProps;

    fn setup(_: &DocumentProps, _: &Handle<Self>) -> Self {
        Document
    }

    fn render(&self, props: &DocumentProps, _: &Handle<Self>) -> Node {
        let title = props.title.clone().unwrap_or_else(|| "Bookstore".into());
        rsx! {
            <html lang="en">
                <head>
                    <meta charset="UTF-8"/>
                    <meta name="viewport" content="width=device-width, initial-scale=1.0"/>
                    <title>{title}</title>
                    <link rel="stylesheet" href="/app.css"/>
                    <script type="module" async src="/boot.js"></script>
                </head>
                <body>{&props.children}</body>
            </html>
        }
    }
}

#[derive(Clone, Default)]
pub struct LayoutProps {
    pub title: Option<String>,
    pub children: Children,
}

pub struct Layout {
    user: Option<User>,
}

impl Component for Layout {
    type Props = LayoutProps;

    fn setup(_: &LayoutProps, h: &Handle<Self>) -> Self {
        Layout { user: h.context::<Viewer>().and_then(|v| v.user.clone()) }
    }

    fn render(&self, props: &LayoutProps, _: &Handle<Self>) -> Node {
        let r = &*ROUTES;
        let nav_user = match &self.user {
            Some(user) => rsx! {
                <>
                    <a data-rmx-prefetch="intent" href={r.account.index.href(())}>"Account"</a>
                    {(user.role == super::data::Role::Admin).then(|| rsx! { <a data-rmx-prefetch="intent" href={r.admin.index.href(())}>"Admin"</a> })}
                    <form method="POST" action={r.auth.logout.href(())} mix={css("display: inline;")}>
                        <button type="submit" class="btn btn-secondary" mix={css("margin-left: 1rem;")}>"Logout"</button>
                    </form>
                </>
            },
            None => rsx! {
                <>
                    <a href={r.auth.login.index.href(())}>"Login"</a>
                    <a data-rmx-prefetch="intent" href={r.auth.register.index.href(())}>"Register"</a>
                </>
            },
        };
        rsx! {
            <Document title={props.title.clone()}>
                <header>
                    <div class="container">
                        <h1><a data-rmx-prefetch="intent" href={r.home.href(())}>"📚 Bookstore"</a></h1>
                        <nav>
                            <a data-rmx-prefetch="intent" href={r.home.href(())}>"Home"</a>
                            <a data-rmx-prefetch="intent" href={r.books.index.href(())}>"Books"</a>
                            <a data-rmx-prefetch="intent" href={r.about.href(())}>"About"</a>
                            <a data-rmx-prefetch="intent" href={r.contact.index.href(())}>"Contact"</a>
                            <a data-rmx-prefetch="intent" href={r.cart.index.href(())}>"Cart"</a>
                            {nav_user}
                        </nav>
                    </div>
                </header>
                <main>
                    <div class="container">{&props.children}</div>
                </main>
                <footer>
                    <div class="container">
                        <p>{format!("\u{a9} {} Bookstore Demo. Built with rsmix.", current_year())}</p>
                    </div>
                </footer>
            </Document>
        }
    }
}

fn current_year() -> i64 {
    let date = super::data::locale_date(super::data::now_ms());
    date.rsplit('/').next().and_then(|y| y.parse().ok()).unwrap_or(1970)
}

/// `ui/book-card.tsx`: the cart button is a frame, so it can reload on its own.
pub fn book_card(book: &Book) -> Node {
    rsx! {
        <div class="book-card" data-test-slug={book.slug.clone()}>
            <img src={book.cover_url.clone()} alt={book.title.clone()}/>
            <div class="book-card-body">
                <h3>{book.title.clone()}</h3>
                <p class="author">{format!("by {}", book.author)}</p>
                <p class="price">{money(book.price)}</p>
                <div mix={css("display: flex; gap: 0.5rem; align-items: center;")}>
                    <a href={ROUTES.books.show.href([("slug", book.slug.as_str())])} data-rmx-prefetch="viewport" class="btn">"View Details"</a>
                    <Frame src={ROUTES.fragments.cart_button.href([("bookId", book.id.to_string())])}/>
                </div>
            </div>
        </div>
    }
}

#[derive(Clone, Default)]
pub struct RestfulFormProps {
    pub method: String,
    pub action: String,
    pub enc_type: Option<&'static str>,
    pub style: Option<Mixin>,
    /// The hidden field that carries the method (default `_method`).
    pub method_override_field: Option<&'static str>,
    pub children: Children,
}

/// A `<form>` that can submit `PUT` and `DELETE`: anything other than `GET` and `POST`
/// posts with a hidden method-override field.
pub struct RestfulForm;

impl Component for RestfulForm {
    type Props = RestfulFormProps;

    fn setup(_: &RestfulFormProps, _: &Handle<Self>) -> Self {
        RestfulForm
    }

    fn render(&self, props: &RestfulFormProps, _: &Handle<Self>) -> Node {
        let method = if props.method.is_empty() { "GET".to_string() } else { props.method.to_uppercase() };
        let mix = props.style.clone().unwrap_or_else(|| css(""));
        if method == "GET" {
            return rsx! { <form method="GET" action={props.action.clone()} enctype={props.enc_type} mix={mix}>{&props.children}</form> };
        }
        let field = props.method_override_field.unwrap_or("_method");
        rsx! {
            <form method="POST" action={props.action.clone()} enctype={props.enc_type} mix={mix}>
                {(method != "POST").then(|| rsx! { <input type="hidden" name={field} value={method.clone()}/> })}
                {&props.children}
            </form>
        }
    }
}
