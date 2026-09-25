//! Shared HTTP client setup for every `Nse*` struct: a consistent user
//! agent and timeouts, plus automatic retry on transient failures.
//!
//! NSE's endpoints have shown one-off flakiness independent of any code
//! change (the exact same request that just worked via `curl` failing
//! once via this crate moments later, then succeeding again on retry
//! with no change in between - see the bhavcopy section of
//! docs/nse-findings.md). Retrying automatically absorbs that instead of
//! surfacing it as a hard error on the first bad response.

use std::time::Duration;

use reqwest::{Client, ClientBuilder};
use reqwest_middleware::ClientBuilder as MiddlewareClientBuilder;
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};

use super::USER_AGENT;

/// The HTTP client type every `Nse*` struct uses in place of a bare
/// `reqwest::Client`.
pub(super) type HttpClient = reqwest_middleware::ClientWithMiddleware;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RETRIES: u32 = 3;

/// A `reqwest::ClientBuilder` pre-configured with this crate's user agent
/// and timeouts - the starting point for every `Nse*::new()`. Callers that
/// need extra options (e.g. `NseHistory`'s `.cookie_store(true)`) chain
/// them on top, then pass the built `Client` to `with_retry`.
pub(super) fn client_builder() -> ClientBuilder {
    Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
}

/// Wraps a built `reqwest::Client` with automatic retry: up to
/// `MAX_RETRIES` attempts with exponential backoff and jitter, only for
/// transient failures - connection errors, timeouts, HTTP 408/429/5xx.
/// A 403 (`Error::Blocked`, NSE's bot protection) is never retried - the
/// session is flagged, not the request, so retrying it immediately would
/// just waste time.
pub(super) fn with_retry(client: Client) -> HttpClient {
    let retry_policy = ExponentialBackoff::builder().build_with_max_retries(MAX_RETRIES);
    MiddlewareClientBuilder::new(client)
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .build()
}
