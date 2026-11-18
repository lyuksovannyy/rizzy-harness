//! The HTTP side of the master's MCP client for custom connectors
//! (ADOPTION_PLAN X.35, PLAN.md T84): our own implementation of rmcp's
//! [`StreamableHttpClient`], so rmcp's transport (sessions, JSON-RPC
//! matching, event streams) runs on rules we set, not on rmcp's own
//! reqwest client, which reads replies and error bodies whole.
//!
//! * **One URL.** Every request goes to the connector's URL as stored; a
//!   request the transport makes for any other URL is refused before it is
//!   sent, so the person's token goes to that origin only.
//! * **Our headers only**: `Accept` (JSON and event-stream), `Content-Type`
//!   (JSON, on a POST), `Mcp-Session-Id` (the server's, when it gave one),
//!   `MCP-Protocol-Version` (the version rmcp negotiated, checked), the
//!   token's header (marked sensitive) and our `User-Agent`. Anything else
//!   rmcp would add is dropped.
//! * **Every byte bounded.** One [`Budget`] of [`super::CHECK_BYTES`] for
//!   the whole check, shared by every request of it, applied to the raw
//!   body stream before any parsing (so the event-stream parser's line
//!   buffer is bounded by it too); a declared length over what is left is
//!   refused unread. Time: the client's own limits (`crate::remote`).
//! * **Never read**: a non-success answer's body (its status is all that
//!   is used), and a success of any content type other than JSON or
//!   `text/event-stream`.
//! * **No standalone stream**: [`get_stream`](StreamableHttpClient::get_stream)
//!   says the server has none, so rmcp never opens the GET stream for
//!   server-initiated messages; the master offers no capabilities, so
//!   there is nothing it would receive there.
//! * **Errors are ours**: [`Failure`] holds fixed words and numbers, never
//!   remote text; the first one of a check is kept in [`Http::failure`] for
//!   the caller, whatever rmcp makes of it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use futures::StreamExt as _;
use futures::stream::BoxStream;
use reqwest::header::{HeaderName, HeaderValue};
use rmcp::model::{ClientJsonRpcMessage, ServerJsonRpcMessage};
use rmcp::transport::streamable_http_client::{
    SseError, StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
};
use sse_stream::{Sse, SseStream};

use crate::lock;

const JSON: &str = "application/json";
const EVENTS: &str = "text/event-stream";
/// The longest session id or protocol version taken from the server.
const MAX_HEADER_VALUE: usize = 256;

/// Why a check failed: fixed words and numbers, never remote text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Failure {
    #[error(
        "it refused the token (HTTP {0}): it is wrong, expired or revoked, or this server needs \
         a sign-in this version does not do"
    )]
    Refused(u16),
    #[error("it answered HTTP {0}")]
    Status(u16),
    #[error("it redirected to another address (HTTP {0}), which is not followed")]
    Redirected(u16),
    #[error("it did not answer in time")]
    Timeout,
    #[error("the connection failed (or the address is not allowed)")]
    Connect,
    #[error("it sent more than this server reads in one check")]
    TooLarge,
    #[error("it answered with content this server does not read")]
    ContentType,
    #[error("it sent an answer this server does not accept: {0}")]
    BadAnswer(&'static str),
    #[error("it did not speak MCP as expected")]
    Protocol,
    #[error("the token cannot be sent in a header")]
    BadToken,
    #[error("the transport asked for another address, which is refused")]
    OtherUrl,
}

/// The bytes one check may still read, for all its requests together.
#[derive(Debug)]
pub struct Budget {
    left: AtomicUsize,
}

impl Budget {
    pub fn new(bytes: usize) -> Arc<Self> {
        Arc::new(Self {
            left: AtomicUsize::new(bytes),
        })
    }

    /// Take `n` bytes; false (and nothing taken) when fewer are left.
    fn take(&self, n: usize) -> bool {
        let mut left = self.left.load(Ordering::SeqCst);
        loop {
            let Some(after) = left.checked_sub(n) else {
                return false;
            };
            match self
                .left
                .compare_exchange_weak(left, after, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return true,
                Err(now) => left = now,
            }
        }
    }

    pub fn left(&self) -> usize {
        self.left.load(Ordering::SeqCst)
    }
}

/// The person's token, as the header the connector takes it in.
#[derive(Clone)]
pub struct TokenHeader {
    pub name: HeaderName,
    /// Marked sensitive.
    pub value: HeaderValue,
}

/// Our client: the rules' reqwest client, the one URL, the token header,
/// the check's budget and its first failure.
#[derive(Clone)]
pub struct Http {
    pub client: reqwest::Client,
    pub url: Arc<str>,
    pub token: Option<TokenHeader>,
    pub budget: Arc<Budget>,
    pub failure: Arc<Mutex<Option<Failure>>>,
}

impl Http {
    fn fail(&self, f: Failure) -> StreamableHttpError<Failure> {
        let mut g = lock(&self.failure);
        if g.is_none() {
            *g = Some(f.clone());
        }
        StreamableHttpError::Client(f)
    }

    /// The first failure of this check, if any.
    pub fn first_failure(&self) -> Option<Failure> {
        lock(&self.failure).clone()
    }

    /// Our headers on `rb`: the token, the session, the negotiated version.
    fn headers(
        &self,
        mut rb: reqwest::RequestBuilder,
        session_id: Option<&str>,
        custom: &HashMap<HeaderName, HeaderValue>,
    ) -> reqwest::RequestBuilder {
        if let Some(t) = &self.token {
            rb = rb.header(t.name.clone(), t.value.clone());
        }
        if let Some(s) = session_id {
            rb = rb.header("mcp-session-id", s);
        }
        // Of what rmcp adds, only the protocol version, and only a short
        // visible-ASCII one (it is the server's word, echoed).
        if let Some(v) = custom
            .iter()
            .find(|(k, _)| k.as_str() == "mcp-protocol-version")
            .map(|(_, v)| v)
            && v.len() <= 32
            && v.as_bytes().iter().all(|b| (0x21..=0x7e).contains(b))
        {
            rb = rb.header("mcp-protocol-version", v.clone());
        }
        rb
    }

    fn sent_error(&self, e: &reqwest::Error) -> StreamableHttpError<Failure> {
        self.fail(if e.is_timeout() {
            Failure::Timeout
        } else {
            Failure::Connect
        })
    }

    /// `r`'s body as a stream of at most what the budget has left; past
    /// it, one [`Failure::TooLarge`] and the end (the response, and with it
    /// the connection, is dropped).
    fn capped(&self, r: reqwest::Response) -> BoxStream<'static, Result<Bytes, Failure>> {
        let budget = self.budget.clone();
        let failure = self.failure.clone();
        let note = move |f: Failure| {
            let mut g = lock(&failure);
            if g.is_none() {
                *g = Some(f.clone());
            }
            f
        };
        futures::stream::unfold(Some(r.bytes_stream()), move |s| {
            let budget = budget.clone();
            let note = note.clone();
            async move {
                let mut s = s?;
                match s.next().await {
                    None => None,
                    Some(Ok(chunk)) if budget.take(chunk.len()) => Some((Ok(chunk), Some(s))),
                    Some(Ok(_)) => Some((Err(note(Failure::TooLarge)), None)),
                    Some(Err(e)) => Some((
                        Err(note(if e.is_timeout() {
                            Failure::Timeout
                        } else {
                            Failure::Connect
                        })),
                        None,
                    )),
                }
            }
        })
        .boxed()
    }
}

fn mime(r: &reqwest::Response) -> Option<String> {
    r.headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            v.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        })
}

impl StreamableHttpClient for Http {
    type Error = Failure;

    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        _auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Failure>> {
        if uri != self.url {
            return Err(self.fail(Failure::OtherUrl));
        }
        let body = serde_json::to_vec(&message).map_err(|_| self.fail(Failure::Protocol))?;
        let rb = self
            .client
            .post(self.url.as_ref())
            .header("accept", format!("{JSON}, {EVENTS}"))
            .header("content-type", JSON)
            .body(body);
        let rb = self.headers(rb, session_id.as_deref(), &custom_headers);
        let r = rb.send().await.map_err(|e| self.sent_error(&e))?;
        let status = r.status();
        if status.is_redirection() {
            return Err(self.fail(Failure::Redirected(status.as_u16())));
        }
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(self.fail(Failure::Refused(status.as_u16())));
        }
        if !status.is_success() {
            return Err(self.fail(Failure::Status(status.as_u16())));
        }
        if matches!(
            status,
            reqwest::StatusCode::ACCEPTED | reqwest::StatusCode::NO_CONTENT
        ) {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        let new_session = match r.headers().get("mcp-session-id") {
            None => None,
            Some(v)
                if v.len() <= MAX_HEADER_VALUE
                    && !v.is_empty()
                    && v.as_bytes().iter().all(|b| (0x21..=0x7e).contains(b)) =>
            {
                v.to_str().ok().map(str::to_string)
            }
            Some(_) => return Err(self.fail(Failure::BadAnswer("its session id"))),
        };
        let no_reply_needed = !matches!(message, ClientJsonRpcMessage::Request(_));
        if no_reply_needed && r.content_length() == Some(0) {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        if r.content_length()
            .is_some_and(|n| n > self.budget.left() as u64)
        {
            return Err(self.fail(Failure::TooLarge));
        }
        match mime(&r).as_deref() {
            Some(EVENTS) => {
                let events = SseStream::from_bytes_stream(self.capped(r))
                    .map(|e| e.map_err(|_| SseError::InvalidLine));
                Ok(StreamableHttpPostResponse::Sse(
                    boxed_events(events),
                    new_session,
                ))
            }
            Some(JSON) => {
                let mut body = Vec::new();
                let mut s = self.capped(r);
                while let Some(chunk) = s.next().await {
                    body.extend_from_slice(&chunk.map_err(|f| self.fail(f))?);
                }
                if no_reply_needed && body.iter().all(u8::is_ascii_whitespace) {
                    return Ok(StreamableHttpPostResponse::Accepted);
                }
                let m: ServerJsonRpcMessage = serde_json::from_slice(&body)
                    .map_err(|_| self.fail(Failure::BadAnswer("it is not a JSON-RPC message")))?;
                Ok(StreamableHttpPostResponse::Json(m, new_session))
            }
            _ if no_reply_needed => Ok(StreamableHttpPostResponse::Accepted),
            _ => Err(self.fail(Failure::ContentType)),
        }
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        _auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), StreamableHttpError<Failure>> {
        // A well-behaved server frees its session; the answer is not read
        // and a failure is nobody's business but the server's.
        if uri == self.url {
            let rb = self.client.delete(self.url.as_ref());
            let rb = self.headers(rb, Some(&session_id), &custom_headers);
            let _ = rb.send().await;
        }
        Ok(())
    }

    async fn get_stream(
        &self,
        _uri: Arc<str>,
        _session_id: Option<Arc<str>>,
        _last_event_id: Option<String>,
        _auth_header: Option<String>,
        _custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxStream<'static, Result<Sse, SseError>>, StreamableHttpError<Failure>> {
        Err(StreamableHttpError::ServerDoesNotSupportSse)
    }
}

fn boxed_events(
    s: impl futures::Stream<Item = Result<Sse, SseError>> + Send + 'static,
) -> BoxStream<'static, Result<Sse, SseError>> {
    s.boxed()
}
