//! Retry policy, backoff, and `Retry-After` parsing.

use std::collections::BTreeSet;
use std::hash::{BuildHasher, Hasher, RandomState};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::StatusCode;
use reqwest::header::HeaderMap;

/// Default timeout for each attempt.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// When and how failed requests are retried.
///
/// Override fields with struct update syntax:
///
/// ```
/// use typesafe::RetryPolicy;
///
/// let policy = RetryPolicy { max_retries: 5, ..RetryPolicy::default() };
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct RetryPolicy {
    /// Maximum retries after the initial attempt; `0` disables retries. Default: 2.
    pub max_retries: u32,
    /// First backoff delay, doubled each retry up to `backoff_max`. Default: 500ms.
    pub backoff_initial: Duration,
    /// Maximum backoff delay. Default: 5s.
    pub backoff_max: Duration,
    /// Fraction of each backoff delay randomly subtracted, from 0 to 1. Default: 0.25.
    pub backoff_jitter: f64,
    /// HTTP statuses to retry. Default: 408, 429, and 500–599.
    pub http_statuses: BTreeSet<StatusCode>,
    /// Honor `retry-after-ms` and `Retry-After` up to `max_retry_after`. Default: true.
    pub respect_retry_after: bool,
    /// Longest server-requested delay to honor; longer ones fall back to backoff. Default: 60s.
    pub max_retry_after: Duration,
    /// Retry connection failures, including interrupted response bodies. Default: true.
    pub retry_connection_errors: bool,
    /// Retry attempts that time out. Default: true.
    pub retry_timeouts: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        let statuses = [408, 429].into_iter().chain(500..600);
        Self {
            max_retries: 2,
            backoff_initial: Duration::from_millis(500),
            backoff_max: Duration::from_secs(5),
            backoff_jitter: 0.25,
            http_statuses: statuses.filter_map(|s| StatusCode::from_u16(s).ok()).collect(),
            respect_retry_after: true,
            max_retry_after: Duration::from_secs(60),
            retry_connection_errors: true,
            retry_timeouts: true,
        }
    }
}

impl RetryPolicy {
    /// The default policy with retries disabled.
    pub fn none() -> Self {
        Self { max_retries: 0, ..Self::default() }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        let jitter = self.backoff_jitter;
        if !(0.0..=1.0).contains(&jitter) {
            return Err(format!("`backoff_jitter` must be between 0 and 1, got {jitter}."));
        }
        Ok(())
    }

    /// The delay before retry number `attempt + 1`, given the failed response's headers.
    ///
    /// An allowed server delay wins; otherwise capped exponential backoff with jitter.
    /// `random` is uniform in `[0, 1)`.
    pub(crate) fn delay(&self, attempt: u32, headers: Option<&HeaderMap>, random: f64) -> Duration {
        if self.respect_retry_after
            && let Some(after) = headers.and_then(|h| parse_retry_after(h, SystemTime::now()))
            && after <= self.max_retry_after
        {
            return after;
        }
        let factor = 1u32.checked_shl(attempt).unwrap_or(u32::MAX);
        let exponential = self.backoff_initial.saturating_mul(factor).min(self.backoff_max);
        let ms = exponential.as_millis() as f64 * (1.0 - random * self.backoff_jitter);
        Duration::from_millis(ms.round() as u64)
    }
}

/// A uniform value in `[0, 1)` for jitter, seeded per call by std's hasher keys.
pub(crate) fn random_unit() -> f64 {
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos());
    (hasher.finish() >> 11) as f64 / (1u64 << 53) as f64
}

/// Parse `retry-after-ms` or `Retry-After` (seconds or an HTTP date), preferring `retry-after-ms`.
pub(crate) fn parse_retry_after(headers: &HeaderMap, now: SystemTime) -> Option<Duration> {
    let header = |name| headers.get(name).and_then(|v| v.to_str().ok()).map(str::trim);
    if let Some(ms) = header("retry-after-ms").and_then(|v| v.parse::<f64>().ok())
        && ms >= 0.0
    {
        return Duration::try_from_secs_f64(ms / 1000.0).ok();
    }
    let raw = header("retry-after")?;
    if let Ok(seconds) = raw.parse::<f64>() {
        return if seconds >= 0.0 { Duration::try_from_secs_f64(seconds).ok() } else { None };
    }
    let date = parse_http_date(raw)?;
    Some(date.duration_since(now).unwrap_or(Duration::ZERO))
}

/// Parse an IMF-fixdate such as `Sun, 06 Nov 1994 08:49:37 GMT` (RFC 9110 §5.6.7).
fn parse_http_date(s: &str) -> Option<SystemTime> {
    let rest = s.split_once(", ")?.1;
    let mut parts = rest.split(' ');
    let (day, month, year, time, zone) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    if zone != "GMT" || parts.next().is_some() {
        return None;
    }
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let month = MONTHS.iter().position(|m| *m == month)? as i64 + 1;
    let day: i64 = day.parse().ok().filter(|d| (1..=31).contains(d))?;
    let year: i64 = year.parse().ok()?;
    let mut hms = time.split(':').map(|p| p.parse::<i64>().ok());
    let (h, m, sec) = (hms.next()??, hms.next()??, hms.next()??);
    if hms.next().is_some() || h > 23 || m > 59 || sec > 60 {
        return None;
    }
    let secs = days_from_civil(year, month, day) * 86_400 + h * 3600 + m * 60 + sec;
    Some(UNIX_EPOCH + Duration::from_secs(u64::try_from(secs).ok()?))
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use reqwest::header::HeaderValue;

    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        pairs.iter().map(|(k, v)| (k.parse().unwrap(), HeaderValue::from_str(v).unwrap())).collect()
    }

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[test]
    fn default_policy_matches_the_js_sdk() {
        let p = RetryPolicy::default();
        assert_eq!(p.max_retries, 2);
        assert_eq!(p.http_statuses.len(), 102);
        assert!(p.http_statuses.contains(&StatusCode::REQUEST_TIMEOUT));
        assert!(!p.http_statuses.contains(&StatusCode::BAD_REQUEST));
    }

    #[test]
    fn backs_off_exponentially_with_a_cap_and_jitter() {
        let p = RetryPolicy::default();
        assert_eq!(p.delay(0, None, 0.0), MS(500));
        assert_eq!(p.delay(1, None, 0.0), MS(1000));
        assert_eq!(p.delay(4, None, 0.0), MS(5000));
        assert_eq!(p.delay(40, None, 0.0), MS(5000));
        assert_eq!(p.delay(0, None, 0.999_999), MS(375));
    }

    #[test]
    fn honors_retry_after_within_the_cap() {
        let p = RetryPolicy::default();
        assert_eq!(p.delay(0, Some(&headers(&[("retry-after-ms", "1234")])), 0.0), MS(1234));
        assert_eq!(p.delay(0, Some(&headers(&[("retry-after", "2")])), 0.0), MS(2000));
        assert_eq!(p.delay(0, Some(&headers(&[("retry-after", "120")])), 0.0), MS(500));
        let ignore = RetryPolicy { respect_retry_after: false, ..p };
        assert_eq!(ignore.delay(0, Some(&headers(&[("retry-after", "2")])), 0.0), MS(500));
    }

    #[test]
    fn parses_retry_after_forms() {
        let now = UNIX_EPOCH + Duration::from_secs(784_111_770);
        let parse = |pairs: &[(&'static str, &str)]| parse_retry_after(&headers(pairs), now);
        assert_eq!(parse(&[("retry-after-ms", "1.5"), ("retry-after", "9")]), Some(Duration::from_micros(1500)));
        assert_eq!(parse(&[("retry-after-ms", "-1"), ("retry-after", "3")]), Some(MS(3000)));
        assert_eq!(parse(&[("retry-after", "Sun, 06 Nov 1994 08:49:37 GMT")]), Some(MS(7000)));
        assert_eq!(parse(&[("retry-after", "Sun, 06 Nov 1994 08:49:00 GMT")]), Some(Duration::ZERO));
        assert_eq!(parse(&[("retry-after", "-3")]), None);
        assert_eq!(parse(&[("retry-after", "soon")]), None);
        assert_eq!(parse(&[]), None);
    }

    #[test]
    fn parses_http_dates() {
        let at = |s| parse_http_date(s).map(|t| t.duration_since(UNIX_EPOCH).unwrap().as_secs());
        assert_eq!(at("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(at("Sun, 06 Nov 1994 08:49:37 GMT"), Some(784_111_777));
        assert_eq!(at("Tue, 29 Feb 2028 12:00:00 GMT"), Some(1_835_438_400));
        assert_eq!(at("Sun, 06 Nov 1994 08:49:37 UTC"), None);
        assert_eq!(at("Sunday, 06-Nov-94 08:49:37 GMT"), None);
    }

    #[test]
    fn random_unit_is_in_range() {
        for _ in 0..1000 {
            assert!((0.0..1.0).contains(&random_unit()));
        }
    }

    #[test]
    fn rejects_out_of_range_jitter() {
        assert!(RetryPolicy { backoff_jitter: 1.5, ..RetryPolicy::default() }.validate().is_err());
        assert!(RetryPolicy { backoff_jitter: f64::NAN, ..RetryPolicy::default() }.validate().is_err());
    }
}
