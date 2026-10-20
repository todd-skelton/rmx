//! Typed HTTP header values (Remix's `headers` package). Each type parses from the raw
//! header (`None` when the request has none) and prints back with `Display`.
//!
//! ```
//! use rmx_headers::{AcceptLanguage, Vary};
//!
//! let accept = AcceptLanguage::parse(Some("fr-CA,fr;q=0.9,en;q=0.8"));
//! assert_eq!(accept.preferred(&["en", "es", "fr"]), Some("fr"));
//!
//! let mut vary = Vary::parse(Some("Accept-Encoding"));
//! vary.add("Accept-Language");
//! assert_eq!(vary.to_string(), "accept-encoding, accept-language");
//! ```

mod accept;
mod cache_control;
mod conditional;
mod content;
mod cookie;
mod date;
mod params;
mod range;
mod vary;

pub use accept::{Accept, AcceptEncoding, AcceptLanguage};
pub use cache_control::CacheControl;
pub use conditional::{IfMatch, IfNoneMatch, IfRange};
pub use content::{ContentDisposition, ContentRange, ContentType};
pub use cookie::{Cookie, SameSite, SetCookie};
pub use date::{format_http_date, parse_http_date, unix_seconds};
pub use params::{parse_params, quote, quote_etag, split_list};
pub use range::Range;
pub use vary::Vary;
