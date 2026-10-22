//! Request logging (`remix/middleware/logger`).
//!
//! ```
//! use rmx_middleware::logger::{logger, LoggerExt};
//! use rmx_router::{Context, Router};
//!
//! let router = Router::new().with(logger().format("%method %path %status %duration ms").colors(false));
//! # let mut router = router;
//! router.get("/", |ctx: Context| async move {
//!     ctx.logger().log("handling the home page"); // `context.logger(...)`
//!     "Home"
//! });
//! ```

use std::sync::Arc;
use std::time::{Instant, SystemTime};

use http::header;
use rmx_headers::UtcDateTime;
use rmx_terminal::Styles;
use rmx_fetch::Response;
use rmx_router::{BoxFuture, Context, Middleware, Next};

/// Remix's default format.
pub const DEFAULT_FORMAT: &str = "[%date] %method %path %status %contentLength";

/// The log function the middleware writes to, available to handlers as `ctx.logger()` (the
/// `Logger` context key).
#[derive(Clone)]
pub struct Logger(Arc<dyn Fn(&str) + Send + Sync>);

impl Logger {
    pub fn new(log: impl Fn(&str) + Send + Sync + 'static) -> Logger {
        Logger(Arc::new(log))
    }

    /// Prints each message on its own line to stdout (`console.log`).
    pub fn stdout() -> Logger {
        Logger::new(|message| println!("{message}"))
    }

    pub fn log(&self, message: &str) {
        (self.0)(message)
    }
}

/// `ctx.logger()`: the request's [Logger], or stdout when the middleware is not installed.
pub trait LoggerExt {
    fn logger(&self) -> Logger;
}

impl LoggerExt for Context {
    fn logger(&self) -> Logger {
        self.get::<Logger>().cloned().unwrap_or_else(Logger::stdout)
    }
}

/// The logger middleware; configure it with the builder methods.
#[derive(Clone)]
pub struct LoggerMiddleware {
    format: Arc<str>,
    log: Logger,
    colors: bool,
}

/// Log every request with [DEFAULT_FORMAT] to stdout, coloring when stdout is a terminal.
pub fn logger() -> LoggerMiddleware {
    LoggerMiddleware { format: DEFAULT_FORMAT.into(), log: Logger::stdout(), colors: rmx_terminal::stdout_colors() }
}

impl LoggerMiddleware {
    /// The message format. Tokens: `%date` (`dd/Mon/yyyy:HH:mm:ss +0000`), `%dateISO`,
    /// `%duration` (ms), `%contentLength`, `%contentType`, `%host`, `%hostname`, `%method`,
    /// `%path` (pathname and search), `%pathname`, `%port`, `%protocol`, `%query`,
    /// `%referer`, `%search`, `%status`, `%statusText`, `%url` and `%userAgent`. Unknown
    /// tokens and missing headers print `-`.
    pub fn format(mut self, format: &str) -> LoggerMiddleware {
        self.format = format.into();
        self
    }

    /// Where messages go (`log`). The default prints to stdout.
    pub fn log(mut self, log: impl Fn(&str) + Send + Sync + 'static) -> LoggerMiddleware {
        self.log = Logger::new(log);
        self
    }

    /// Force ANSI colors on or off for `%method`, `%status`, `%duration` and
    /// `%contentLength`. By default they are on when stdout is a terminal, unless `NO_COLOR`,
    /// `CI=true` or `TERM=dumb` is set; `FORCE_COLOR` turns them on.
    pub fn colors(mut self, colors: bool) -> LoggerMiddleware {
        self.colors = colors;
        self
    }

    fn message(&self, request: &RequestInfo, response: &Response, duration_ms: u128) -> String {
        let style = Style(Styles::new(self.colors));
        let headers = response.headers();
        let content_length = headers.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok());
        let url = &request.url;
        let token = |name: &str| -> Option<String> {
            Some(match name {
                "date" => {
                    let d = UtcDateTime::new(request.start);
                    format!("{:02}/{}/{}:{:02}:{:02}:{:02} +0000", d.day, d.month_name(), d.year, d.hour, d.minute, d.second)
                }
                "dateISO" => UtcDateTime::new(request.start).to_iso_string(),
                "duration" => style.duration(duration_ms),
                "contentLength" => style.content_length(content_length.unwrap_or("-")),
                "contentType" => headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("-").to_string(),
                "host" => url.host(),
                "hostname" => url.hostname().to_string(),
                "method" => style.method(&request.method),
                "path" => format!("{}{}", url.pathname(), url.search()),
                "pathname" => url.pathname().to_string(),
                "port" => url.port().map(|p| p.to_string()).unwrap_or_default(),
                "protocol" => format!("{}:", url.scheme()),
                "query" | "search" => url.search(),
                "referer" => request.referer.clone().unwrap_or_else(|| "-".into()),
                "status" => style.status(response.status().as_u16()),
                "statusText" => response.status().canonical_reason().unwrap_or("").to_string(),
                "url" => url.href(),
                "userAgent" => request.user_agent.clone().unwrap_or_else(|| "-".into()),
                _ => return None,
            })
        };
        replace_tokens(&self.format, |name| token(name).unwrap_or_else(|| "-".into()))
    }
}

struct RequestInfo {
    start: SystemTime,
    method: String,
    url: rmx_url::Url,
    referer: Option<String>,
    user_agent: Option<String>,
}

impl Middleware for LoggerMiddleware {
    fn call(&self, mut ctx: Context, next: Next) -> BoxFuture<Response> {
        let this = self.clone();
        Box::pin(async move {
            ctx.set(this.log.clone());
            let request = RequestInfo {
                start: SystemTime::now(),
                method: ctx.method().to_string(),
                url: ctx.url().clone(),
                referer: ctx.header(header::REFERER.as_str()).map(str::to_string),
                user_agent: ctx.header(header::USER_AGENT.as_str()).map(str::to_string),
            };
            let started = Instant::now();
            let response = next.run(ctx).await;
            this.log.log(&this.message(&request, &response, started.elapsed().as_millis()));
            response
        })
    }
}

/// Replace each `%word` (`/%(\w+)/g`) with `token(word)`.
fn replace_tokens(format: &str, token: impl Fn(&str) -> String) -> String {
    let mut out = String::with_capacity(format.len());
    let mut rest = format;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let len = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(after.len());
        if len == 0 {
            out.push('%');
        } else {
            out.push_str(&token(&after[..len]));
        }
        rest = &after[len..];
    }
    out.push_str(rest);
    out
}

/// The logger's token colors over [`Styles`] (Remix's logger uses `createStyles`).
struct Style(Styles);

impl Style {
    fn method(&self, method: &str) -> String {
        let s = &self.0;
        match method.to_ascii_uppercase().as_str() {
            "GET" | "HEAD" => s.green(method),
            "POST" => s.cyan(method),
            "PUT" | "PATCH" => s.yellow(method),
            "DELETE" => s.red(method),
            "OPTIONS" => s.magenta(method),
            _ => method.to_string(),
        }
    }

    fn status(&self, status: u16) -> String {
        let (s, value) = (&self.0, status.to_string());
        match status {
            500.. => s.red(&value),
            400.. => s.yellow(&value),
            300.. => s.cyan(&value),
            200.. => s.green(&value),
            _ => value,
        }
    }

    fn duration(&self, ms: u128) -> String {
        let (s, value) = (&self.0, ms.to_string());
        match ms {
            1000.. => s.red(&value),
            500.. => s.magenta(&value),
            100.. => s.yellow(&value),
            _ => s.green(&value),
        }
    }

    fn content_length(&self, value: &str) -> String {
        let s = &self.0;
        match value.parse::<u64>() {
            Ok(bytes) if bytes >= 1024 * 1024 => s.red(value),
            Ok(bytes) if bytes >= 100 * 1024 => s.yellow(value),
            Ok(bytes) if bytes >= 1024 => s.cyan(value),
            _ => value.to_string(),
        }
    }
}

#[cfg(test)]
mod tests;
