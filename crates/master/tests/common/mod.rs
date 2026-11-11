//! Shared by the HTTP tests.

/// Send `Origin` as the page served from the request's own URL would: the
/// server refuses cookie-authenticated writes and WebSockets without one
/// (AUDIT L6). Call it after the URL and before a streaming body; a request
/// that already names an Origin keeps it.
pub trait SameOrigin {
    fn same_origin(self) -> Self;
}

impl SameOrigin for reqwest::RequestBuilder {
    fn same_origin(self) -> Self {
        let Some(req) = self.try_clone().and_then(|b| b.build().ok()) else {
            return self;
        };
        if req.headers().contains_key(reqwest::header::ORIGIN) {
            return self;
        }
        let origin = req.url().origin().ascii_serialization();
        self.header(reqwest::header::ORIGIN, origin)
    }
}
