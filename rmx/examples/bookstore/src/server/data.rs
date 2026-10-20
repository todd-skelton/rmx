//! `data/schema.ts`, `db/seed.sql` and `utils/password-hash.ts`, in memory.
//!
//! Remix keeps this in SQLite through `remix/data-table`. Storage is not what the demo is
//! about, so this port keeps the same tables, normalization, validation and foreign-key
//! rules in a mutex-guarded struct.

use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use crate::cart::CartItem;

pub const PLACEHOLDER_COVER: &str = "/images/placeholder.jpg";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Book {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub author: String,
    pub description: String,
    pub price: f64,
    pub genre: String,
    pub image_urls: Vec<String>,
    pub cover_url: String,
    pub isbn: String,
    pub published_year: i64,
    pub in_stock: bool,
}

/// The writable book columns (`db.create(books, ..)` / `db.update(books, id, ..)`).
#[derive(Clone, Debug, Default)]
pub struct BookInput {
    pub slug: String,
    pub title: String,
    pub author: String,
    pub description: String,
    pub price: f64,
    pub genre: String,
    pub cover_url: String,
    pub image_urls: Option<Vec<String>>,
    pub isbn: String,
    pub published_year: i64,
    pub in_stock: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    #[default]
    Customer,
    Admin,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Customer => "customer",
            Role::Admin => "admin",
        }
    }

    pub fn parse(value: &str) -> Option<Role> {
        match value {
            "customer" => Some(Role::Customer),
            "admin" => Some(Role::Admin),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub password_hash: String,
    pub name: String,
    pub role: Role,
    /// Milliseconds since the Unix epoch.
    pub created_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderStatus {
    Pending,
    Processing,
    Shipped,
    Delivered,
}

impl OrderStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            OrderStatus::Pending => "pending",
            OrderStatus::Processing => "processing",
            OrderStatus::Shipped => "shipped",
            OrderStatus::Delivered => "delivered",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ShippingAddress {
    pub street: String,
    pub city: String,
    pub state: String,
    pub zip: String,
}

/// An order with its items, each joined to its book (`with: { items: orderItemsWithBook }`).
#[derive(Clone, Debug, PartialEq)]
pub struct Order {
    pub id: i64,
    pub user_id: i64,
    pub total: f64,
    pub status: OrderStatus,
    pub shipping_address: ShippingAddress,
    pub created_at: u64,
    pub items: Vec<OrderItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrderItem {
    pub order_id: i64,
    pub book_id: i64,
    pub title: String,
    pub unit_price: f64,
    pub quantity: i64,
    pub book: Option<Book>,
}

#[derive(Clone, Debug, PartialEq)]
struct OrderRow {
    id: i64,
    user_id: i64,
    total: f64,
    status: OrderStatus,
    shipping_address: ShippingAddress,
    created_at: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PasswordResetToken {
    pub token: String,
    pub user_id: i64,
    pub expires_at: u64,
}

/// A failed write: the schema's validation issues, or a constraint the tables enforce.
#[derive(Clone, Debug, PartialEq)]
pub enum WriteError {
    Invalid(Vec<String>),
    /// A unique column already holds the value.
    Duplicate(&'static str),
    /// Other rows still reference this one (`on delete restrict`).
    Referenced(&'static str),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::Invalid(issues) => f.write_str(&issues.join(" ")),
            WriteError::Duplicate(column) => write!(f, "That {column} is already taken."),
            WriteError::Referenced(by) => write!(f, "Cannot delete: still referenced by {by}."),
        }
    }
}

#[derive(Default)]
struct Tables {
    books: Vec<Book>,
    users: Vec<User>,
    orders: Vec<OrderRow>,
    order_items: Vec<OrderItem>,
    reset_tokens: Vec<PasswordResetToken>,
}

/// The database handle. Clones share the same tables.
#[derive(Clone, Default)]
pub struct Db(Arc<Mutex<Tables>>);

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn normalize_text(value: &str) -> String {
    value.trim().to_string()
}

pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

fn normalize_slug(value: &str) -> String {
    let kept: String = value.trim().to_lowercase().chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c.is_whitespace() || *c == '-').collect();
    let mut out = String::with_capacity(kept.len());
    for word in kept.split_whitespace() {
        if !out.is_empty() {
            out.push('-');
        }
        out.push_str(word);
    }
    let mut collapsed = String::with_capacity(out.len());
    for c in out.chars() {
        if !(c == '-' && collapsed.ends_with('-')) {
            collapsed.push(c);
        }
    }
    collapsed
}

fn is_valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else { return false };
    let Some((host, tld)) = domain.rsplit_once('.') else { return false };
    let ok = |s: &str| !s.is_empty() && !s.contains(char::is_whitespace) && !s.contains('@');
    ok(local) && ok(host) && ok(tld)
}

impl BookInput {
    /// `beforeWrite` then `validate` for books.
    fn normalized(mut self) -> Result<BookInput, WriteError> {
        self.slug = normalize_slug(&self.slug);
        for field in [&mut self.title, &mut self.author, &mut self.description, &mut self.genre, &mut self.isbn] {
            *field = normalize_text(field);
        }
        if self.cover_url.trim().is_empty() {
            self.cover_url = PLACEHOLDER_COVER.into();
        }
        let mut issues = Vec::new();
        if self.slug.is_empty() {
            issues.push("Book slug is required.".to_string());
        }
        if self.title.is_empty() {
            issues.push("Book title is required.".to_string());
        }
        if !self.price.is_finite() || self.price < 0.0 {
            issues.push("Price must be a non-negative number.".to_string());
        }
        if self.published_year < 0 {
            issues.push("Published year must be a valid positive integer.".to_string());
        }
        if issues.is_empty() { Ok(self) } else { Err(WriteError::Invalid(issues)) }
    }
}

/// The user columns a write may change.
#[derive(Clone, Debug, Default)]
pub struct UserInput {
    pub email: Option<String>,
    pub password_hash: Option<String>,
    pub name: Option<String>,
    pub role: Option<Role>,
}

impl UserInput {
    fn normalized(mut self, creating: bool) -> Result<UserInput, WriteError> {
        self.name = self.name.map(|n| normalize_text(&n));
        self.email = self.email.map(|e| normalize_email(&e));
        self.password_hash = self.password_hash.map(|p| p.trim().to_string());
        let mut issues = Vec::new();
        if self.name.as_deref().map_or(creating, str::is_empty) {
            issues.push("Name is required.".to_string());
        }
        match self.email.as_deref() {
            None if creating => issues.push("Email is required.".to_string()),
            Some(email) if !is_valid_email(email) => issues.push("Email address is invalid.".to_string()),
            _ => {}
        }
        if self.password_hash.as_deref().map_or(creating, str::is_empty) {
            issues.push("Password hash is required.".to_string());
        }
        if issues.is_empty() { Ok(self) } else { Err(WriteError::Invalid(issues)) }
    }
}

impl Db {
    fn tables(&self) -> MutexGuard<'_, Tables> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The tables with `db/seed.sql` loaded.
    pub fn seeded() -> Db {
        let book = |id, slug: &str, title: &str, author: &str, description: &str, price, genre: &str, isbn: &str, year, in_stock| Book {
            id,
            slug: slug.into(),
            title: title.into(),
            author: author.into(),
            description: description.into(),
            price,
            genre: genre.into(),
            image_urls: (1..=3).map(|i| format!("/images/{slug}-{i}.jpg")).collect(),
            cover_url: format!("/images/{slug}-1.jpg"),
            isbn: isbn.into(),
            published_year: year,
            in_stock,
        };
        let address = ShippingAddress { street: "123 Main St".into(), city: "Boston".into(), state: "MA".into(), zip: "02101".into() };
        let item = |order_id, book_id, title: &str, unit_price, quantity| OrderItem { order_id, book_id, title: title.into(), unit_price, quantity, book: None };
        Db(Arc::new(Mutex::new(Tables {
            books: vec![
                book(1, "bbq", "Ash & Smoke", "Rusty Char-Broil", "The perfect gift for the BBQ enthusiast in your life!", 16.99, "cookbook", "978-0525559474", 2020, true),
                book(2, "heavy-metal", "Heavy Metal Guitar Riffs", "Axe Master Krush", "The ultimate guide to heavy metal guitar riffs!", 27.0, "music", "978-0735211292", 2018, true),
                book(3, "three-ways", "Three Ways to Change Your Life", "Wisdom Sage", "Life-changing guidance for modern living and personal growth.", 28.99, "self-help", "978-0061120084", 2021, false),
            ],
            // admin123 and password123, hashed by the Remix demo's utils/password-hash.ts.
            users: vec![
                User {
                    id: 1,
                    email: "admin@bookstore.com".into(),
                    password_hash: "pbkdf2_sha256$100000$Fyiej9zGB7-ZynH8DRWUJQ$AMSk4t8p8Jw9DVw8EdHOcq_g8SUtzTbh0agA9g0Q0qY".into(),
                    name: "Admin User".into(),
                    role: Role::Admin,
                    created_at: 1705276800000,
                },
                User {
                    id: 2,
                    email: "customer@example.com".into(),
                    password_hash: "pbkdf2_sha256$100000$R-GBp6yHQBgiqGkXZm70IQ$PmYoRXgmMoFYh2ONKFQ-KDg-QvfPAfBduyMlsX7tveA".into(),
                    name: "John Doe".into(),
                    role: Role::Customer,
                    created_at: 1709251200000,
                },
            ],
            orders: vec![
                OrderRow { id: 1001, user_id: 2, total: 45.98, status: OrderStatus::Delivered, shipping_address: address.clone(), created_at: 1726358400000 },
                OrderRow { id: 1002, user_id: 2, total: 54.0, status: OrderStatus::Shipped, shipping_address: address, created_at: 1727740800000 },
            ],
            order_items: vec![
                item(1001, 1, "Ash & Smoke", 16.99, 1),
                item(1001, 3, "Three Ways to Change Your Life", 28.99, 1),
                item(1002, 2, "Heavy Metal Guitar Riffs", 27.0, 2),
            ],
            reset_tokens: Vec::new(),
        })))
    }

    // --- books ---------------------------------------------------------------------------

    /// All books by id.
    pub fn books(&self) -> Vec<Book> {
        let mut books = self.tables().books.clone();
        books.sort_by_key(|b| b.id);
        books
    }

    pub fn count_books(&self) -> usize {
        self.tables().books.len()
    }

    /// Distinct genres, sorted.
    pub fn genres(&self) -> Vec<String> {
        let mut genres: Vec<String> = self.tables().books.iter().map(|b| b.genre.clone()).collect();
        genres.sort();
        genres.dedup();
        genres
    }

    pub fn find_book(&self, id: i64) -> Option<Book> {
        self.tables().books.iter().find(|b| b.id == id).cloned()
    }

    pub fn find_book_by_slug(&self, slug: &str) -> Option<Book> {
        self.tables().books.iter().find(|b| b.slug == slug).cloned()
    }

    /// Books whose genre equals `genre`, ignoring case (`ilike('genre', genre)`).
    pub fn books_in_genre(&self, genre: &str) -> Vec<Book> {
        self.books().into_iter().filter(|b| b.genre.to_lowercase() == genre.to_lowercase()).collect()
    }

    /// Books whose title, author or description contains `query`, ignoring case. The query
    /// is matched literally (no `%` or `_` wildcards).
    pub fn search_books(&self, query: &str) -> Vec<Book> {
        let q = query.to_lowercase();
        self.books()
            .into_iter()
            .filter(|b| [&b.title, &b.author, &b.description].iter().any(|field| field.to_lowercase().contains(&q)))
            .collect()
    }

    pub fn create_book(&self, input: BookInput) -> Result<Book, WriteError> {
        let input = input.normalized()?;
        let mut tables = self.tables();
        if tables.books.iter().any(|b| b.slug == input.slug) {
            return Err(WriteError::Duplicate("slug"));
        }
        let id = tables.books.iter().map(|b| b.id).max().unwrap_or(0) + 1;
        let book = Book {
            id,
            slug: input.slug,
            title: input.title,
            author: input.author,
            description: input.description,
            price: input.price,
            genre: input.genre,
            image_urls: input.image_urls.unwrap_or_default(),
            cover_url: input.cover_url,
            isbn: input.isbn,
            published_year: input.published_year,
            in_stock: input.in_stock,
        };
        tables.books.push(book.clone());
        Ok(book)
    }

    /// Update a book's columns; image URLs are kept unless given.
    pub fn update_book(&self, id: i64, input: BookInput) -> Result<Option<Book>, WriteError> {
        let input = input.normalized()?;
        let mut tables = self.tables();
        if tables.books.iter().any(|b| b.slug == input.slug && b.id != id) {
            return Err(WriteError::Duplicate("slug"));
        }
        let Some(book) = tables.books.iter_mut().find(|b| b.id == id) else { return Ok(None) };
        *book = Book {
            id,
            slug: input.slug,
            title: input.title,
            author: input.author,
            description: input.description,
            price: input.price,
            genre: input.genre,
            image_urls: input.image_urls.unwrap_or_else(|| book.image_urls.clone()),
            cover_url: input.cover_url,
            isbn: input.isbn,
            published_year: input.published_year,
            in_stock: input.in_stock,
        };
        Ok(Some(book.clone()))
    }

    pub fn delete_book(&self, id: i64) -> Result<bool, WriteError> {
        let mut tables = self.tables();
        if tables.order_items.iter().any(|item| item.book_id == id) {
            return Err(WriteError::Referenced("orders"));
        }
        let before = tables.books.len();
        tables.books.retain(|b| b.id != id);
        Ok(tables.books.len() < before)
    }

    // --- users ---------------------------------------------------------------------------

    pub fn users(&self) -> Vec<User> {
        let mut users = self.tables().users.clone();
        users.sort_by_key(|u| u.id);
        users
    }

    pub fn find_user(&self, id: i64) -> Option<User> {
        self.tables().users.iter().find(|u| u.id == id).cloned()
    }

    /// Exact match on the normalized address.
    pub fn find_user_by_email(&self, email: &str) -> Option<User> {
        let email = normalize_email(email);
        self.tables().users.iter().find(|u| u.email == email).cloned()
    }

    /// A new customer, created now unless `role` says otherwise.
    pub fn create_user(&self, input: UserInput) -> Result<User, WriteError> {
        let input = input.normalized(true)?;
        let mut tables = self.tables();
        let email = input.email.expect("validated");
        if tables.users.iter().any(|u| u.email == email) {
            return Err(WriteError::Duplicate("email"));
        }
        let id = tables.users.iter().map(|u| u.id).max().unwrap_or(0) + 1;
        let user = User {
            id,
            email,
            password_hash: input.password_hash.expect("validated"),
            name: input.name.expect("validated"),
            role: input.role.unwrap_or_default(),
            created_at: now_ms(),
        };
        tables.users.push(user.clone());
        Ok(user)
    }

    pub fn update_user(&self, id: i64, input: UserInput) -> Result<Option<User>, WriteError> {
        let input = input.normalized(false)?;
        let mut tables = self.tables();
        if let Some(email) = &input.email {
            if tables.users.iter().any(|u| &u.email == email && u.id != id) {
                return Err(WriteError::Duplicate("email"));
            }
        }
        let Some(user) = tables.users.iter_mut().find(|u| u.id == id) else { return Ok(None) };
        if let Some(email) = input.email {
            user.email = email;
        }
        if let Some(name) = input.name {
            user.name = name;
        }
        if let Some(hash) = input.password_hash {
            user.password_hash = hash;
        }
        if let Some(role) = input.role {
            user.role = role;
        }
        Ok(Some(user.clone()))
    }

    /// Deleting a user also deletes their reset tokens; users with orders cannot be deleted.
    pub fn delete_user(&self, id: i64) -> Result<bool, WriteError> {
        let mut tables = self.tables();
        if tables.orders.iter().any(|o| o.user_id == id) {
            return Err(WriteError::Referenced("orders"));
        }
        tables.reset_tokens.retain(|t| t.user_id != id);
        let before = tables.users.len();
        tables.users.retain(|u| u.id != id);
        Ok(tables.users.len() < before)
    }

    // --- orders --------------------------------------------------------------------------

    fn with_items(tables: &Tables, row: &OrderRow) -> Order {
        let mut items: Vec<OrderItem> = tables
            .order_items
            .iter()
            .filter(|item| item.order_id == row.id)
            .map(|item| OrderItem { book: tables.books.iter().find(|b| b.id == item.book_id).cloned(), ..item.clone() })
            .collect();
        items.sort_by_key(|item| item.book_id);
        Order {
            id: row.id,
            user_id: row.user_id,
            total: row.total,
            status: row.status,
            shipping_address: row.shipping_address.clone(),
            created_at: row.created_at,
            items,
        }
    }

    /// All orders, oldest first.
    pub fn orders(&self) -> Vec<Order> {
        let tables = self.tables();
        let mut rows = tables.orders.clone();
        rows.sort_by_key(|o| o.created_at);
        rows.iter().map(|row| Db::with_items(&tables, row)).collect()
    }

    pub fn orders_for_user(&self, user_id: i64) -> Vec<Order> {
        self.orders().into_iter().filter(|o| o.user_id == user_id).collect()
    }

    pub fn find_order(&self, id: i64) -> Option<Order> {
        let tables = self.tables();
        tables.orders.iter().find(|o| o.id == id).map(|row| Db::with_items(&tables, row))
    }

    /// Create a pending order and its items in one step (the demo's transaction).
    pub fn create_order(&self, user_id: i64, items: &[CartItem], shipping_address: ShippingAddress) -> Result<Order, WriteError> {
        let total: f64 = items.iter().map(|i| i.price * i.quantity as f64).sum();
        if items.iter().any(|i| i.quantity < 1) {
            return Err(WriteError::Invalid(vec!["Quantity must be an integer greater than 0.".into()]));
        }
        let mut tables = self.tables();
        if items.iter().any(|i| !tables.books.iter().any(|b| b.id == i.book_id)) {
            return Err(WriteError::Invalid(vec!["Order items must reference existing books.".into()]));
        }
        let id = tables.orders.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let row = OrderRow { id, user_id, total, status: OrderStatus::Pending, shipping_address, created_at: now_ms() };
        tables.orders.push(row.clone());
        for item in items {
            tables.order_items.push(OrderItem {
                order_id: id,
                book_id: item.book_id,
                title: normalize_text(&item.title),
                unit_price: item.price,
                quantity: item.quantity,
                book: None,
            });
        }
        Ok(Db::with_items(&tables, &row))
    }

    // --- password reset tokens -----------------------------------------------------------

    pub fn create_reset_token(&self, token: PasswordResetToken) {
        self.tables().reset_tokens.push(token);
    }

    pub fn find_reset_token(&self, token: &str) -> Option<PasswordResetToken> {
        self.tables().reset_tokens.iter().find(|t| t.token == token).cloned()
    }

    pub fn delete_reset_token(&self, token: &str) {
        self.tables().reset_tokens.retain(|t| t.token != token);
    }
}

// ---------------------------------------------------------------------------------------
// utils/password-hash.ts
// ---------------------------------------------------------------------------------------

const HASH_PREFIX: &str = "pbkdf2_sha256";
const ITERATIONS: u32 = 100_000;

pub fn hash_password(password: &str) -> String {
    let salt = rmx_crypto::random_bytes::<16>();
    let hash = rmx_crypto::pbkdf2_sha256(password.as_bytes(), &salt, ITERATIONS, 32);
    format!("{HASH_PREFIX}${ITERATIONS}${}${}", rmx_crypto::base64url_encode(&salt), rmx_crypto::base64url_encode(&hash))
}

pub fn verify_password(password: &str, encoded: &str) -> bool {
    let mut parts = encoded.split('$');
    let (Some(HASH_PREFIX), Some(iterations), Some(salt), Some(hash)) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let Some(iterations) = iterations.parse::<u32>().ok().filter(|i| *i >= 1) else { return false };
    let (Some(salt), Some(expected)) = (rmx_crypto::base64url_decode(salt), rmx_crypto::base64url_decode(hash)) else {
        return false;
    };
    if salt.is_empty() || expected.is_empty() {
        return false;
    }
    let actual = rmx_crypto::pbkdf2_sha256(password.as_bytes(), &salt, iterations, expected.len());
    rmx_crypto::constant_time_eq(&actual, &expected)
}

/// `toLocaleDateString()` in en-US for a UTC timestamp: `3/1/2024`.
pub fn locale_date(ms: u64) -> String {
    let days = (ms / 86_400_000) as i64;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{month}/{day}/{year}")
}
