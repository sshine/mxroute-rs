//! The HTTP client: construction, authentication, retries and request execution.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use reqwest::header::{self, HeaderMap, HeaderName, HeaderValue};
use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tracing::Instrument;
use url::Url;

use crate::error::{ApiError, Error, InvalidValue, Result, truncate};
use crate::ratelimit::{Limiter, RateLimits, Scope};

/// The public MXroute API.
///
/// The path carries no version segment: endpoints sit at the root, and the version is
/// recorded only in the spec.
pub const DEFAULT_BASE_URL: &str = "https://api.mxroute.com";

/// `User-Agent` sent unless the builder overrides it.
pub const DEFAULT_USER_AGENT: &str = concat!("mxroute-rs/", env!("CARGO_PKG_VERSION"));

/// Ceiling applied to a `Retry-After` header.
///
/// No real throttle asks for longer than a day, and the value is used in `Instant`
/// arithmetic that panics on overflow, so an absurd header is clamped rather than trusted.
pub const MAX_RETRY_AFTER: Duration = Duration::from_secs(86_400);

/// Names the mail server a request is addressed to.
const HEADER_SERVER: HeaderName = HeaderName::from_static("x-server");
/// Names the DirectAdmin user a request authenticates as.
const HEADER_USERNAME: HeaderName = HeaderName::from_static("x-username");
/// Carries the API key.
const HEADER_API_KEY: HeaderName = HeaderName::from_static("x-api-key");

/// A credential that must not appear in logs.
///
/// `Debug` and `Display` both render a placeholder, which is what stops an API key from
/// riding along in a `{:?}` of a [`Credentials`] or an error. Reach for
/// [`expose`](Secret::expose) at the point of use, so every place a secret escapes is
/// greppable.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    /// Wraps a secret value.
    pub fn new(secret: impl Into<String>) -> Self {
        Self(secret.into())
    }

    /// The underlying value.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl<T: Into<String>> From<T> for Secret {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// What the API needs to identify a caller.
///
/// All three parts are mandatory on every request; the API has no anonymous endpoint, and
/// a request missing the server header is refused before it is routed. Find all three on
/// the API Keys page at <https://panel.mxroute.com/api-keys.php>.
///
/// ```
/// let credentials = mxroute::Credentials::new(
///     "eagle.mxlogin.com",
///     "johndoe",
///     "Mx8d989005f0cded8371b7d7271c50K1",
/// );
/// // The key is redacted wherever the credentials are rendered.
/// assert!(!format!("{credentials:?}").contains("Mx8d98"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    /// Mail server hostname, such as `eagle.mxlogin.com`. Sent as `X-Server`.
    pub server: String,
    /// DirectAdmin username. Sent as `X-Username`.
    pub username: String,
    /// The API key. Sent as `X-API-Key`.
    pub api_key: Secret,
}

impl Credentials {
    /// Assembles credentials from the three values the API Keys page shows.
    pub fn new(
        server: impl Into<String>,
        username: impl Into<String>,
        api_key: impl Into<Secret>,
    ) -> Self {
        Self {
            server: server.into(),
            username: username.into(),
            api_key: api_key.into(),
        }
    }

    /// Renders the three headers, rejecting values that cannot go in one.
    ///
    /// Done once at build time rather than per request: a credential holding a newline
    /// would otherwise fail deep inside reqwest, on every call, with no indication of
    /// which of the three was at fault.
    fn headers(&self) -> Result<HeaderMap> {
        fn value(field: &'static str, raw: &str, redact: bool) -> Result<HeaderValue> {
            HeaderValue::from_str(raw).map_err(|_| {
                Error::Invalid(InvalidValue::new(
                    field,
                    "contains characters that cannot go in an HTTP header",
                    if redact { "<redacted>" } else { raw },
                ))
            })
        }

        let mut headers = HeaderMap::with_capacity(3);
        headers.insert(HEADER_SERVER, value("server", &self.server, false)?);
        headers.insert(HEADER_USERNAME, value("username", &self.username, false)?);
        let mut key = value("api_key", self.api_key.expose(), true)?;
        // Marks the value sensitive so header maps rendered by other crates elide it.
        key.set_sensitive(true);
        headers.insert(HEADER_API_KEY, key);
        Ok(headers)
    }
}

/// Whether a request may be sent again after an unknown outcome.
///
/// This gates retries of `5xx` responses and mid-flight transport failures, where the
/// server may have processed the request before the failure. It deliberately does not gate
/// `429` retries: a throttled request was rejected before processing, so replaying any
/// method is safe.
///
/// `PATCH` is excluded. Every write this API exposes through `PATCH` is a partial update
/// whose outcome cannot be inferred from a lost response, and the spam settings say so
/// outright: their writes are serialized against the panel, and a caller is told to read
/// the settings back rather than repeat the write.
fn is_replayable(method: &Method) -> bool {
    matches!(
        *method,
        Method::GET | Method::HEAD | Method::PUT | Method::DELETE | Method::OPTIONS
    )
}

/// Retry behaviour for throttled and transiently failed requests.
#[derive(Debug, Clone)]
pub(crate) struct RetryConfig {
    /// Retries after the first attempt. Zero disables retrying.
    pub(crate) max_retries: u32,
    /// Longest single sleep the client will accept, whether from `Retry-After` or backoff.
    /// A longer wait fails instead.
    pub(crate) max_delay: Duration,
    /// First backoff step for server errors; doubles per attempt.
    pub(crate) initial_backoff: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            max_delay: Duration::from_secs(60),
            initial_backoff: Duration::from_millis(500),
        }
    }
}

#[derive(Debug)]
struct Inner {
    http: reqwest::Client,
    base: Url,
    auth: HeaderMap,
    /// Shared so that a client derived by [`Client::with_credentials`] paces itself
    /// against the same buckets: the throttle follows the account and the source address,
    /// not the individual key.
    limiter: Arc<Limiter>,
    /// Every attempt the limiter admitted, shared for the same reason `limiter` is.
    requests: Arc<AtomicU64>,
    retry: RetryConfig,
}

/// An asynchronous MXroute API client.
///
/// Cheap to clone: clones share one connection pool, one rate-limiter state and one
/// request count, which is what makes the client-side limits meaningful across concurrent
/// tasks.
///
/// ```no_run
/// # async fn run() -> Result<(), mxroute::Error> {
/// let client = mxroute::Client::new(mxroute::Credentials::new(
///     "eagle.mxlogin.com",
///     "johndoe",
///     "Mx8d989005f0cded8371b7d7271c50K1",
/// ))?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

impl Client {
    /// Starts building a client.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// A client with every default in place.
    pub fn new(credentials: Credentials) -> Result<Self> {
        Self::builder().credentials(credentials).build()
    }

    /// The base URL requests are built against.
    pub fn base_url(&self) -> &Url {
        &self.inner.base
    }

    /// The limits this client paces itself against.
    #[cfg(test)]
    pub(crate) fn rate_limits(&self) -> &RateLimits {
        self.inner.limiter.limits()
    }

    /// How many HTTP requests this client has sent since it was built.
    ///
    /// Attempts, not operations: a retry and a throttle replay are each a request, and a
    /// caller counting its own calls cannot see the difference. Requests the local limiter
    /// refused are not counted, because they never reached the network.
    pub fn requests_made(&self) -> u64 {
        self.inner.requests.load(Ordering::Relaxed)
    }

    /// A client identical to this one but authenticating as someone else.
    ///
    /// Shares the connection pool, the rate-limiter state and the request count. This is
    /// how one process manages several mail servers without each client pacing itself in
    /// ignorance of the others.
    pub fn with_credentials(&self, credentials: Credentials) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(Inner {
                http: self.inner.http.clone(),
                base: self.inner.base.clone(),
                auth: credentials.headers()?,
                limiter: Arc::clone(&self.inner.limiter),
                requests: Arc::clone(&self.inner.requests),
                retry: self.inner.retry.clone(),
            }),
        })
    }

    /// Builds a request URL by appending percent-encoded path segments to the base.
    ///
    /// No trailing slash: this API's routes do not carry one, and adding it would turn
    /// every path into a redirect or a `404`.
    pub(crate) fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.inner.base.clone();
        {
            // The builder rejects any base URL that cannot be a base, so this holds.
            #[expect(clippy::expect_used)]
            let mut path = url
                .path_segments_mut()
                .expect("base URL was validated as a base");
            for segment in segments {
                path.push(segment);
            }
        }
        url
    }

    pub(crate) fn request(&self, method: Method, url: Url) -> Req {
        Req {
            scope: Scope::of(&method),
            method,
            url,
            body: None,
        }
    }

    /// Sends a request, applying rate limits and retries, and maps an error status onto
    /// [`Error::Api`].
    pub(crate) async fn send(&self, req: Req) -> Result<Res> {
        let res = self.execute(req).await?;
        if res.status.is_client_error() || res.status.is_server_error() {
            return Err(res.to_api_error());
        }
        Ok(res)
    }

    /// Sends a request and decodes the `data` member of the response envelope.
    pub(crate) async fn send_json<T: DeserializeOwned>(&self, req: Req) -> Result<T> {
        self.send(req).await?.data()
    }

    /// Sends a request and decodes the `data` member, mapping `404` onto `None`.
    pub(crate) async fn send_json_opt<T: DeserializeOwned>(&self, req: Req) -> Result<Option<T>> {
        match self.send(req).await {
            Ok(res) => res.data().map(Some),
            Err(err) if err.is_not_found() => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Sends a request and discards the body.
    ///
    /// Used by the writes whose success response the API documents without a body, where
    /// insisting on a shape would turn an undocumented payload into a failure.
    pub(crate) async fn send_empty(&self, req: Req) -> Result<()> {
        self.send(req).await.map(drop)
    }

    /// One request, including rate limiting and retries. Status is not interpreted here.
    async fn execute(&self, req: Req) -> Result<Res> {
        let Req {
            method,
            url,
            body,
            scope,
        } = req;
        let path = url.path().to_owned();

        let span = tracing::debug_span!(
            "mxroute.request",
            http.method = %method,
            url.path = %path,
        );

        async move {
            let mut attempt = 0u32;
            loop {
                attempt += 1;
                self.inner.limiter.acquire(scope).await?;
                // After the limiter admits the attempt, so a call refused locally is not
                // counted against an allowance it never spent.
                self.inner.requests.fetch_add(1, Ordering::Relaxed);

                let mut builder = self
                    .inner
                    .http
                    .request(method.clone(), url.clone())
                    .headers(self.inner.auth.clone());
                if let Some(body) = &body {
                    builder = builder
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(body.clone());
                }

                let outcome = match builder.send().await {
                    Ok(response) => {
                        let status = response.status();
                        let headers = response.headers().clone();
                        response.bytes().await.map(|bytes| Res {
                            status,
                            headers,
                            body: bytes.to_vec(),
                            method: method.clone(),
                            path: path.clone(),
                        })
                    }
                    Err(err) => Err(err),
                };

                let res = match outcome {
                    Ok(res) => res,
                    Err(err) => {
                        // A connect or timeout failure may still have been processed by
                        // the server, so replaying it is only safe for an idempotent
                        // method. A malformed URL or a decode failure is never worth a
                        // second attempt.
                        let transient = err.is_timeout() || err.is_connect() || err.is_request();
                        let retryable = transient && is_replayable(&method);
                        // Scrubbed before logging, because the reqwest error's own
                        // rendering would otherwise carry the query string.
                        let err = Error::transport(err);
                        if retryable && attempt <= self.inner.retry.max_retries {
                            let delay = self.backoff(attempt);
                            tracing::warn!(
                                attempt,
                                delay_ms = delay.as_millis(),
                                error = %err,
                                "request failed, retrying"
                            );
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        return Err(err);
                    }
                };

                tracing::debug!(
                    attempt,
                    http.status = res.status.as_u16(),
                    body_bytes = res.body.len(),
                    "response"
                );

                // Every response carries the allowance, not just the throttled ones, so
                // this is where the client learns that something else spent it.
                self.inner.limiter.observe(scope, &res.headers);

                if res.status == StatusCode::TOO_MANY_REQUESTS {
                    let retry_after = res.retry_after();
                    if let Some(retry_after) = retry_after {
                        self.inner.limiter.record_throttled(scope, retry_after);
                    }

                    let delay = retry_after.unwrap_or_else(|| self.backoff(attempt));
                    if attempt > self.inner.retry.max_retries || delay > self.inner.retry.max_delay
                    {
                        tracing::warn!(
                            attempt,
                            retry_after_s = retry_after.map(|d| d.as_secs()),
                            "giving up on a throttled request"
                        );
                        return Err(Error::RateLimited {
                            attempts: attempt,
                            retry_after,
                            body: ApiError::parse(&res.text_lossy()),
                        });
                    }
                    tracing::info!(
                        attempt,
                        delay_ms = delay.as_millis(),
                        "throttled by the server, waiting"
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }

                // 5xx is worth a retry, but only where replaying is safe: the server may
                // have processed the request before failing. 4xx other than 429 will not
                // change on its own.
                if res.status.is_server_error()
                    && is_replayable(&method)
                    && attempt <= self.inner.retry.max_retries
                {
                    let delay = self.backoff(attempt);
                    tracing::warn!(
                        attempt,
                        http.status = res.status.as_u16(),
                        delay_ms = delay.as_millis(),
                        "server error, retrying"
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }

                return Ok(res);
            }
        }
        .instrument(span)
        .await
    }

    /// Exponential backoff, capped at the configured ceiling.
    fn backoff(&self, attempt: u32) -> Duration {
        let factor = 1u32 << attempt.min(16).saturating_sub(1);
        self.inner
            .retry
            .initial_backoff
            .saturating_mul(factor)
            .min(self.inner.retry.max_delay)
    }
}

/// A request under construction.
pub(crate) struct Req {
    method: Method,
    url: Url,
    body: Option<Vec<u8>>,
    scope: Scope,
}

impl Req {
    /// Serializes `body` as the JSON request body.
    pub(crate) fn json<T: Serialize + ?Sized>(mut self, body: &T) -> Result<Self> {
        self.body = Some(serde_json::to_vec(body).map_err(Error::Encode)?);
        Ok(self)
    }
}

/// The shape every endpoint but the quota pair answers with.
///
/// `success` defaults to true: the API's own schema marks neither member required, and a
/// 2xx that omitted the flag has already said as much in its status line.
#[derive(serde::Deserialize)]
struct Envelope<T> {
    #[serde(default = "succeeded")]
    success: bool,
    data: T,
}

fn succeeded() -> bool {
    true
}

/// A response whose body has been read into memory.
pub(crate) struct Res {
    pub(crate) status: StatusCode,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Vec<u8>,
    pub(crate) method: Method,
    pub(crate) path: String,
}

impl Res {
    /// Decodes the whole body as `T`.
    pub(crate) fn decode<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(&self.body).map_err(|source| Error::Decode {
            expected: std::any::type_name::<T>(),
            body: truncate(&self.text_lossy(), 2048),
            source,
        })
    }

    /// Decodes the `data` member of the success envelope.
    ///
    /// A 2xx body that says `"success": false` is a contradiction the API does not
    /// document; it is reported as the error it claims to be rather than returned as data.
    pub(crate) fn data<T: DeserializeOwned>(&self) -> Result<T> {
        let envelope: Envelope<T> = self.decode()?;
        if !envelope.success {
            return Err(self.to_api_error());
        }
        Ok(envelope.data)
    }

    pub(crate) fn text_lossy(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// Builds the error for a response the client treats as a failure.
    fn to_api_error(&self) -> Error {
        Error::Api {
            status: self.status,
            method: self.method.clone(),
            path: self.path.clone(),
            body: ApiError::parse(&self.text_lossy()),
        }
    }

    /// `Retry-After` as a duration, accepting both forms the HTTP spec allows.
    fn retry_after(&self) -> Option<Duration> {
        let raw = self.headers.get(header::RETRY_AFTER)?.to_str().ok()?;
        parse_retry_after(raw, chrono::Utc::now())
    }
}

/// `Retry-After` as a duration, with the date form resolved against `now`.
///
/// Clamped to [`MAX_RETRY_AFTER`]. The header is proxy-controlled and otherwise unbounded,
/// and the value reaches `Instant` arithmetic, which panics on overflow. A deadline already
/// in the past yields `None`, leaving the caller on its own backoff.
///
/// `now` is a parameter rather than a call to the wall clock so the date form can be
/// pinned in tests.
fn parse_retry_after(raw: &str, now: chrono::DateTime<chrono::Utc>) -> Option<Duration> {
    let raw = raw.trim();
    if let Ok(secs) = raw.parse::<u64>() {
        return Some(Duration::from_secs(secs).min(MAX_RETRY_AFTER));
    }
    // An HTTP-date, which MXroute does not currently send but the spec permits.
    let deadline = chrono::DateTime::parse_from_rfc2822(raw).ok()?;
    let delta = deadline.signed_duration_since(now);
    Some(delta.to_std().ok()?.min(MAX_RETRY_AFTER))
}

/// Builds a [`Client`].
#[derive(Debug, Default)]
pub struct ClientBuilder {
    base: Option<String>,
    credentials: Option<Credentials>,
    user_agent: Option<String>,
    timeout: Option<Duration>,
    rate_limits: Option<RateLimits>,
    max_rate_limit_wait: Option<Duration>,
    retry: RetryConfig,
    http: Option<reqwest::Client>,
}

impl ClientBuilder {
    /// Sets the credentials. Required: [`build`](Self::build) fails without them.
    pub fn credentials(mut self, credentials: Credentials) -> Self {
        self.credentials = Some(credentials);
        self
    }

    /// Overrides the API root. Defaults to [`DEFAULT_BASE_URL`].
    ///
    /// Point this at a mock server in tests.
    pub fn base_url(mut self, base: impl Into<String>) -> Self {
        self.base = Some(base.into());
        self
    }

    /// Overrides the `User-Agent`.
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = Some(user_agent.into());
        self
    }

    /// Total timeout per attempt.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Replaces the client-side rate limits.
    ///
    /// Defaults to [`RateLimits::mxroute_defaults`]. Pass [`RateLimits::unlimited`] to
    /// send requests as fast as the caller asks and deal with `429`s reactively.
    pub fn rate_limits(mut self, limits: RateLimits) -> Self {
        self.rate_limits = Some(limits);
        self
    }

    /// Longest the client-side limiter may sleep before giving up with
    /// [`Error::RateLimitWouldBlock`].
    ///
    /// Defaults to 60 seconds, which is long enough to wait out either of the API's
    /// per-minute buckets.
    pub fn max_rate_limit_wait(mut self, max_wait: Duration) -> Self {
        self.max_rate_limit_wait = Some(max_wait);
        self
    }

    /// Retries after the first attempt, for `429`s, `5xx`s and connection failures.
    /// Defaults to 3; zero disables retrying.
    pub fn max_retries(mut self, retries: u32) -> Self {
        self.retry.max_retries = retries;
        self
    }

    /// Longest single retry sleep to accept. Defaults to 60 seconds.
    ///
    /// A `Retry-After` longer than this fails with [`Error::RateLimited`] instead of
    /// blocking.
    pub fn max_retry_delay(mut self, delay: Duration) -> Self {
        self.retry.max_delay = delay;
        self
    }

    /// Supplies a preconfigured [`reqwest::Client`], for proxy or TLS settings this
    /// builder does not expose. Overrides [`timeout`](Self::timeout) and
    /// [`user_agent`](Self::user_agent).
    pub fn http_client(mut self, http: reqwest::Client) -> Self {
        self.http = Some(http);
        self
    }

    /// Finishes the client.
    pub fn build(self) -> Result<Client> {
        let credentials = self.credentials.ok_or_else(|| {
            InvalidValue::new(
                "credentials",
                "are required: every endpoint authenticates",
                "<unset>",
            )
        })?;
        let auth = credentials.headers()?;

        let raw = self.base.as_deref().unwrap_or(DEFAULT_BASE_URL);
        let mut base = Url::parse(raw)?;
        if !matches!(base.scheme(), "http" | "https") {
            return Err(InvalidValue::new("base_url", "must be http or https", raw).into());
        }
        {
            // Normalize away a trailing slash so appending segments cannot produce `//`.
            let mut segments = base
                .path_segments_mut()
                .map_err(|()| InvalidValue::new("base_url", "cannot be a base URL", raw))?;
            segments.pop_if_empty();
        }
        base.set_query(None);
        base.set_fragment(None);

        let http = match self.http {
            Some(http) => http,
            None => {
                let mut headers = HeaderMap::new();
                headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
                let mut builder = reqwest::Client::builder()
                    .user_agent(self.user_agent.as_deref().unwrap_or(DEFAULT_USER_AGENT))
                    .default_headers(headers);
                if let Some(timeout) = self.timeout {
                    builder = builder.timeout(timeout);
                }
                builder.build().map_err(Error::transport)?
            }
        };

        let limits = self.rate_limits.unwrap_or_default();
        let max_wait = self
            .max_rate_limit_wait
            .unwrap_or_else(|| Duration::from_secs(60));

        Ok(Client {
            inner: Arc::new(Inner {
                http,
                base,
                auth,
                limiter: Arc::new(Limiter::new(limits, max_wait)),
                requests: Arc::new(AtomicU64::new(0)),
                retry: self.retry,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn credentials() -> Credentials {
        Credentials::new("eagle.mxlogin.com", "johndoe", "Mx8d989005f0cded")
    }

    fn client() -> Client {
        Client::builder()
            .credentials(credentials())
            .build()
            .expect("valid configuration")
    }

    #[test]
    fn secrets_do_not_leak_through_debug_or_display() {
        let secret = Secret::new("Mx8d989005f0cded8371b7d7271c50K1");
        assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
        assert_eq!(secret.to_string(), "<redacted>");
        assert!(!format!("{secret:?} {secret}").contains("Mx8d98"));
    }

    #[test]
    fn neither_the_credentials_nor_the_client_render_the_key() {
        assert!(!format!("{:?}", credentials()).contains("Mx8d98"));
        let rendered = format!("{:?}", client());
        assert!(!rendered.contains("Mx8d98"), "{rendered}");
    }

    #[test]
    fn all_three_headers_are_sent_and_the_key_is_marked_sensitive() {
        let headers = credentials().headers().expect("well-formed credentials");
        assert_eq!(
            headers.get(HEADER_SERVER).map(HeaderValue::as_bytes),
            Some(&b"eagle.mxlogin.com"[..])
        );
        assert_eq!(
            headers.get(HEADER_USERNAME).map(HeaderValue::as_bytes),
            Some(&b"johndoe"[..])
        );
        let key = headers.get(HEADER_API_KEY).expect("the key header is set");
        assert_eq!(key.as_bytes(), b"Mx8d989005f0cded");
        assert!(key.is_sensitive());
    }

    #[test]
    fn a_credential_that_cannot_go_in_a_header_fails_at_build_time() {
        let err = Client::builder()
            .credentials(Credentials::new("eagle.mxlogin.com", "john\ndoe", "key"))
            .build()
            .expect_err("a newline cannot go in a header");
        assert!(err.is_validation());
        assert!(err.to_string().contains("username"), "{err}");
    }

    #[test]
    fn a_bad_api_key_is_reported_without_being_quoted_back() {
        let err = Client::builder()
            .credentials(Credentials::new("eagle.mxlogin.com", "johndoe", "ke\ny"))
            .build()
            .expect_err("a newline cannot go in a header");
        assert!(err.to_string().contains("api_key"), "{err}");
        assert!(!err.to_string().contains("ke\ny"), "{err}");
    }

    #[test]
    fn building_without_credentials_fails_rather_than_sending_anonymous_requests() {
        let err = Client::builder()
            .build()
            .expect_err("there is no anonymous endpoint");
        assert!(err.is_validation());
        assert!(err.to_string().contains("credentials"), "{err}");
    }

    #[test]
    fn urls_append_segments_without_a_trailing_slash() {
        let client = client();
        assert_eq!(
            client.url(&["domains"]).as_str(),
            "https://api.mxroute.com/domains"
        );
        assert_eq!(
            client
                .url(&["domains", "example.com", "email-accounts"])
                .as_str(),
            "https://api.mxroute.com/domains/example.com/email-accounts"
        );
    }

    #[test]
    fn path_segments_are_percent_encoded() {
        let client = client();
        // A wildcard spam entry has to survive the trip as one segment.
        assert_eq!(
            client
                .url(&["domains", "a.com", "spam", "whitelist", "*@b.com"])
                .as_str(),
            "https://api.mxroute.com/domains/a.com/spam/whitelist/*@b.com"
        );
        // A slash would otherwise invent a path segment.
        assert_eq!(
            client.url(&["domains", "a/b"]).as_str(),
            "https://api.mxroute.com/domains/a%2Fb"
        );
    }

    #[test]
    fn a_base_url_with_a_trailing_slash_does_not_double_it() {
        let client = Client::builder()
            .credentials(credentials())
            .base_url("http://127.0.0.1:8080/")
            .build()
            .expect("valid configuration");
        assert_eq!(
            client.url(&["domains"]).as_str(),
            "http://127.0.0.1:8080/domains"
        );
    }

    #[test]
    fn a_base_url_that_is_not_http_is_rejected() {
        for base in ["ftp://example.com", "mailto:someone@example.com"] {
            assert!(
                Client::builder()
                    .credentials(credentials())
                    .base_url(base)
                    .build()
                    .is_err(),
                "{base} should not be accepted"
            );
        }
    }

    #[test]
    fn requests_are_scoped_by_whether_they_change_anything() {
        let client = client();
        let url = client.url(&["domains"]);
        assert_eq!(client.request(Method::GET, url.clone()).scope, Scope::Read);
        assert_eq!(
            client.request(Method::POST, url.clone()).scope,
            Scope::Write
        );
        assert_eq!(client.request(Method::DELETE, url).scope, Scope::Write);
    }

    #[test]
    fn only_methods_safe_to_replay_are_retried() {
        assert!(is_replayable(&Method::GET));
        assert!(is_replayable(&Method::DELETE));
        assert!(is_replayable(&Method::HEAD));
        assert!(!is_replayable(&Method::POST));
        // The spam settings are the reason: their writes cannot be repeated blindly.
        assert!(!is_replayable(&Method::PATCH));
    }

    #[test]
    fn backoff_doubles_until_it_hits_the_ceiling() {
        let client = Client::builder()
            .credentials(credentials())
            .max_retry_delay(Duration::from_secs(2))
            .build()
            .expect("valid configuration");
        assert_eq!(client.backoff(1), Duration::from_millis(500));
        assert_eq!(client.backoff(2), Duration::from_secs(1));
        assert_eq!(client.backoff(3), Duration::from_secs(2));
        // Capped, and a large attempt count cannot overflow the shift.
        assert_eq!(client.backoff(99), Duration::from_secs(2));
    }

    #[test]
    fn retry_after_accepts_both_forms_and_is_clamped() {
        let now = chrono::Utc::now();
        assert_eq!(parse_retry_after("30", now), Some(Duration::from_secs(30)));
        assert_eq!(
            parse_retry_after("  30  ", now),
            Some(Duration::from_secs(30))
        );
        assert_eq!(parse_retry_after("999999999", now), Some(MAX_RETRY_AFTER));
        assert_eq!(parse_retry_after("soon", now), None);

        let deadline = now + chrono::Duration::seconds(120);
        let parsed = parse_retry_after(&deadline.to_rfc2822(), now).expect("an HTTP-date parses");
        assert!(parsed <= Duration::from_secs(120) && parsed >= Duration::from_secs(119));

        // A date already gone yields nothing rather than a negative wait.
        let past = now - chrono::Duration::seconds(60);
        assert_eq!(parse_retry_after(&past.to_rfc2822(), now), None);
    }

    #[test]
    fn a_fresh_client_has_sent_nothing() {
        assert_eq!(client().requests_made(), 0);
    }

    #[test]
    fn the_defaults_are_the_documented_rates() {
        assert_eq!(client().rate_limits(), &RateLimits::mxroute_defaults());
    }

    #[test]
    fn a_derived_client_shares_the_limiter_and_the_request_count() {
        let first = client();
        let second = first
            .with_credentials(Credentials::new("hawk.mxlogin.com", "janedoe", "key"))
            .expect("valid configuration");
        assert!(Arc::ptr_eq(&first.inner.limiter, &second.inner.limiter));
        assert!(Arc::ptr_eq(&first.inner.requests, &second.inner.requests));
    }
}
