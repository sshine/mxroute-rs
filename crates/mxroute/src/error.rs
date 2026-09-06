//! Error types.
//!
//! [`Error`] is the crate-wide error. The interesting variant is [`Error::Api`], which
//! carries the server's error document as an [`ApiError`]: a code, a message, and for a
//! validation failure the name of the field that was rejected.

use std::fmt;
use std::time::Duration;

use reqwest::StatusCode;
use serde::Deserialize;

use crate::ratelimit::Scope;

/// Result alias used throughout the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Anything that can go wrong talking to the MXroute API.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The request never produced a response, or the body could not be read.
    ///
    /// The URL carried by the underlying error has had its query string stripped, since
    /// `reqwest::Error` renders the URL in both `Debug` and `Display`. There is
    /// deliberately no `From` impl for `reqwest::Error`, so a stray `?` cannot smuggle an
    /// unscrubbed URL in.
    #[error("HTTP transport error")]
    Transport(#[source] reqwest::Error),

    /// A URL could not be constructed from the supplied path segments.
    #[error("could not build request URL")]
    Url(#[from] url::ParseError),

    /// The server answered with a status the client treats as an error.
    #[error("{method} {path} failed with HTTP {status}: {body}")]
    Api {
        /// Status code of the response.
        status: StatusCode,
        /// Request method, for context in logs.
        method: reqwest::Method,
        /// Request path, for context in logs. Query strings are stripped.
        path: String,
        /// The server's error document.
        #[source]
        body: ApiError,
    },

    /// The response body did not match the expected shape.
    #[error("could not decode response body as {expected}")]
    Decode {
        /// Name of the Rust type the body was being decoded into.
        expected: &'static str,
        /// The raw body, truncated to a sane length for diagnostics.
        body: String,
        /// The underlying serde error.
        #[source]
        source: serde_json::Error,
    },

    /// Serializing a request body failed.
    #[error("could not encode request body")]
    Encode(#[source] serde_json::Error),

    /// The client's own rate limiter would have to wait longer than
    /// [`max_rate_limit_wait`](crate::ClientBuilder::max_rate_limit_wait) allows.
    ///
    /// No request was sent. Retrying later is the only remedy.
    #[error(
        "local rate limit for the {scope} scope would block for {wait:.1?}, over the limit of {max_wait:.1?}"
    )]
    RateLimitWouldBlock {
        /// The scope whose allowance is spent.
        scope: Scope,
        /// How long the limiter wanted to sleep.
        wait: Duration,
        /// The configured ceiling.
        max_wait: Duration,
    },

    /// The server kept answering `429` until the retry budget ran out.
    #[error("still rate limited after {attempts} attempts")]
    RateLimited {
        /// Number of attempts made, including the first.
        attempts: u32,
        /// `Retry-After` from the final response, if it had one.
        retry_after: Option<Duration>,
        /// The final `429` body.
        #[source]
        body: ApiError,
    },

    /// A value failed client-side validation, so no request was sent.
    ///
    /// These are constraints the API documents and would reject anyway; checking them
    /// locally saves a request and a rate-limit slot.
    #[error("{0}")]
    Invalid(#[from] InvalidValue),
}

impl Error {
    /// Wraps a transport failure, dropping the URL's query string on the way in.
    ///
    /// `reqwest::Error` renders the URL it was working on in both `Debug` and `Display`.
    /// Host and path are kept, since those are what makes a transport failure
    /// diagnosable.
    pub(crate) fn transport(mut err: reqwest::Error) -> Self {
        if let Some(url) = err.url_mut() {
            url.set_query(None);
            url.set_fragment(None);
        }
        Self::Transport(err)
    }

    /// The HTTP status, when the error came from a response.
    pub fn status(&self) -> Option<StatusCode> {
        match self {
            Self::Api { status, .. } => Some(*status),
            Self::RateLimited { .. } => Some(StatusCode::TOO_MANY_REQUESTS),
            _ => None,
        }
    }

    /// True when the API said the resource does not exist, or is not ours.
    ///
    /// The API answers `404` for a domain owned by someone else as readily as for one
    /// that does not exist, so this does not distinguish "absent" from "not yours".
    pub fn is_not_found(&self) -> bool {
        self.status() == Some(StatusCode::NOT_FOUND)
    }

    /// True when the credentials were missing, malformed or rejected.
    ///
    /// A missing `X-Server` header is reported this way for every path, so an
    /// unauthorized error is not evidence that the endpoint exists.
    pub fn is_unauthorized(&self) -> bool {
        self.status() == Some(StatusCode::UNAUTHORIZED)
    }

    /// True when the credentials authenticated but are not allowed this call.
    ///
    /// The reseller endpoints answer this way for a non-reseller account.
    pub fn is_forbidden(&self) -> bool {
        self.status() == Some(StatusCode::FORBIDDEN)
    }

    /// True when the request was rejected by validation, whether locally or by the API.
    ///
    /// Covers the `400` the API uses for a malformed body and the `422` it uses for a
    /// body that parsed but named something that does not exist.
    pub fn is_validation(&self) -> bool {
        matches!(self, Self::Invalid(_))
            || matches!(
                self.status(),
                Some(StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY)
            )
    }

    /// True when the resource already exists.
    pub fn is_conflict(&self) -> bool {
        self.status() == Some(StatusCode::CONFLICT)
    }

    /// True for both local and remote rate limiting.
    pub fn is_rate_limited(&self) -> bool {
        matches!(
            self,
            Self::RateLimited { .. } | Self::RateLimitWouldBlock { .. }
        ) || self.status() == Some(StatusCode::TOO_MANY_REQUESTS)
    }

    /// True when a spam-settings update could not take the lock it shares with the panel.
    ///
    /// Nothing was necessarily left unchanged: the write may have been applied before the
    /// response was lost. Read the settings back rather than repeating the write.
    pub fn is_busy(&self) -> bool {
        self.status() == Some(StatusCode::SERVICE_UNAVAILABLE)
    }

    /// The server's error document, when there was one.
    pub fn api_error(&self) -> Option<&ApiError> {
        match self {
            Self::Api { body, .. } | Self::RateLimited { body, .. } => Some(body),
            _ => None,
        }
    }
}

/// A value rejected by client-side validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid {field}: {reason} (got {value:?})")]
pub struct InvalidValue {
    /// Which field or type rejected the value.
    pub field: &'static str,
    /// Why it was rejected.
    pub reason: &'static str,
    /// The offending value.
    pub value: String,
}

impl InvalidValue {
    pub(crate) fn new(field: &'static str, reason: &'static str, value: impl Into<String>) -> Self {
        Self {
            field,
            reason,
            value: value.into(),
        }
    }
}

/// Rejects the values that `url` collapses instead of encoding as a path segment.
///
/// `url::PathSegmentsMut::push` silently drops `.` and `..`, and an empty segment folds
/// away too, so `domains().delete("..")` would address the collection rather than an item.
/// Nothing else needs escaping: `push` percent-encodes `/` and `%`.
pub(crate) fn check_path_segment(field: &'static str, value: &str) -> Result<(), InvalidValue> {
    if matches!(value, "" | "." | "..") {
        return Err(InvalidValue::new(
            field,
            "is not addressable as a path segment",
            value,
        ));
    }
    Ok(())
}

/// The body of an error response.
///
/// Every documented error answers with `{"success": false, "error": {…}}`, so unlike a
/// Django-style API there is no tree to preserve — a code, a message and at most one
/// field name is the whole document.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub struct ApiError {
    /// The machine-readable code.
    pub code: ErrorCode,
    /// The human-readable message, meant for showing to a user.
    pub message: String,
    /// Which field was rejected. Only validation errors carry this.
    pub field: Option<String>,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.field {
            Some(field) => write!(f, "{} ({}: {})", self.message, self.code, field),
            None => write!(f, "{} ({})", self.message, self.code),
        }
    }
}

impl ApiError {
    /// Parses an error body, falling back to the raw text when it is not the documented
    /// shape.
    ///
    /// A proxy in front of the API, or the API's own unhandled failures, can answer with
    /// HTML or nothing at all; those become [`ErrorCode::Unparsed`] rather than a decode
    /// failure, so the status code stays reportable.
    ///
    /// Public so an error document obtained some other way — a log line, a captured
    /// response — can be inspected with the same accessors.
    pub fn parse(body: &str) -> Self {
        match serde_json::from_str::<ErrorEnvelope>(body) {
            Ok(envelope) => envelope.error,
            Err(_) => Self {
                code: ErrorCode::Unparsed,
                message: truncate(body.trim(), 2048),
                field: None,
            },
        }
    }
}

/// The `error` member of an error response.
///
/// Separate from [`ApiError`] so the envelope's `success` field is not part of the public
/// type, and so `Deserialize` cannot be reached for a body that was not an error.
#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ApiError,
}

impl<'de> Deserialize<'de> for ApiError {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            code: String,
            #[serde(default)]
            message: String,
            #[serde(default)]
            field: Option<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(Self {
            code: ErrorCode::from_wire(&raw.code),
            message: raw.message,
            field: raw.field,
        })
    }
}

/// The documented error codes.
///
/// [`Other`](ErrorCode::Other) keeps a code this crate has not been taught, so a new one
/// upstream is reported rather than turned into a decode failure.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorCode {
    /// Invalid input data. The [`field`](ApiError::field) names what was rejected.
    Validation,
    /// Invalid or missing credentials.
    Unauthorized,
    /// Insufficient permissions.
    Forbidden,
    /// Resource not found.
    NotFound,
    /// Resource already exists.
    Conflict,
    /// A business rule refused the request, such as an account at its domain limit.
    Business,
    /// Too many requests.
    RateLimited,
    /// Internal server error.
    Server,
    /// The account has no domain verification key.
    VerificationKeyNotFound,
    /// A code this crate does not know, as the server sent it.
    Other(String),
    /// The response body was not an error document at all.
    ///
    /// [`message`](ApiError::message) holds the body, truncated.
    Unparsed,
}

impl ErrorCode {
    fn from_wire(code: &str) -> Self {
        match code {
            "VALIDATION_ERROR" => Self::Validation,
            "UNAUTHORIZED" => Self::Unauthorized,
            "FORBIDDEN" => Self::Forbidden,
            "NOT_FOUND" => Self::NotFound,
            "CONFLICT" => Self::Conflict,
            "BUSINESS_ERROR" => Self::Business,
            "RATE_LIMITED" => Self::RateLimited,
            "SERVER_ERROR" => Self::Server,
            "VERIFICATION_KEY_NOT_FOUND" => Self::VerificationKeyNotFound,
            other => Self::Other(other.to_owned()),
        }
    }

    /// The code as the API spells it.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Validation => "VALIDATION_ERROR",
            Self::Unauthorized => "UNAUTHORIZED",
            Self::Forbidden => "FORBIDDEN",
            Self::NotFound => "NOT_FOUND",
            Self::Conflict => "CONFLICT",
            Self::Business => "BUSINESS_ERROR",
            Self::RateLimited => "RATE_LIMITED",
            Self::Server => "SERVER_ERROR",
            Self::VerificationKeyNotFound => "VERIFICATION_KEY_NOT_FOUND",
            Self::Other(code) => code,
            Self::Unparsed => "<no error document>",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Truncates on a character boundary, so error text from an unexpected body cannot panic
/// the formatter.
pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = s[..end].to_owned();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn a_documented_error_body_parses_into_its_code_and_field() {
        let err = ApiError::parse(
            r#"{"success": false, "error": {"code": "VALIDATION_ERROR",
               "message": "Password too weak", "field": "password"}}"#,
        );
        assert_eq!(err.code, ErrorCode::Validation);
        assert_eq!(err.message, "Password too weak");
        assert_eq!(err.field.as_deref(), Some("password"));
    }

    #[test]
    fn an_error_without_a_field_leaves_it_absent() {
        let err = ApiError::parse(
            r#"{"success": false, "error": {"code": "NOT_FOUND", "message": "No such domain"}}"#,
        );
        assert_eq!(err.code, ErrorCode::NotFound);
        assert_eq!(err.field, None);
        assert_eq!(err.to_string(), "No such domain (NOT_FOUND)");
    }

    #[test]
    fn an_unknown_code_is_kept_rather_than_rejected() {
        let err = ApiError::parse(r#"{"error": {"code": "TEAPOT", "message": "short and stout"}}"#);
        assert_eq!(err.code, ErrorCode::Other("TEAPOT".to_owned()));
        assert_eq!(err.code.as_str(), "TEAPOT");
    }

    #[test]
    fn a_body_that_is_not_an_error_document_keeps_the_text() {
        let err = ApiError::parse("<html><body>502 Bad Gateway</body></html>");
        assert_eq!(err.code, ErrorCode::Unparsed);
        assert!(err.message.contains("502 Bad Gateway"));
    }

    #[test]
    fn an_empty_body_parses_without_panicking() {
        let err = ApiError::parse("");
        assert_eq!(err.code, ErrorCode::Unparsed);
        assert_eq!(err.message, "");
    }

    #[test]
    fn truncation_lands_on_a_character_boundary() {
        // "æ" is two bytes, so a limit of 3 falls inside the second one.
        assert_eq!(truncate("æææ", 3), "æ…");
        assert_eq!(truncate("abc", 3), "abc");
    }

    #[test]
    fn path_segments_that_url_would_collapse_are_rejected() {
        for value in ["", ".", ".."] {
            assert!(check_path_segment("domain", value).is_err());
        }
        assert!(check_path_segment("domain", "example.com").is_ok());
        // A wildcard spam entry is a legitimate segment; `push` encodes what it must.
        assert!(check_path_segment("entry", "*@trusted.com").is_ok());
    }

    #[test]
    fn predicates_read_the_status_they_name() {
        let api = |status: StatusCode| Error::Api {
            status,
            method: reqwest::Method::GET,
            path: "/domains".to_owned(),
            body: ApiError::parse(""),
        };
        assert!(api(StatusCode::NOT_FOUND).is_not_found());
        assert!(api(StatusCode::UNAUTHORIZED).is_unauthorized());
        assert!(api(StatusCode::FORBIDDEN).is_forbidden());
        assert!(api(StatusCode::CONFLICT).is_conflict());
        assert!(api(StatusCode::TOO_MANY_REQUESTS).is_rate_limited());
        assert!(api(StatusCode::SERVICE_UNAVAILABLE).is_busy());
        // Both statuses the API uses to reject a body count as validation failures.
        assert!(api(StatusCode::BAD_REQUEST).is_validation());
        assert!(api(StatusCode::UNPROCESSABLE_ENTITY).is_validation());
        assert!(!api(StatusCode::NOT_FOUND).is_validation());
    }

    #[test]
    fn a_local_validation_failure_has_no_status_but_is_a_validation_error() {
        let err = Error::Invalid(InvalidValue::new("quota", "must not exceed 9600", "10000"));
        assert_eq!(err.status(), None);
        assert!(err.is_validation());
        assert!(err.api_error().is_none());
    }
}
