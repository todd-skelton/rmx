//! `actions/books`: the catalog, genres and book pages.

use rmx::{css, rsx, Frame, Node};
use rmx::fetch::Response;
use rmx::render::ResponseInit;
use rmx::router::{Context, Router};

use super::data::Book;
use super::ui::{book_card, Layout};
use super::{db, not_found_page, render, render_with};
use crate::cart::money;
use crate::routes::ROUTES;
use crate::ImageCarousel;

pub fn routes(router: &mut Router) {
    let r = &ROUTES.books;
    router.route(&r.index, index);
    router.route(&r.genre, genre);
    router.route(&r.show, show);
}

async fn index(ctx: Context) -> Response {
    let books = db(&ctx).books();
    let genres = db(&ctx).genres();
    let page = rsx! {
        <Layout>
            <h1>"Browse Books"</h1>
            <div class="card" mix={css("margin-bottom: 2rem;")}>
                <form action={ROUTES.search.href(())} method="GET" mix={css("display: flex; gap: 0.5rem;")}>
                    <input type="search" name="q" placeholder="Search books by title, author, or description..." mix={css("flex: 1; padding: 0.5rem;")}/>
                    <button type="submit" class="btn">"Search"</button>
                </form>
            </div>
            <div class="card" mix={css("margin-bottom: 2rem;")}>
                <h3>"Browse by Genre"</h3>
                <div mix={css("display: flex; gap: 0.5rem; flex-wrap: wrap; margin-top: 1rem;")}>
                    {genres.iter().map(|g| rsx! { <a href={ROUTES.books.genre.href([("genre", g.as_str())])} class="btn btn-secondary">{g.clone()}</a> }).collect::<Vec<_>>()}
                </div>
            </div>
            <div class="grid">{books.iter().map(book_card).collect::<Vec<_>>()}</div>
        </Layout>
    };
    render(&ctx, page)
}

async fn genre(ctx: Context) -> Response {
    let genre = ctx.param("genre").unwrap_or_default().to_string();
    let books = db(&ctx).books_in_genre(&genre);
    if books.is_empty() {
        return not_found_page(&ctx, rsx! {
            <Layout>
                <div class="card">
                    <h1>"Genre Not Found"</h1>
                    <p>{format!("No books found in the \"{genre}\" genre.")}</p>
                    <p mix={css("margin-top: 1rem;")}><a href={ROUTES.books.index.href(())} class="btn">"Browse All Books"</a></p>
                </div>
            </Layout>
        });
    }
    let mut title: Vec<char> = genre.chars().collect();
    if let Some(first) = title.first_mut() {
        *first = first.to_ascii_uppercase();
    }
    let page = rsx! {
        <Layout>
            <h1>{format!("{} Books", title.into_iter().collect::<String>())}</h1>
            <p mix={css("margin: 1rem 0;")}><a href={ROUTES.books.index.href(())} class="btn btn-secondary">"View All Books"</a></p>
            <div class="grid" mix={css("margin-top: 2rem;")}>{books.iter().map(book_card).collect::<Vec<_>>()}</div>
        </Layout>
    };
    render(&ctx, page)
}

async fn show(ctx: Context) -> Response {
    let Some(book) = db(&ctx).find_book_by_slug(ctx.param("slug").unwrap_or_default()) else {
        return not_found_page(&ctx, rsx! { <Layout><div class="card"><h1>"Book Not Found"</h1></div></Layout> });
    };
    render_with(&ctx, show_page(&book), ResponseInit::default().header("cache-control", "no-store"))
}

fn show_page(book: &Book) -> Node {
    let stock_badge = if book.in_stock { "badge badge-success" } else { "badge badge-warning" };
    let buy = if book.in_stock {
        rsx! { <div mix={css("margin-top: 2rem;")}><Frame src={ROUTES.fragments.cart_button.href([("bookId", book.id.to_string())])}/></div> }
    } else {
        rsx! { <p mix={css("color: #e74c3c; font-weight: 500;")}>"This book is currently out of stock."</p> }
    };
    rsx! {
        <Layout>
            <div mix={css("display: grid; grid-template-columns: 300px 1fr; gap: 2rem;")}>
                <div mix={css("height: 400px; border-radius: 8px; box-shadow: 0 4px 8px rgba(0,0,0,0.1); overflow: hidden;")}>
                    <ImageCarousel images={book.image_urls.clone()}/>
                </div>
                <div class="card">
                    <h1>{book.title.clone()}</h1>
                    <p class="author" mix={css("font-size: 1.2rem; margin: 0.5rem 0;")}>{format!("by {}", book.author)}</p>
                    <p mix={css("margin: 1rem 0;")}>
                        <span class="badge badge-info">{book.genre.clone()}</span>
                        <span class={stock_badge} mix={css("margin-left: 0.5rem;")}>{if book.in_stock { "In Stock" } else { "Out of Stock" }}</span>
                    </p>
                    <p class="price" mix={css("font-size: 2rem; margin: 1rem 0;")}>{money(book.price)}</p>
                    <p mix={css("margin: 1.5rem 0; line-height: 1.8;")}>{book.description.clone()}</p>
                    <div mix={css("margin: 1.5rem 0; padding: 1rem; background: #f8f9fa; border-radius: 4px;")}>
                        <p><strong>"ISBN:"</strong>{format!(" {}", book.isbn)}</p>
                        <p><strong>"Published:"</strong>{format!(" {}", book.published_year)}</p>
                    </div>
                    {buy}
                    <p mix={css("margin-top: 1.5rem;")}><a href={ROUTES.books.index.href(())} class="btn btn-secondary">"Back to Books"</a></p>
                </div>
            </div>
        </Layout>
    }
}
