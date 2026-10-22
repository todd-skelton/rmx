//! Port of Remix's `fetch-router/src/lib/request-context.test.ts`, one test per case.
//!
//! A context key is a type: `createContextKey<T>()` is a type (often a newtype) and
//! `context.get(key)` is `ctx.get::<T>()`, which is `None` until the value is set. A key with a
//! default value reads as `ctx.get::<T>().cloned().unwrap_or_default()` (or `unwrap_or`).
//!
//! Not ported, because they are about JavaScript objects: the four "direct property" cases
//! (`context.set(key, value, { property: 'message' })` installs `context.message` and the three
//! conflict errors), and the four `ContextWithEntry` type-derivation cases, which type those
//! properties. Rust has no properties to install; a value is read by its type.

use http::{HeaderMap, Method};
use rmx::fetch::{Body, File, FormData, Request};
use rmx::router::Context;

fn request(url: &str) -> Request {
    http::Request::builder().uri(url).body(Body::empty()).unwrap()
}

fn with_header(url: &str, name: &str, value: &str) -> Request {
    http::Request::builder().uri(url).header(name, value).body(Body::empty()).unwrap()
}

#[test]
fn provides_access_to_request_headers() {
    let context = Context::new(with_header("https://remix.run/test", "Content-Type", "application/json"));

    assert_eq!(context.header("content-type"), Some("application/json"));
}

#[test]
fn lazily_creates_a_mutable_copy_of_request_headers() {
    let mut context = Context::new(with_header("https://remix.run/test", "Content-Type", "text/html"));

    // The request is the context's now; a change to it before the copy is made is seen.
    context.request_mut().headers_mut().insert("content-type", "application/json".parse().unwrap());
    let copy = context.headers_mut().clone();
    assert_eq!(copy["content-type"], "application/json");

    context.request_mut().headers_mut().insert("content-type", "text/plain".parse().unwrap());

    assert_eq!(*context.headers(), copy);
    assert_eq!(context.header("content-type"), Some("application/json"));
}

#[test]
fn allows_overriding_request_headers() {
    let mut context = Context::new(request("https://remix.run/test"));
    let mut headers = HeaderMap::new();
    headers.insert("content-type", "text/plain".parse().unwrap());

    context.set_headers(headers.clone());

    assert_eq!(*context.headers(), headers);
    assert_eq!(context.header("content-type"), Some("text/plain"));
}

#[test]
fn provides_a_copy_of_request_headers_that_can_be_mutated_independently() {
    let mut context = Context::new(with_header("https://remix.run/test", "X-Original", "value"));

    context.headers_mut().insert("x-new", "new-value".parse().unwrap());
    context.headers_mut().remove("x-original");

    assert_eq!(context.header("x-new"), Some("new-value"));
    assert_eq!(context.header("x-original"), None);
    assert_eq!(context.request().headers()["x-original"], "value");
    assert!(context.request().headers().get("x-new").is_none());
}

#[test]
fn stores_and_reads_form_data_using_the_form_data_type_as_a_context_key() {
    let mut context = Context::new(http::Request::builder().method("POST").uri("https://remix.run/test").body(Body::empty()).unwrap());
    let file = File::new("hello.txt", "text/plain", "hello");
    let mut form_data = FormData::new();

    assert!(!context.has::<FormData>());

    form_data.append_file("avatar", file.clone());
    form_data.set("name", "Jane");
    context.set(form_data);

    assert!(context.has::<FormData>());
    let stored: &FormData = context.get::<FormData>().expect("FormData in request context");
    assert_eq!(stored.get("name"), Some("Jane"));
    let avatar = stored.file("avatar").expect("a File");
    assert_eq!(avatar.name, file.name);
    assert_eq!(avatar.content_type, file.content_type);
    assert_eq!(avatar.bytes, "hello");
}

#[test]
fn does_not_type_constructor_keys_as_available_unless_they_are_in_context() {
    let context = Context::new(request("https://remix.run/test"));

    // The type is an Option: a value is not available until it is set.
    let form_data: Option<&FormData> = context.get::<FormData>();

    assert!(form_data.is_none());
}

#[test]
fn stores_arbitrary_request_methods_as_strings() {
    let context = Context::new(http::Request::builder().method("PROPFIND").uri("https://remix.run/test").body(Body::empty()).unwrap());

    assert_eq!(context.method(), Method::from_bytes(b"PROPFIND").unwrap());
    assert_eq!(context.method().as_str(), "PROPFIND");
}

/// `createContextKey('hello')`: a key whose default value is `"hello"`.
#[derive(Clone, Debug, PartialEq)]
struct Hello(String);

impl Default for Hello {
    fn default() -> Hello {
        Hello("hello".into())
    }
}

/// `createContextKey<string>()`: a key without a default.
#[derive(Clone, Debug, PartialEq)]
struct Message(String);

#[test]
fn sets_and_gets_values_in_request_context() {
    let mut context = Context::new(request("https://remix.run/test"));

    context.set(Hello("world".into()));

    assert_eq!(context.get::<Hello>(), Some(&Hello("world".into())));
}

#[test]
fn gets_a_default_value_from_request_context_when_one_is_available() {
    let context = Context::new(request("https://remix.run/test"));

    assert_eq!(context.get::<Hello>().cloned().unwrap_or_default(), Hello("hello".into()));
}

#[test]
fn allows_null_as_a_valid_default_value_in_request_context() {
    // `createContextKey(null)`: a key whose value may be absent, defaulting to absent.
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Nullable(Option<String>);
    let context = Context::new(request("https://remix.run/test"));

    assert_eq!(context.get::<Nullable>().cloned().unwrap_or_default(), Nullable(None));
}

#[test]
fn allows_undefined_as_a_valid_default_value_in_request_context() {
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Undefined(());
    let context = Context::new(request("https://remix.run/test"));

    assert_eq!(context.get::<Undefined>().cloned().unwrap_or_default(), Undefined(()));
}

#[test]
fn returns_undefined_if_a_context_value_is_not_set_and_no_default_value_exists() {
    let context = Context::new(request("https://remix.run/test"));

    let value: Option<&Message> = context.get::<Message>();

    assert_eq!(value, None);
}

#[test]
fn checks_if_a_key_has_a_context_value() {
    let mut context = Context::new(request("https://remix.run/test"));

    // A default value does not count as set.
    assert!(!context.has::<Hello>());
    context.set(Hello("value".into()));
    assert!(context.has::<Hello>());
}

#[test]
fn gets_values_without_a_default_only_when_they_have_been_set() {
    let mut context = Context::new(request("https://remix.run/test"));

    assert_eq!(context.get::<Message>(), None);
    context.set(Message("value".into()));
    assert_eq!(context.get::<Message>(), Some(&Message("value".into())));
}

#[test]
fn supports_destructuring_get_set_has_from_request_context() {
    // The methods are plain functions of the context, usable as values.
    let (get, has, set) = (Context::get::<Message>, Context::has::<Message>, Context::set::<Message>);
    let get_default = |context: &Context| context.get::<Hello>().cloned().unwrap_or_default();
    let mut context = Context::new(request("https://remix.run/test"));

    assert!(!has(&context));
    assert_eq!(get_default(&context), Hello("hello".into()));
    assert_eq!(get(&context), None);

    set(&mut context, Message("value".into()));

    assert!(has(&context));
    assert_eq!(get_default(&context), Hello("hello".into()));
    assert_eq!(get(&context), Some(&Message("value".into())));
}

#[test]
fn supports_class_constructors_as_context_keys() {
    #[derive(Clone, Debug, PartialEq)]
    struct Value {
        text: String,
    }
    let mut context = Context::new(request("https://remix.run/test"));
    let value = Value { text: "hello".into() };

    context.set(value.clone());

    assert!(context.has::<Value>());
    assert_eq!(context.get::<Value>(), Some(&value));
}

#[test]
fn header_changes_reach_the_async_context() {
    // rmx: middleware that changes the context's headers changes what `get_context()` sees.
    use rmx::router::async_context::{async_context, get_context};
    use rmx::router::{Next, Router};
    let mut router = Router::new().with(async_context()).with(|mut ctx: Context, next: Next| {
        ctx.headers_mut().insert("x-user", "jane".parse().unwrap());
        next.run(ctx)
    });
    router.get("/", |_: Context| async { get_context().unwrap().header("x-user").unwrap_or_default() });

    let res = rmx::fetch::block_on(router.fetch(request("https://remix.run/")));

    assert_eq!(rmx::fetch::block_on(res.into_body().text()).unwrap(), "jane");
}
