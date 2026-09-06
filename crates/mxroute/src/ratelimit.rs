//! Client-side pacing against the API's documented throttles.
//!
//! Two things happen here. The client keeps a sliding log per scope so it paces itself
//! rather than collecting `429`s, and it reads the `X-RateLimit-*` headers the API returns
//! on every response so that a limit already spent by something else sharing the account
//! is respected too.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Method;
use reqwest::header::HeaderMap;
use tokio::time::Instant;

use crate::error::{Error, InvalidValue, Result};

/// Longest window [`Rate::new`] accepts.
const MAX_PERIOD: Duration = Duration::from_secs(366 * 86_400);

/// The throttle a request counts against.
///
/// The API documents exactly two, split by whether the request changes anything. Both are
/// account-wide, so there is no per-resource bucket to key on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Scope {
    /// `GET`, documented at 100 requests per minute.
    Read,
    /// `POST`, `PATCH` and `DELETE`, documented at 20 requests per minute.
    Write,
}

impl Scope {
    /// The scope a method counts against.
    ///
    /// Anything that is not a read is treated as a write, which is the safe direction:
    /// the write allowance is the smaller of the two.
    pub(crate) fn of(method: &Method) -> Self {
        match *method {
            Method::GET | Method::HEAD | Method::OPTIONS => Self::Read,
            _ => Self::Write,
        }
    }

    /// The scope name, for logs and error messages.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A limit of `limit` requests per `period`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    limit: u32,
    period: Duration,
}

impl Rate {
    /// Builds a rate, rejecting a zero limit or period since neither could ever admit a
    /// request.
    pub fn new(limit: u32, period: Duration) -> Result<Self, InvalidValue> {
        if limit == 0 {
            return Err(InvalidValue::new(
                "rate",
                "limit must be greater than zero",
                limit.to_string(),
            ));
        }
        if period.is_zero() {
            return Err(InvalidValue::new(
                "rate",
                "period must be greater than zero",
                "0",
            ));
        }
        // Bounded because the period is added to an `Instant` to find the next free slot,
        // and that panics on overflow. No throttling window is longer than a year.
        if period > MAX_PERIOD {
            return Err(InvalidValue::new(
                "rate",
                "period must be at most 366 days",
                format!("{period:?}"),
            ));
        }
        Ok(Self { limit, period })
    }

    /// Requests permitted per period.
    pub fn limit(self) -> u32 {
        self.limit
    }

    /// Length of the sliding window.
    pub fn period(self) -> Duration {
        self.period
    }
}

impl FromStr for Rate {
    type Err = InvalidValue;

    /// Parses `100/min`, `20/min`, `10/s`, `2/2min` and the like.
    ///
    /// The period may carry a multiplier, so a window of several minutes is expressible.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || InvalidValue::new("rate", "expected a rate like `100/min` or `2/2min`", s);

        let (limit, period) = s.split_once('/').ok_or_else(invalid)?;
        let limit: u32 = limit.trim().parse().map_err(|_| invalid())?;

        let period = period.trim();
        let split = period
            .find(|c: char| !c.is_ascii_digit())
            .ok_or_else(invalid)?;
        let (count, unit) = period.split_at(split);
        let count: u32 = if count.is_empty() {
            1
        } else {
            count.parse().map_err(|_| invalid())?
        };

        let unit = match unit {
            "s" | "sec" | "second" | "seconds" => Duration::from_secs(1),
            "m" | "min" | "minute" | "minutes" => Duration::from_secs(60),
            "h" | "hour" | "hours" => Duration::from_secs(3600),
            "d" | "day" | "days" => Duration::from_secs(86_400),
            _ => return Err(invalid()),
        };

        Self::new(limit, unit * count)
    }
}

impl fmt::Display for Rate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let secs = self.period.as_secs();
        let (count, unit) = match secs {
            0 => (self.period.as_millis(), "ms"),
            s if s % 86_400 == 0 => ((s / 86_400).into(), "day"),
            s if s % 3600 == 0 => ((s / 3600).into(), "h"),
            s if s % 60 == 0 => ((s / 60).into(), "min"),
            s => (s.into(), "s"),
        };
        if count == 1 {
            write!(f, "{}/{unit}", self.limit)
        } else {
            write!(f, "{}/{count}{unit}", self.limit)
        }
    }
}

/// The rates to enforce for each scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimits {
    scopes: HashMap<Scope, Vec<Rate>>,
}

impl Default for RateLimits {
    fn default() -> Self {
        Self::mxroute_defaults()
    }
}

impl RateLimits {
    /// The rates MXroute documents, as of the API version this crate targets.
    ///
    /// These are what the server enforces, so a client configured with them should rarely
    /// see a `429` — only when something else shares the account.
    pub fn mxroute_defaults() -> Self {
        // Parsing string literals keeps these readable against the documentation table;
        // the expect cannot fire because every literal is well-formed, and a unit test
        // below pins that.
        #[expect(clippy::expect_used)]
        fn rate(spec: &str) -> Vec<Rate> {
            vec![spec.parse().expect("built-in rate literal is well-formed")]
        }

        Self {
            scopes: [
                (Scope::Read, rate("100/min")),
                (Scope::Write, rate("20/min")),
            ]
            .into_iter()
            .collect(),
        }
    }

    /// No client-side limiting at all.
    ///
    /// This stops the client pacing itself against the documented rates. It does not
    /// discard what the server says: a `429` and an exhausted `X-RateLimit-Remaining`
    /// still back the scope off, because those describe a throttle that has already
    /// happened rather than one being predicted.
    pub fn unlimited() -> Self {
        Self {
            scopes: HashMap::new(),
        }
    }

    /// Replaces the rates for one scope. An empty iterator removes the limit.
    pub fn with_scope(mut self, scope: Scope, rates: impl IntoIterator<Item = Rate>) -> Self {
        let rates: Vec<_> = rates.into_iter().collect();
        if rates.is_empty() {
            self.scopes.remove(&scope);
        } else {
            self.scopes.insert(scope, rates);
        }
        self
    }

    /// The rates configured for one scope.
    pub fn rates(&self, scope: Scope) -> &[Rate] {
        self.scopes.get(&scope).map_or(&[], Vec::as_slice)
    }

    /// True when no scope has any rate configured.
    pub fn is_unlimited(&self) -> bool {
        self.scopes.is_empty()
    }
}

/// One rate's worth of history.
///
/// A sliding log rather than a token bucket, so the bursts the server permits are not
/// refused locally.
#[derive(Debug)]
struct Window {
    rate: Rate,
    hits: VecDeque<Instant>,
}

impl Window {
    fn new(rate: Rate) -> Self {
        Self {
            rate,
            hits: VecDeque::with_capacity(rate.limit.min(64) as usize),
        }
    }

    /// Drops hits that have aged out, then reports when the next slot frees up.
    ///
    /// `None` means a slot is free now.
    fn wait_until(&mut self, now: Instant) -> Option<Instant> {
        while self
            .hits
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= self.rate.period)
        {
            self.hits.pop_front();
        }

        if (self.hits.len() as u32) < self.rate.limit {
            None
        } else {
            // The window frees a slot once its oldest hit ages out. `limit` is non-zero,
            // so a full window always has a front element, and `Rate::new` bounds the
            // period so the addition cannot overflow the clock.
            self.hits
                .front()
                .and_then(|t| t.checked_add(self.rate.period))
        }
    }

    fn record(&mut self, now: Instant) {
        self.hits.push_back(now);
    }
}

/// Per-scope state: one window per configured rate, plus any server-imposed backoff.
#[derive(Debug)]
struct ScopeState {
    windows: Vec<Window>,
    /// Set from a `429`'s `Retry-After` or from an exhausted `X-RateLimit-Remaining`, so
    /// concurrent tasks back off together rather than each discovering the throttle for
    /// itself.
    penalty_until: Option<Instant>,
}

impl ScopeState {
    fn new(rates: &[Rate]) -> Self {
        Self {
            windows: rates.iter().copied().map(Window::new).collect(),
            penalty_until: None,
        }
    }

    fn wait_until(&mut self, now: Instant) -> Option<Instant> {
        let penalty = self.penalty_until.filter(|t| *t > now);
        self.windows
            .iter_mut()
            .filter_map(|w| w.wait_until(now))
            .chain(penalty)
            .max()
    }

    fn record(&mut self, now: Instant) {
        for window in &mut self.windows {
            window.record(now);
        }
    }

    /// Extends the backoff, never shortens it.
    ///
    /// The server's headers describe a window this client only partly knows about, and
    /// they can arrive out of order when requests overlap. Taking the maximum means a
    /// response that was already in flight cannot retract a penalty a later one imposed.
    fn penalize_until(&mut self, until: Instant) {
        self.penalty_until = Some(match self.penalty_until {
            Some(existing) if existing > until => existing,
            _ => until,
        });
    }
}

/// Enforces [`RateLimits`] across all requests made through one client.
#[derive(Debug)]
pub(crate) struct Limiter {
    limits: RateLimits,
    max_wait: Duration,
    state: Mutex<HashMap<Scope, ScopeState>>,
}

impl Limiter {
    pub(crate) fn new(limits: RateLimits, max_wait: Duration) -> Self {
        Self {
            limits,
            max_wait,
            state: Mutex::new(HashMap::new()),
        }
    }

    #[cfg(test)]
    pub(crate) fn limits(&self) -> &RateLimits {
        &self.limits
    }

    /// Waits until `scope` has a free slot, then claims one.
    ///
    /// Waiters re-check after sleeping rather than being handed a reservation, so
    /// wake-ups are not ordered and a request may be overtaken under contention.
    /// Correctness depends on the re-check, not on the order.
    pub(crate) async fn acquire(&self, scope: Scope) -> Result<()> {
        let rates = self.limits.rates(scope);

        // A budget for the whole call, not per sleep. Checking each sleep in isolation
        // would let a penalty that keeps being refreshed park a task indefinitely, which
        // is exactly what `max_rate_limit_wait` promises not to do.
        let deadline = Instant::now().checked_add(self.max_wait);

        loop {
            let wait = {
                let now = Instant::now();
                let mut state = self.lock();

                // Nothing to pace against and nothing the server has told us to wait for.
                // Checked before the entry is created so an opted-out scope stays free.
                if rates.is_empty() && !state.contains_key(&scope) {
                    return Ok(());
                }

                let entry = state.entry(scope).or_insert_with(|| ScopeState::new(rates));

                match entry.wait_until(now) {
                    None => {
                        entry.record(now);
                        return Ok(());
                    }
                    Some(until) => until.saturating_duration_since(now),
                }
            };

            // Over budget either as a single sleep or cumulatively across this call. A
            // `None` deadline means `max_wait` is large enough to overflow the clock, so
            // there is effectively no total budget to exceed.
            let over_total = deadline
                .zip(Instant::now().checked_add(wait))
                .is_some_and(|(deadline, finish)| finish > deadline);
            if wait > self.max_wait || over_total {
                return Err(Error::RateLimitWouldBlock {
                    scope,
                    wait,
                    max_wait: self.max_wait,
                });
            }

            tracing::debug!(
                scope = %scope,
                wait_ms = wait.as_millis(),
                "local rate limit reached, waiting"
            );
            tokio::time::sleep(wait).await;
        }
    }

    /// Folds a response's `X-RateLimit-*` headers into the scope's backoff.
    ///
    /// The local windows only count what this client sent. When the account is shared —
    /// with the panel, with another process — the server's remaining count is the only
    /// evidence that the allowance is already spent, and ignoring it is what turns a
    /// paced client back into one that collects `429`s.
    ///
    /// Only an exhausted allowance imposes a wait. A non-zero remaining count is left
    /// alone rather than used to relax the local windows, so the headers can only ever
    /// make this client more patient.
    pub(crate) fn observe(&self, scope: Scope, headers: &HeaderMap) {
        let Some(0) = header_u64(headers, "x-ratelimit-remaining") else {
            return;
        };
        let Some(reset) = header_u64(headers, "x-ratelimit-reset") else {
            return;
        };
        let Some(until) = unix_timestamp_to_instant(reset) else {
            return;
        };
        tracing::debug!(
            scope = %scope,
            reset,
            "server reports the allowance is spent, backing off until it resets"
        );
        self.penalize(scope, until);
    }

    /// Records a `429`, backing the scope off for `retry_after`.
    pub(crate) fn record_throttled(&self, scope: Scope, retry_after: Duration) {
        if let Some(until) = Instant::now().checked_add(retry_after) {
            self.penalize(scope, until);
        }
    }

    fn penalize(&self, scope: Scope, until: Instant) {
        let rates = self.limits.rates(scope);
        self.lock()
            .entry(scope)
            .or_insert_with(|| ScopeState::new(rates))
            .penalize_until(until);
    }

    /// The state map, recovering from a poisoned lock.
    ///
    /// A panic while holding it can only have left a window mid-update; the worst
    /// outcome is pacing against a slightly wrong history, which is not worth
    /// propagating the panic for.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Scope, ScopeState>> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Reads a header whose value is a bare integer.
fn header_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

/// Converts a Unix timestamp into a point on the (possibly paused) monotonic clock.
///
/// Goes through the wall clock because that is the only shared reference the server and
/// this process have. A timestamp already in the past yields the current instant, and one
/// far enough ahead to overflow the clock yields `None`.
fn unix_timestamp_to_instant(timestamp: u64) -> Option<Instant> {
    let now_unix = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    let target = Duration::from_secs(timestamp);
    Instant::now().checked_add(target.saturating_sub(now_unix))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn rate(spec: &str) -> Rate {
        spec.parse().expect("test rate literal is well-formed")
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            let name: reqwest::header::HeaderName =
                name.parse().expect("test header name is well-formed");
            headers.insert(
                name,
                value.parse().expect("test header value is well-formed"),
            );
        }
        headers
    }

    #[test]
    fn the_built_in_rate_literals_parse() {
        let limits = RateLimits::mxroute_defaults();
        assert_eq!(limits.rates(Scope::Read), [rate("100/min")]);
        assert_eq!(limits.rates(Scope::Write), [rate("20/min")]);
        assert!(!limits.is_unlimited());
    }

    #[test]
    fn rates_round_trip_through_their_own_notation() {
        for spec in ["100/min", "20/min", "10/s", "2/2min", "1000/h", "300/day"] {
            assert_eq!(rate(spec).to_string(), spec);
        }
    }

    #[test]
    fn a_rate_that_could_never_admit_a_request_is_rejected() {
        assert!(Rate::new(0, Duration::from_secs(60)).is_err());
        assert!(Rate::new(1, Duration::ZERO).is_err());
        assert!(Rate::new(1, MAX_PERIOD + Duration::from_secs(1)).is_err());
        assert!("".parse::<Rate>().is_err());
        assert!("100".parse::<Rate>().is_err());
        assert!("100/fortnight".parse::<Rate>().is_err());
        assert!("100/".parse::<Rate>().is_err());
    }

    #[test]
    fn writes_and_reads_are_scoped_by_method() {
        assert_eq!(Scope::of(&Method::GET), Scope::Read);
        assert_eq!(Scope::of(&Method::HEAD), Scope::Read);
        assert_eq!(Scope::of(&Method::POST), Scope::Write);
        assert_eq!(Scope::of(&Method::PATCH), Scope::Write);
        assert_eq!(Scope::of(&Method::DELETE), Scope::Write);
    }

    #[test]
    fn removing_every_rate_for_a_scope_lifts_the_limit() {
        let limits = RateLimits::mxroute_defaults().with_scope(Scope::Write, []);
        assert!(limits.rates(Scope::Write).is_empty());
        assert_eq!(limits.rates(Scope::Read), [rate("100/min")]);
    }

    #[tokio::test(start_paused = true)]
    async fn an_unlimited_scope_never_waits() {
        let limiter = Limiter::new(RateLimits::unlimited(), Duration::from_secs(60));
        let start = Instant::now();
        for _ in 0..1000 {
            limiter
                .acquire(Scope::Write)
                .await
                .expect("an unlimited scope admits every request");
        }
        assert_eq!(Instant::now(), start);
    }

    #[tokio::test(start_paused = true)]
    async fn a_full_window_waits_for_its_oldest_hit_to_age_out() {
        let limits = RateLimits::unlimited().with_scope(Scope::Write, [rate("2/min")]);
        let limiter = Limiter::new(limits, Duration::from_secs(300));
        let start = Instant::now();

        for _ in 0..2 {
            limiter.acquire(Scope::Write).await.expect("within limit");
        }
        assert_eq!(Instant::now(), start, "the first two fit in the window");

        limiter
            .acquire(Scope::Write)
            .await
            .expect("waits, not errs");
        assert_eq!(
            Instant::now().duration_since(start),
            Duration::from_secs(60),
            "the third waits out the first hit"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_wait_over_the_budget_fails_instead_of_sleeping() {
        let limits = RateLimits::unlimited().with_scope(Scope::Read, [rate("1/h")]);
        let limiter = Limiter::new(limits, Duration::from_secs(5));
        limiter.acquire(Scope::Read).await.expect("the first fits");

        let err = limiter
            .acquire(Scope::Read)
            .await
            .expect_err("an hour is over the five-second budget");
        assert!(err.is_rate_limited());
        assert!(matches!(err, Error::RateLimitWouldBlock { scope, .. } if scope == Scope::Read));
    }

    #[tokio::test(start_paused = true)]
    async fn scopes_do_not_share_a_bucket() {
        let limits = RateLimits::unlimited()
            .with_scope(Scope::Write, [rate("1/min")])
            .with_scope(Scope::Read, [rate("1/min")]);
        let limiter = Limiter::new(limits, Duration::from_secs(300));
        let start = Instant::now();

        limiter.acquire(Scope::Write).await.expect("first write");
        limiter.acquire(Scope::Read).await.expect("first read");
        assert_eq!(Instant::now(), start, "a read is not spent by a write");
    }

    #[tokio::test(start_paused = true)]
    async fn an_exhausted_allowance_reported_by_the_server_imposes_a_wait() {
        let limiter = Limiter::new(RateLimits::unlimited(), Duration::from_secs(300));
        let start = Instant::now();

        let reset = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_secs()
            + 30;
        limiter.observe(
            Scope::Read,
            &headers(&[
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", &reset.to_string()),
            ]),
        );

        // The scope has no local rate, so only the server's penalty can hold it back.
        limiter.acquire(Scope::Read).await.expect("waits it out");
        let waited = Instant::now().duration_since(start);
        assert!(
            waited >= Duration::from_secs(29) && waited <= Duration::from_secs(31),
            "waited {waited:?}, expected about 30s"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_allowance_with_requests_left_imposes_nothing() {
        let limiter = Limiter::new(RateLimits::unlimited(), Duration::from_secs(300));
        let start = Instant::now();
        limiter.observe(
            Scope::Read,
            &headers(&[
                ("x-ratelimit-remaining", "42"),
                ("x-ratelimit-reset", "99999999999"),
            ]),
        );
        limiter.acquire(Scope::Read).await.expect("no penalty");
        assert_eq!(Instant::now(), start);
    }

    #[tokio::test(start_paused = true)]
    async fn headers_can_only_lengthen_a_wait_never_shorten_one() {
        let limiter = Limiter::new(RateLimits::unlimited(), Duration::from_secs(600));
        let start = Instant::now();

        limiter.record_throttled(Scope::Write, Duration::from_secs(120));
        // A response that was already in flight when the 429 landed, reporting a reset
        // that has almost arrived. It must not retract the longer penalty.
        let soon = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_secs()
            + 1;
        limiter.observe(
            Scope::Write,
            &headers(&[
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", &soon.to_string()),
            ]),
        );

        limiter.acquire(Scope::Write).await.expect("waits it out");
        assert!(Instant::now().duration_since(start) >= Duration::from_secs(119));
    }

    #[tokio::test(start_paused = true)]
    async fn malformed_rate_limit_headers_are_ignored() {
        let limiter = Limiter::new(RateLimits::unlimited(), Duration::from_secs(300));
        let start = Instant::now();
        for pairs in [
            vec![("x-ratelimit-remaining", "0")],
            vec![
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", "soon"),
            ],
            vec![
                ("x-ratelimit-remaining", "nope"),
                ("x-ratelimit-reset", "1"),
            ],
            vec![],
        ] {
            limiter.observe(Scope::Read, &headers(&pairs));
        }
        limiter.acquire(Scope::Read).await.expect("no penalty");
        assert_eq!(Instant::now(), start);
    }

    #[tokio::test(start_paused = true)]
    async fn a_reset_already_in_the_past_does_not_wait() {
        let limiter = Limiter::new(RateLimits::unlimited(), Duration::from_secs(300));
        let start = Instant::now();
        limiter.observe(
            Scope::Read,
            &headers(&[
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", "1"), // 1970
            ]),
        );
        limiter.acquire(Scope::Read).await.expect("nothing to wait");
        assert_eq!(Instant::now(), start);
    }
}
