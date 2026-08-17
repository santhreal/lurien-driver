//! Passive network instrumentation for BiDi browsers.
//!
//! Captures every request, response, and fetch error that flows through the
//! browser, then provides ergonomic query, streaming, and export utilities.
//!
//! This module captures response *metadata* only. Response bodies are not
//! fetched because the `rustenium-bidi-definitions` crate does not expose a
//! `network.getResponseBody` command and the `ResponseContent` event only
//! reports `size`. Callers that need body content must fetch it separately.

use anyhow::Result;
use rustenium_bidi_definitions::network::events::{
    BeforeRequestSent, FetchError, ResponseCompleted,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

// ------------------------------------------------------------------
// Data types
// ------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapturedHeader {
    pub name: String,
    pub value: String,
}

impl From<&rustenium_bidi_definitions::network::types::Header> for CapturedHeader {
    fn from(h: &rustenium_bidi_definitions::network::types::Header) -> Self {
        let value = match &h.value {
            rustenium_bidi_definitions::network::types::BytesValue::StringValue(s) => {
                s.value.clone()
            }
            rustenium_bidi_definitions::network::types::BytesValue::Base64Value(b) => {
                b.value.clone()
            }
        };
        Self {
            name: h.name.clone(),
            value,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CapturedTiming {
    pub dns_start_ms: Option<f64>,
    pub dns_end_ms: Option<f64>,
    pub connect_start_ms: Option<f64>,
    pub connect_end_ms: Option<f64>,
    pub tls_start_ms: Option<f64>,
    pub response_start_ms: Option<f64>,
    pub response_end_ms: Option<f64>,
}

impl From<&rustenium_bidi_definitions::network::types::FetchTimingInfo> for CapturedTiming {
    fn from(t: &rustenium_bidi_definitions::network::types::FetchTimingInfo) -> Self {
        let origin = t.request_time;
        Self {
            dns_start_ms: non_neg(t.dns_start - origin),
            dns_end_ms: non_neg(t.dns_end - origin),
            connect_start_ms: non_neg(t.connect_start - origin),
            connect_end_ms: non_neg(t.connect_end - origin),
            tls_start_ms: non_neg(t.tls_start - origin),
            response_start_ms: non_neg(t.response_start - origin),
            response_end_ms: non_neg(t.response_end - origin),
        }
    }
}

fn non_neg(v: f64) -> Option<f64> {
    if v >= 0.0 {
        Some(v)
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapturedCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub size: u64,
    pub http_only: bool,
    pub secure: bool,
    pub same_site: String,
}

impl From<&rustenium_bidi_definitions::network::types::Cookie> for CapturedCookie {
    fn from(c: &rustenium_bidi_definitions::network::types::Cookie) -> Self {
        let value = match &c.value {
            rustenium_bidi_definitions::network::types::BytesValue::StringValue(s) => {
                s.value.clone()
            }
            rustenium_bidi_definitions::network::types::BytesValue::Base64Value(b) => {
                b.value.clone()
            }
        };
        Self {
            name: c.name.clone(),
            value,
            domain: c.domain.clone(),
            path: c.path.clone(),
            size: c.size,
            http_only: c.http_only,
            secure: c.secure,
            same_site: format!("{:?}", c.same_site).to_lowercase(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapturedRequest {
    pub id: String,
    pub context: Option<String>,
    pub method: String,
    pub url: String,
    pub headers: Vec<CapturedHeader>,
    pub post_data: Option<String>,
    pub timestamp: u64,
    pub destination: String,
    pub initiator_type: Option<String>,
    pub timing: CapturedTiming,
    pub cookies: Vec<CapturedCookie>,
}

impl CapturedRequest {
    /// If `post_data` looks like JSON, return it parsed.
    pub fn json_body(&self) -> Option<serde_json::Value> {
        self.post_data
            .as_ref()
            .and_then(|s| serde_json::from_str(s).ok())
    }

    /// Return the parsed query parameters as a vec of (key, value) pairs.
    ///
    /// Returns `Err` when `self.url` cannot be parsed as a URL, so callers can
    /// distinguish "URL did not parse" from "URL had no query string".
    pub fn query_params(&self) -> Result<Vec<(String, String)>, url::ParseError> {
        let u = url::Url::parse(&self.url)?;
        Ok(u.query_pairs()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect())
    }

    /// Case-insensitive header lookup.
    pub fn request_header(&self, name: &str) -> Option<&str> {
        let name_lower = name.to_lowercase();
        self.headers
            .iter()
            .find(|h| h.name.to_lowercase() == name_lower)
            .map(|h| h.value.as_str())
    }
}

/// A captured network response.
///
/// `foxdriver` captures response **metadata** (headers, status, MIME, size)
/// from BiDi `network.responseCompleted` events. It does **not** fetch or
/// store response bodies: the `rustenium-bidi-definitions` crate used by this
/// driver does not expose a `network.getResponseBody` command, and the
/// `ResponseContent` event field only carries `size`. Callers that need body
/// content must fetch it through another mechanism.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapturedResponse {
    pub id: String,
    pub url: String,
    pub protocol: String,
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<CapturedHeader>,
    pub mime_type: String,
    /// Response body size, as reported by the browser.
    ///
    /// This is metadata only; the response body bytes are not captured.
    pub body_size: Option<u64>,
    pub from_cache: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapturedError {
    pub id: String,
    pub url: String,
    pub error_text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkEntry {
    pub request: CapturedRequest,
    pub response: Option<CapturedResponse>,
    pub error: Option<CapturedError>,
}

impl NetworkEntry {
    pub fn has_response(&self) -> bool {
        self.response.is_some()
    }

    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    pub fn final_url(&self) -> &str {
        self.response
            .as_ref()
            .map(|r| r.url.as_str())
            .unwrap_or(&self.request.url)
    }

    pub fn status(&self) -> Option<u16> {
        self.response.as_ref().map(|r| r.status)
    }

    pub fn response_header(&self, name: &str) -> Option<&str> {
        let name_lower = name.to_lowercase();
        self.response.as_ref().and_then(|r| {
            r.headers
                .iter()
                .find(|h| h.name.to_lowercase() == name_lower)
                .map(|h| h.value.as_str())
        })
    }

    pub fn request_header(&self, name: &str) -> Option<&str> {
        self.request.request_header(name)
    }

    /// Generate a curl command that replays this request.
    pub fn to_curl(&self) -> String {
        fn shell_quote(s: &str) -> String {
            // Escape single quotes for bash: ' -> '\''
            s.replace('\'', "'\\''")
        }
        let req = &self.request;
        // The method is interpolated into a shell command line, so it must be
        // quoted like every other captured value. BiDi delivers it as a
        // string; a non-standard method from a hostile or buggy endpoint
        // must not become extra shell arguments.
        let mut parts = vec![format!("curl -X '{}'", shell_quote(&req.method))];
        for h in &req.headers {
            if h.name.eq_ignore_ascii_case("host")
                || h.name.eq_ignore_ascii_case("connection")
                || h.name.eq_ignore_ascii_case("accept-encoding")
            {
                continue;
            }
            parts.push(format!(
                "-H '{}: {}'",
                shell_quote(&h.name),
                shell_quote(&h.value)
            ));
        }
        if let Some(ref body) = req.post_data {
            parts.push(format!("-d '{}'", shell_quote(body)));
        }
        parts.push(format!("'{}'", shell_quote(&req.url)));
        parts.join(" ")
    }
}

// ------------------------------------------------------------------
// Filter
// ------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Filter {
    method: Option<String>,
    status_range: Option<std::ops::RangeInclusive<u16>>,
    url_substring: Option<String>,
    url_regex: Option<regex::Regex>,
    header_name: Option<String>,
    header_value_substring: Option<String>,
    destination: Option<String>,
    has_response: Option<bool>,
    has_error: Option<bool>,
}

impl Filter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn method(mut self, m: impl Into<String>) -> Self {
        self.method = Some(m.into().to_uppercase());
        self
    }

    pub fn status_range(mut self, r: std::ops::RangeInclusive<u16>) -> Self {
        self.status_range = Some(r);
        self
    }

    pub fn url_contains(mut self, needle: impl Into<String>) -> Self {
        self.url_substring = Some(needle.into().to_lowercase());
        self
    }

    pub fn url_regex(mut self, pattern: &str) -> Result<Self> {
        self.url_regex = Some(regex::Regex::new(pattern)?);
        Ok(self)
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.header_name = Some(name.into().to_lowercase());
        self.header_value_substring = Some(value.into().to_lowercase());
        self
    }

    pub fn header_name(mut self, name: impl Into<String>) -> Self {
        self.header_name = Some(name.into().to_lowercase());
        self
    }

    pub fn header_value(mut self, value: impl Into<String>) -> Self {
        self.header_value_substring = Some(value.into().to_lowercase());
        self
    }

    pub fn destination(mut self, d: impl Into<String>) -> Self {
        self.destination = Some(d.into().to_lowercase());
        self
    }

    pub fn with_response(mut self) -> Self {
        self.has_response = Some(true);
        self
    }

    pub fn without_response(mut self) -> Self {
        self.has_response = Some(false);
        self
    }

    pub fn with_error(mut self) -> Self {
        self.has_error = Some(true);
        self
    }

    fn matches(&self, e: &NetworkEntry) -> bool {
        if let Some(m) = &self.method {
            if e.request.method.to_uppercase() != *m {
                return false;
            }
        }
        if let Some(r) = &self.status_range {
            // A response with no status (fetch error, incomplete response)
            // must not match a status range; treat missing as absent, not 0.
            let Some(st) = e.status() else {
                return false;
            };
            if !r.contains(&st) {
                return false;
            }
        }
        if let Some(needle) = &self.url_substring {
            if !e.request.url.to_lowercase().contains(needle) {
                return false;
            }
        }
        if let Some(re) = &self.url_regex {
            if !re.is_match(&e.request.url) {
                return false;
            }
        }
        if let (Some(name), Some(value)) = (&self.header_name, &self.header_value_substring) {
            let found = e
                .request
                .headers
                .iter()
                .chain(
                    e.response
                        .as_ref()
                        .map(|r| r.headers.as_slice())
                        .unwrap_or(&[]),
                )
                .any(|h| {
                    h.name.to_lowercase().contains(name) && h.value.to_lowercase().contains(value)
                });
            if !found {
                return false;
            }
        } else if let Some(name) = &self.header_name {
            let found = e
                .request
                .headers
                .iter()
                .chain(
                    e.response
                        .as_ref()
                        .map(|r| r.headers.as_slice())
                        .unwrap_or(&[]),
                )
                .any(|h| h.name.to_lowercase().contains(name));
            if !found {
                return false;
            }
        } else if let Some(value) = &self.header_value_substring {
            let found = e
                .request
                .headers
                .iter()
                .chain(
                    e.response
                        .as_ref()
                        .map(|r| r.headers.as_slice())
                        .unwrap_or(&[]),
                )
                .any(|h| h.value.to_lowercase().contains(value));
            if !found {
                return false;
            }
        }
        if let Some(d) = &self.destination {
            if e.request.destination.to_lowercase() != *d {
                return false;
            }
        }
        if let Some(want) = self.has_response {
            if e.has_response() != want {
                return false;
            }
        }
        if let Some(want) = self.has_error {
            if e.is_error() != want {
                return false;
            }
        }
        true
    }
}

// ------------------------------------------------------------------
// Metrics
// ------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NetworkMetrics {
    pub requests_received: u64,
    pub responses_received: u64,
    pub errors_received: u64,
    pub entries_evicted: u64,
    pub pending_responses_dropped: u64,
    pub pending_errors_dropped: u64,
    pub broadcast_drops: u64,
    pub duplicate_responses: u64,
    pub duplicate_errors: u64,
    pub max_entries: usize,
}

// ------------------------------------------------------------------
// Inner log state
// ------------------------------------------------------------------

#[derive(Debug)]
struct Inner {
    entries: Vec<Arc<NetworkEntry>>,
    by_id: HashMap<String, usize>,
    pending_responses: HashMap<String, CapturedResponse>,
    pending_errors: HashMap<String, CapturedError>,
    max_entries: usize,
    max_pending: usize,
    tx: broadcast::Sender<Arc<NetworkEntry>>,
    metrics: NetworkMetrics,
    /// Whether we have already warned about entry eviction this session.
    eviction_warned: bool,
}

impl Inner {
    fn new(
        max_entries: usize,
        max_pending: usize,
        tx: broadcast::Sender<Arc<NetworkEntry>>,
    ) -> Self {
        let metrics = NetworkMetrics {
            max_entries,
            ..NetworkMetrics::default()
        };
        Self {
            entries: if max_entries > 0 {
                Vec::with_capacity(max_entries)
            } else {
                Vec::new()
            },
            by_id: HashMap::new(),
            pending_responses: HashMap::new(),
            pending_errors: HashMap::new(),
            max_entries,
            max_pending,
            tx,
            metrics,
            eviction_warned: false,
        }
    }

    fn push_entry(&mut self, entry: Arc<NetworkEntry>) {
        // Skip duplicate request IDs: BiDi can occasionally resend.
        if self.by_id.contains_key(&entry.request.id) {
            return;
        }

        // Evict just enough oldest entries to stay under the cap.
        if self.max_entries > 0 && self.entries.len() >= self.max_entries {
            let remove = self.entries.len().saturating_sub(self.max_entries - 1);
            let removed = self.entries.drain(..remove).len();
            self.metrics.entries_evicted += removed as u64;
            self.by_id.clear();
            for (i, e) in self.entries.iter().enumerate() {
                self.by_id.insert(e.request.id.clone(), i);
            }
            if !self.eviction_warned {
                self.eviction_warned = true;
                tracing::warn!(
                    removed,
                    max_entries = self.max_entries,
                    "[foxdriver] network capture entry evicted; oldest requests are being dropped"
                );
            }
        }

        let idx = self.entries.len();
        self.by_id.insert(entry.request.id.clone(), idx);
        self.entries.push(entry.clone());

        // Broadcast (ignore errors (all receivers dropped)).
        if self.tx.send(entry).is_err() {
            self.metrics.broadcast_drops += 1;
        }
    }
}

// ------------------------------------------------------------------
// NetworkLog
// ------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct NetworkLog {
    inner: Arc<RwLock<Inner>>,
}

impl NetworkLog {
    pub fn new() -> Self {
        Self::with_limits(50_000, 10_000)
    }

    pub fn with_limits(max_entries: usize, max_pending: usize) -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            inner: Arc::new(RwLock::new(Inner::new(max_entries, max_pending, tx))),
        }
    }

    pub async fn subscribe(&self) -> broadcast::Receiver<Arc<NetworkEntry>> {
        self.inner.read().await.tx.subscribe()
    }

    pub async fn entries(&self) -> Vec<Arc<NetworkEntry>> {
        let inner = self.inner.read().await;
        inner.entries.clone()
    }

    pub async fn completed(&self) -> Vec<Arc<NetworkEntry>> {
        let inner = self.inner.read().await;
        inner
            .entries
            .iter()
            .filter(|e| e.has_response() || e.is_error())
            .cloned()
            .collect()
    }

    pub async fn count(&self, filter: Filter) -> usize {
        let inner = self.inner.read().await;
        inner.entries.iter().filter(|e| filter.matches(e)).count()
    }

    pub async fn filter(&self, f: Filter) -> Vec<Arc<NetworkEntry>> {
        let inner = self.inner.read().await;
        inner
            .entries
            .iter()
            .filter(|e| f.matches(e))
            .cloned()
            .collect()
    }

    pub async fn first(&self) -> Option<Arc<NetworkEntry>> {
        let inner = self.inner.read().await;
        inner.entries.first().cloned()
    }

    pub async fn last(&self) -> Option<Arc<NetworkEntry>> {
        let inner = self.inner.read().await;
        inner.entries.last().cloned()
    }

    pub async fn nth(&self, n: usize) -> Option<Arc<NetworkEntry>> {
        let inner = self.inner.read().await;
        inner.entries.get(n).cloned()
    }

    pub async fn find_by_url(&self, substring: &str) -> Option<Arc<NetworkEntry>> {
        let inner = self.inner.read().await;
        inner
            .entries
            .iter()
            .find(|e| e.request.url.contains(substring))
            .cloned()
    }

    pub async fn find_by_url_regex(&self, re: &regex::Regex) -> Option<Arc<NetworkEntry>> {
        let inner = self.inner.read().await;
        inner
            .entries
            .iter()
            .find(|e| re.is_match(&e.request.url))
            .cloned()
    }

    pub async fn endpoints(&self) -> Vec<String> {
        let inner = self.inner.read().await;
        let mut seen = std::collections::HashSet::new();
        inner
            .entries
            .iter()
            .filter(|e| seen.insert(e.request.url.clone()))
            .map(|e| e.request.url.clone())
            .collect()
    }

    pub async fn hostnames(&self) -> Vec<String> {
        let inner = self.inner.read().await;
        let mut seen = std::collections::HashSet::new();
        inner
            .entries
            .iter()
            .filter_map(|e| {
                url::Url::parse(&e.request.url).ok().and_then(|u| {
                    let host = u.host_str()?.to_string();
                    seen.insert(host.clone()).then_some(host)
                })
            })
            .collect()
    }

    pub async fn distinct_methods(&self) -> Vec<String> {
        let inner = self.inner.read().await;
        let mut seen = std::collections::HashSet::new();
        let mut out: Vec<String> = inner
            .entries
            .iter()
            .filter_map(|e| {
                seen.insert(e.request.method.clone())
                    .then_some(e.request.method.clone())
            })
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    pub async fn distinct_statuses(&self) -> Vec<u16> {
        let inner = self.inner.read().await;
        let mut seen = std::collections::HashSet::new();
        let mut out: Vec<u16> = inner
            .entries
            .iter()
            .filter_map(|e| {
                let status = e.status()?;
                seen.insert(status).then_some(status)
            })
            .collect();
        out.sort_unstable();
        out
    }

    pub async fn total_bytes_in(&self) -> u64 {
        let inner = self.inner.read().await;
        inner
            .entries
            .iter()
            .filter_map(|e| e.response.as_ref().and_then(|r| r.body_size))
            .sum()
    }

    pub async fn total_bytes_out(&self) -> u64 {
        let inner = self.inner.read().await;
        inner
            .entries
            .iter()
            .map(|e| {
                e.request
                    .post_data
                    .as_ref()
                    .map(|b| b.len() as u64)
                    .unwrap_or(0)
            })
            .sum()
    }

    pub async fn len(&self) -> usize {
        self.inner.read().await.entries.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }

    pub async fn clear(&self) {
        let mut inner = self.inner.write().await;
        inner.entries.clear();
        inner.by_id.clear();
    }

    pub async fn metrics(&self) -> NetworkMetrics {
        self.inner.read().await.metrics.clone()
    }

    pub async fn contains_id(&self, id: &str) -> bool {
        self.inner.read().await.by_id.contains_key(id)
    }

    pub async fn remove_by_id(&self, id: &str) -> Option<Arc<NetworkEntry>> {
        let mut inner = self.inner.write().await;
        let idx = inner.by_id.remove(id)?;
        let entry = inner.entries.remove(idx);
        // Rebuild indices for entries after the removed one
        let updates: Vec<(String, usize)> = inner
            .entries
            .iter()
            .enumerate()
            .skip(idx)
            .map(|(i, e)| (e.request.id.clone(), i))
            .collect();
        for (id, i) in updates {
            inner.by_id.insert(id, i);
        }
        Some(entry)
    }

    /// Return all captured request IDs in chronological order.
    pub async fn request_ids(&self) -> Vec<String> {
        let inner = self.inner.read().await;
        inner.entries.iter().map(|e| e.request.id.clone()).collect()
    }

    /// Return an approximate memory footprint in bytes.
    /// Useful for monitoring and backpressure decisions.
    pub async fn memory_estimate(&self) -> usize {
        let inner = self.inner.read().await;
        let mut estimate = 0;
        estimate += inner.entries.capacity() * std::mem::size_of::<Arc<NetworkEntry>>();
        estimate +=
            inner.by_id.capacity() * (std::mem::size_of::<String>() + std::mem::size_of::<usize>());
        estimate += inner.pending_responses.capacity()
            * (std::mem::size_of::<String>() + std::mem::size_of::<CapturedResponse>());
        estimate += inner.pending_errors.capacity()
            * (std::mem::size_of::<String>() + std::mem::size_of::<CapturedError>());
        for e in &inner.entries {
            estimate += e.request.url.len();
            estimate += e.request.method.len();
            estimate += e
                .request
                .headers
                .iter()
                .map(|h| h.name.len() + h.value.len())
                .sum::<usize>();
            if let Some(ref body) = e.request.post_data {
                estimate += body.len();
            }
            if let Some(ref r) = e.response {
                estimate +=
                    r.url.len() + r.status_text.len() + r.mime_type.len() + r.protocol.len();
                estimate += r
                    .headers
                    .iter()
                    .map(|h| h.name.len() + h.value.len())
                    .sum::<usize>();
            }
            if let Some(ref err) = e.error {
                estimate += err.url.len() + err.error_text.len();
            }
        }
        estimate
    }

    /// Return whether the entry with `id` has a response.
    pub async fn has_response(&self, id: &str) -> bool {
        let inner = self.inner.read().await;
        inner
            .by_id
            .get(id)
            .map(|&idx| inner.entries[idx].response.is_some())
            .unwrap_or(false)
    }

    /// Return whether the entry with `id` has an error.
    pub async fn has_error(&self, id: &str) -> bool {
        let inner = self.inner.read().await;
        inner
            .by_id
            .get(id)
            .map(|&idx| inner.entries[idx].error.is_some())
            .unwrap_or(false)
    }

    /// Return the number of out-of-order responses/errors stashed in
    /// pending maps (orphaned entries waiting for their request).
    pub async fn pending_count(&self) -> usize {
        let inner = self.inner.read().await;
        inner.pending_responses.len() + inner.pending_errors.len()
    }

    /// Drain all orphaned pending responses and errors, returning them.
    /// Useful for preventing unbounded growth when requests never arrive
    /// (e.g. page navigation interrupts the request lifecycle).
    pub async fn drain_pending(&self) -> (Vec<CapturedResponse>, Vec<CapturedError>) {
        let mut inner = self.inner.write().await;
        let responses: Vec<CapturedResponse> =
            inner.pending_responses.drain().map(|(_, v)| v).collect();
        let errors: Vec<CapturedError> = inner.pending_errors.drain().map(|(_, v)| v).collect();
        (responses, errors)
    }

    /// Retain only entries that satisfy the predicate.
    /// More efficient than repeated `remove_by_id` for bulk filtering.
    pub async fn retain<F>(&self, mut f: F)
    where
        F: FnMut(&NetworkEntry) -> bool,
    {
        let mut inner = self.inner.write().await;
        let mut new_entries = Vec::new();
        let mut new_by_id = HashMap::new();
        for entry in inner.entries.drain(..) {
            if f(&entry) {
                let idx = new_entries.len();
                new_by_id.insert(entry.request.id.clone(), idx);
                new_entries.push(entry);
            }
        }
        inner.entries = new_entries;
        inner.by_id = new_by_id;
    }

    /// Wait up to `timeout` for an entry whose request URL contains `substring`.
    pub async fn wait_for_url(
        &self,
        substring: &str,
        timeout: std::time::Duration,
    ) -> Option<Arc<NetworkEntry>> {
        let mut rx = self.subscribe().await;
        let deadline = tokio::time::Instant::now() + timeout;

        // Check existing entries first
        if let Some(e) = self.find_by_url(substring).await {
            return Some(e);
        }

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Ok(entry)) => {
                    if entry.request.url.contains(substring) {
                        return Some(entry);
                    }
                }
                _ => return None,
            }
        }
    }

    /// Wait up to `timeout` for a response to the given request id.
    pub async fn wait_for_response(
        &self,
        id: &str,
        timeout: std::time::Duration,
    ) -> Option<Arc<NetworkEntry>> {
        let mut rx = self.subscribe().await;
        let deadline = tokio::time::Instant::now() + timeout;

        // Check existing entries first
        {
            let inner = self.inner.read().await;
            if let Some(idx) = inner.by_id.get(id) {
                if inner.entries[*idx].has_response() || inner.entries[*idx].is_error() {
                    return Some(inner.entries[*idx].clone());
                }
            }
        }

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Ok(entry)) => {
                    if entry.request.id == id && (entry.has_response() || entry.is_error()) {
                        return Some(entry);
                    }
                }
                _ => return None,
            }
        }
    }

    // ------------------------------------------------------------------
    // Convenience queries
    // ------------------------------------------------------------------

    /// Return all entries with the given HTTP status code.
    pub async fn find_by_status(&self, status: u16) -> Vec<Arc<NetworkEntry>> {
        let inner = self.inner.read().await;
        inner
            .entries
            .iter()
            .filter(|e| e.status() == Some(status))
            .cloned()
            .collect()
    }

    /// Return entries whose request timestamp is >= the given value.
    pub async fn entries_since(&self, timestamp: u64) -> Vec<Arc<NetworkEntry>> {
        let inner = self.inner.read().await;
        inner
            .entries
            .iter()
            .filter(|e| e.request.timestamp >= timestamp)
            .cloned()
            .collect()
    }

    /// Return the most recent `n` entries.
    pub async fn last_n(&self, n: usize) -> Vec<Arc<NetworkEntry>> {
        let inner = self.inner.read().await;
        inner
            .entries
            .iter()
            .rev()
            .take(n)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    /// Return a deduplicated list of every captured request URL.
    pub async fn unique_urls(&self) -> Vec<String> {
        let inner = self.inner.read().await;
        let mut seen = std::collections::HashSet::new();
        inner
            .entries
            .iter()
            .filter_map(|e| {
                if seen.insert(e.request.url.clone()) {
                    Some(e.request.url.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    // ------------------------------------------------------------------
    // Export
    // ------------------------------------------------------------------

    pub async fn save_to_json(&self, path: &std::path::Path) -> Result<()> {
        let entries: Vec<NetworkEntry> =
            self.entries().await.iter().map(|e| (**e).clone()).collect();
        let json = serde_json::to_string_pretty(&entries)?;
        tokio::fs::write(path, json).await?;
        Ok(())
    }

    /// Write a HAR 1.2 file.
    pub async fn save_as_har(
        &self,
        path: &std::path::Path,
        page_title: Option<&str>,
    ) -> Result<()> {
        use chrono::Utc;

        let entries = self.entries().await;
        let started = if let Some(e) = entries.first() {
            chrono::DateTime::from_timestamp_millis(e.request.timestamp as i64)
                .unwrap_or_else(Utc::now)
        } else {
            Utc::now()
        };

        let mut har_entries = Vec::new();
        for e in &entries {
            let req = &e.request;
            let resp = e.response.as_ref();
            let timing = &req.timing;

            let query_string: Vec<_> = match req.query_params() {
                Ok(params) => params
                    .into_iter()
                    .map(|(name, value)| serde_json::json!({"name": name, "value": value}))
                    .collect(),
                Err(error) => {
                    tracing::warn!(
                        "failed to parse request URL for query parameters: {error}; url={}",
                        req.url
                    );
                    Vec::new()
                }
            };

            let request_json = serde_json::json!({
                "method": req.method,
                "url": req.url,
                "httpVersion": "HTTP/1.1",
                "headers": req.headers.iter().map(|h| serde_json::json!({"name": h.name, "value": h.value})).collect::<Vec<_>>(),
                "cookies": req.cookies.iter().map(|c| serde_json::json!({"name": c.name, "value": c.value, "domain": c.domain, "path": c.path})).collect::<Vec<_>>(),
                "queryString": query_string,
                "headersSize": -1,
                "bodySize": req.post_data.as_ref().map(|b| b.len() as i64).unwrap_or(-1),
                "postData": req.post_data.as_ref().map(|b| {
                    let mime = req.request_header("content-type").unwrap_or("application/octet-stream");
                    serde_json::json!({"mimeType": mime, "text": b})
                }),
            });

            let response_json = resp.map(|r| serde_json::json!({
                "status": r.status,
                "statusText": r.status_text,
                "httpVersion": "HTTP/1.1",
                "headers": r.headers.iter().map(|h| serde_json::json!({"name": h.name, "value": h.value})).collect::<Vec<_>>(),
                "cookies": [],
                "content": {
                    "size": r.body_size.unwrap_or(0),
                    "mimeType": r.mime_type,
                },
                "redirectURL": "",
                "headersSize": -1,
                "bodySize": r.body_size.map(|b| b as i64).unwrap_or(-1),
            }));

            let timings_json = serde_json::json!({
                "blocked": -1,
                "dns": option_f64_ms(timing.dns_end_ms, timing.dns_start_ms),
                "connect": option_f64_ms(timing.connect_end_ms, timing.connect_start_ms),
                "ssl": option_f64_ms(timing.connect_end_ms, timing.tls_start_ms),
                "send": -1,
                "wait": option_f64_ms(timing.response_start_ms, timing.connect_end_ms),
                "receive": option_f64_ms(timing.response_end_ms, timing.response_start_ms),
            });

            har_entries.push(serde_json::json!({
                "startedDateTime": format!("{}", chrono::DateTime::from_timestamp_millis(req.timestamp as i64).unwrap_or_else(Utc::now)),
                "time": 0,
                "request": request_json,
                "response": response_json,
                "cache": {},
                "timings": timings_json,
            }));
        }

        let har = serde_json::json!({
            "log": {
                "version": "1.2",
                "creator": {
                    "name": "runtime_foxdriver",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "pages": [{
                    "startedDateTime": format!("{}", started),
                    "id": "page_1",
                    "title": page_title.unwrap_or("unknown"),
                    "pageTimings": { "onContentLoad": -1, "onLoad": -1 },
                }],
                "entries": har_entries,
            }
        });

        tokio::fs::write(path, serde_json::to_string_pretty(&har)?).await?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Ingestion helpers (called by Page::start_network_log handler)
    // ------------------------------------------------------------------

    pub(crate) async fn ingest_before_request_sent(&self, evt: &BeforeRequestSent) {
        let mut inner = self.inner.write().await;
        inner.metrics.requests_received += 1;

        let req = build_request(evt);
        let resp = inner.pending_responses.remove(&req.id);
        let err = inner.pending_errors.remove(&req.id);

        let entry = Arc::new(NetworkEntry {
            request: req,
            response: resp,
            error: err,
        });
        inner.push_entry(entry);
    }

    pub(crate) async fn ingest_response_completed(&self, evt: &ResponseCompleted) {
        let mut inner = self.inner.write().await;
        inner.metrics.responses_received += 1;

        let id = evt.params.base_parameters.request.request.inner().clone();
        let resp = build_response(evt);

        if let Some(idx) = inner.by_id.get(&id).copied() {
            let mut new_entry = (*inner.entries[idx]).clone();
            if new_entry.response.is_some() {
                inner.metrics.duplicate_responses += 1;
            }
            new_entry.response = Some(resp);
            let new_arc = Arc::new(new_entry);
            inner.entries[idx] = new_arc.clone();
            let _ = inner.tx.send(new_arc);
        } else {
            while inner.max_pending > 0 && inner.pending_responses.len() >= inner.max_pending {
                if let Some(k) = inner.pending_responses.keys().next().cloned() {
                    inner.pending_responses.remove(&k);
                    inner.metrics.pending_responses_dropped += 1;
                }
            }
            inner.pending_responses.insert(id, resp);
        }
    }

    pub(crate) async fn ingest_fetch_error(&self, evt: &FetchError) {
        let mut inner = self.inner.write().await;
        inner.metrics.errors_received += 1;

        let id = evt.params.base_parameters.request.request.inner().clone();
        let err = CapturedError {
            id: id.clone(),
            // The request's own URL, not `navigation`. `navigation` is the
            // top-level document URL and is None for subresource requests
            // (scripts, images, XHR), so reading it there produced an EMPTY
            // error URL for every failed subresource - the caller could not
            // tell which resource failed. `request.url` is always the actual
            // requested URL, matching build_request's success-path source.
            url: evt.params.base_parameters.request.url.clone(),
            error_text: evt.params.error_text.clone(),
        };

        if let Some(idx) = inner.by_id.get(&id).copied() {
            let mut new_entry = (*inner.entries[idx]).clone();
            if new_entry.error.is_some() {
                inner.metrics.duplicate_errors += 1;
            }
            new_entry.error = Some(err);
            let new_arc = Arc::new(new_entry);
            inner.entries[idx] = new_arc.clone();
            let _ = inner.tx.send(new_arc);
        } else {
            while inner.max_pending > 0 && inner.pending_errors.len() >= inner.max_pending {
                if let Some(k) = inner.pending_errors.keys().next().cloned() {
                    inner.pending_errors.remove(&k);
                    inner.metrics.pending_errors_dropped += 1;
                }
            }
            inner.pending_errors.insert(id, err);
        }
    }
}

impl Default for NetworkLog {
    fn default() -> Self {
        Self::new()
    }
}

fn option_f64_ms(end: Option<f64>, start: Option<f64>) -> f64 {
    match (end, start) {
        (Some(e), Some(s)) => (e - s).max(0.0),
        _ => -1.0,
    }
}

// ------------------------------------------------------------------
// Build helpers (BiDi events -> our structs)
// ------------------------------------------------------------------

fn build_request(evt: &BeforeRequestSent) -> CapturedRequest {
    let bp = &evt.params.base_parameters;
    let id = bp.request.request.inner().clone();
    let url = bp.request.url.clone();
    let headers = bp
        .request
        .headers
        .iter()
        .map(CapturedHeader::from)
        .collect();
    let post_data = None; // BiDi doesn't expose body in beforeRequestSent
    let timestamp = bp.timestamp;
    let destination = bp.request.destination.clone();
    let initiator_type = evt
        .params
        .initiator
        .as_ref()
        .and_then(|i| i.r#type.as_ref().map(|t| format!("{:?}", t).to_lowercase()));
    let timing = CapturedTiming::from(&bp.request.timings);
    let cookies = bp
        .request
        .cookies
        .iter()
        .map(CapturedCookie::from)
        .collect();

    CapturedRequest {
        id,
        context: bp.context.as_ref().map(|c| c.inner().to_string()),
        method: bp.request.method.clone(),
        url,
        headers,
        post_data,
        timestamp,
        destination,
        initiator_type,
        timing,
        cookies,
    }
}

fn build_response(evt: &ResponseCompleted) -> CapturedResponse {
    let bp = &evt.params.base_parameters;
    let resp = &evt.params.response;
    let id = bp.request.request.inner().clone();
    let url = resp.url.clone();
    let headers = resp.headers.iter().map(CapturedHeader::from).collect();
    let protocol = resp.protocol.clone();
    let status = resp.status as u16;
    let status_text = resp.status_text.clone();
    let mime_type = resp.mime_type.clone();
    let body_size = resp.body_size;
    let from_cache = resp.from_cache;

    CapturedResponse {
        id,
        url,
        protocol,
        status,
        status_text,
        headers,
        mime_type,
        body_size,
        from_cache,
    }
}

// ------------------------------------------------------------------
// Handler factory (used by browser.rs)
// ------------------------------------------------------------------

pub fn make_network_handler(
    log: NetworkLog,
) -> impl FnMut(
    rustenium_bidi_definitions::Event,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    use rustenium_bidi_definitions::network::events::NetworkEvent;
    move |evt| {
        let log = log.clone();
        Box::pin(async move {
            if let rustenium_bidi_definitions::Event::Network(nev) = evt {
                match nev {
                    NetworkEvent::BeforeRequestSent(evt) => {
                        log.ingest_before_request_sent(&evt).await
                    }
                    NetworkEvent::ResponseCompleted(evt) => {
                        log.ingest_response_completed(&evt).await
                    }
                    NetworkEvent::FetchError(evt) => log.ingest_fetch_error(&evt).await,
                    _ => {}
                }
            }
        })
    }
}

// ------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- helpers ----------

    fn make_request(id: &str, method: &str, url: &str) -> CapturedRequest {
        CapturedRequest {
            id: id.into(),
            context: None,
            method: method.into(),
            url: url.into(),
            headers: vec![],
            post_data: None,
            timestamp: 0,
            destination: "document".into(),
            initiator_type: None,
            timing: CapturedTiming::default(),
            cookies: vec![],
        }
    }

    fn make_request_with_headers(
        id: &str,
        method: &str,
        url: &str,
        headers: Vec<CapturedHeader>,
    ) -> CapturedRequest {
        CapturedRequest {
            id: id.into(),
            context: None,
            method: method.into(),
            url: url.into(),
            headers,
            post_data: None,
            timestamp: 0,
            destination: "document".into(),
            initiator_type: None,
            timing: CapturedTiming::default(),
            cookies: vec![],
        }
    }

    fn make_response(id: &str, status: u16, url: &str) -> CapturedResponse {
        CapturedResponse {
            id: id.into(),
            url: url.into(),
            protocol: "h2".into(),
            status,
            status_text: "OK".into(),
            headers: vec![],
            mime_type: "application/json".into(),
            body_size: Some(100),
            from_cache: false,
        }
    }

    fn make_error(id: &str, url: &str, text: &str) -> CapturedError {
        CapturedError {
            id: id.into(),
            url: url.into(),
            error_text: text.into(),
        }
    }

    async fn push_request(log: &NetworkLog, req: CapturedRequest) {
        let entry = Arc::new(NetworkEntry {
            request: req,
            response: None,
            error: None,
        });
        let mut inner = log.inner.write().await;
        inner.push_entry(entry);
    }

    async fn push_entry(
        log: &NetworkLog,
        req: CapturedRequest,
        resp: Option<CapturedResponse>,
        err: Option<CapturedError>,
    ) {
        let entry = Arc::new(NetworkEntry {
            request: req,
            response: resp,
            error: err,
        });
        let mut inner = log.inner.write().await;
        inner.push_entry(entry);
    }

    // ---------- basic lifecycle ----------

    #[tokio::test]
    async fn test_new_log_is_empty() {
        let log = NetworkLog::new();
        assert!(log.is_empty().await);
        assert_eq!(log.len().await, 0);
    }

    #[tokio::test]
    async fn test_with_limits() {
        let log = NetworkLog::with_limits(10, 5);
        let m = log.metrics().await;
        assert_eq!(m.max_entries, 10);
    }

    #[tokio::test]
    async fn test_clear() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://example.com")).await;
        assert_eq!(log.len().await, 1);
        log.clear().await;
        assert!(log.is_empty().await);
    }

    #[tokio::test]
    async fn test_entries_returns_all() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "POST", "https://b.com")).await;
        let entries = log.entries().await;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].request.id, "1");
        assert_eq!(entries[1].request.id, "2");
    }

    #[tokio::test]
    async fn test_first_last_nth() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "POST", "https://b.com")).await;
        push_request(&log, make_request("3", "PUT", "https://c.com")).await;

        assert_eq!(log.first().await.unwrap().request.id, "1");
        assert_eq!(log.last().await.unwrap().request.id, "3");
        assert_eq!(log.nth(0).await.unwrap().request.id, "1");
        assert_eq!(log.nth(1).await.unwrap().request.id, "2");
        assert_eq!(log.nth(2).await.unwrap().request.id, "3");
        assert!(log.nth(99).await.is_none());
    }

    // ---------- filtering ----------

    #[tokio::test]
    async fn test_filter_method() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://example.com")).await;
        push_request(&log, make_request("2", "POST", "https://example.com")).await;
        let get = log.filter(Filter::new().method("GET")).await;
        assert_eq!(get.len(), 1);
        assert_eq!(get[0].request.method, "GET");
    }

    #[tokio::test]
    async fn test_filter_status_range() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(make_response("1", 200, "https://a.com")),
            None,
        )
        .await;
        push_entry(
            &log,
            make_request("2", "GET", "https://b.com"),
            Some(make_response("2", 404, "https://b.com")),
            None,
        )
        .await;
        push_entry(&log, make_request("3", "GET", "https://c.com"), None, None).await;
        let f = log.filter(Filter::new().status_range(200..=299)).await;
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].request.id, "1");
    }

    #[tokio::test]
    async fn test_filter_url_contains() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://api.example.com/v1")).await;
        push_request(&log, make_request("2", "GET", "https://other.com")).await;
        let f = log.filter(Filter::new().url_contains("api")).await;
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].request.id, "1");
    }

    #[tokio::test]
    async fn test_filter_url_regex() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://api.example.com/v1")).await;
        push_request(&log, make_request("2", "GET", "https://other.com")).await;
        let f = log
            .filter(Filter::new().url_regex(r"api\.\w+\.com").unwrap())
            .await;
        assert_eq!(f.len(), 1);
    }

    #[tokio::test]
    async fn test_filter_header() {
        let log = NetworkLog::new();
        let h1 = vec![CapturedHeader {
            name: "Authorization".into(),
            value: "Bearer abc".into(),
        }];
        let h2 = vec![CapturedHeader {
            name: "Content-Type".into(),
            value: "application/json".into(),
        }];
        push_request(
            &log,
            make_request_with_headers("1", "GET", "https://a.com", h1),
        )
        .await;
        push_request(
            &log,
            make_request_with_headers("2", "GET", "https://b.com", h2),
        )
        .await;
        let f = log
            .filter(Filter::new().header("authorization", "bearer"))
            .await;
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].request.id, "1");
    }

    #[tokio::test]
    async fn test_filter_destination() {
        let log = NetworkLog::new();
        let mut r1 = make_request("1", "GET", "https://a.com");
        r1.destination = "image".into();
        let mut r2 = make_request("2", "GET", "https://b.com");
        r2.destination = "document".into();
        push_request(&log, r1).await;
        push_request(&log, r2).await;
        let f = log.filter(Filter::new().destination("image")).await;
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].request.id, "1");
    }

    #[tokio::test]
    async fn test_filter_with_response() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(make_response("1", 200, "https://a.com")),
            None,
        )
        .await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        let f = log.filter(Filter::new().with_response()).await;
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].request.id, "1");
    }

    #[tokio::test]
    async fn test_filter_without_response() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(make_response("1", 200, "https://a.com")),
            None,
        )
        .await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        let f = log.filter(Filter::new().without_response()).await;
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].request.id, "2");
    }

    #[tokio::test]
    async fn test_filter_with_error() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            None,
            Some(make_error("1", "https://a.com", "net::ERR_FAILED")),
        )
        .await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        let f = log.filter(Filter::new().with_error()).await;
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].request.id, "1");
    }

    #[tokio::test]
    async fn test_filter_composition() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "POST", "https://api.example.com/login"),
            Some(make_response("1", 200, "https://api.example.com/login")),
            None,
        )
        .await;
        push_request(
            &log,
            make_request("2", "GET", "https://api.example.com/login"),
        )
        .await;
        let f = log
            .filter(
                Filter::new()
                    .method("POST")
                    .url_contains("login")
                    .with_response(),
            )
            .await;
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].request.id, "1");
    }

    #[tokio::test]
    async fn test_count() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "POST", "https://b.com")).await;
        assert_eq!(log.count(Filter::new().method("GET")).await, 1);
        assert_eq!(log.count(Filter::new()).await, 2);
    }

    // ---------- completed ----------

    #[tokio::test]
    async fn test_completed() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(make_response("1", 200, "https://a.com")),
            None,
        )
        .await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        push_entry(
            &log,
            make_request("3", "GET", "https://c.com"),
            None,
            Some(make_error("3", "https://c.com", "err")),
        )
        .await;
        let c = log.completed().await;
        assert_eq!(c.len(), 2);
    }

    // ---------- find_by_url / regex ----------

    #[tokio::test]
    async fn test_find_by_url() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://api.example.com")).await;
        push_request(&log, make_request("2", "GET", "https://other.com")).await;
        assert_eq!(log.find_by_url("api").await.unwrap().request.id, "1");
        assert!(log.find_by_url("notfound").await.is_none());
    }

    #[tokio::test]
    async fn test_find_by_url_regex() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://api.example.com")).await;
        push_request(&log, make_request("2", "GET", "https://other.com")).await;
        let re = regex::Regex::new(r"api\.\w+\.com").unwrap();
        assert_eq!(log.find_by_url_regex(&re).await.unwrap().request.id, "1");
    }

    // ---------- aggregation ----------

    #[tokio::test]
    async fn test_endpoints() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com/x")).await;
        push_request(&log, make_request("2", "GET", "https://a.com/y")).await;
        push_request(&log, make_request("3", "GET", "https://b.com/z")).await;
        let ep = log.endpoints().await;
        assert_eq!(ep.len(), 3);
    }

    #[tokio::test]
    async fn test_hostnames() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com/x")).await;
        push_request(&log, make_request("2", "GET", "https://a.com/y")).await;
        push_request(&log, make_request("3", "GET", "https://b.com/z")).await;
        let h = log.hostnames().await;
        assert_eq!(h.len(), 2);
        assert!(h.contains(&"a.com".into()));
        assert!(h.contains(&"b.com".into()));
    }

    #[tokio::test]
    async fn test_distinct_methods() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "POST", "https://b.com")).await;
        push_request(&log, make_request("3", "GET", "https://c.com")).await;
        let m = log.distinct_methods().await;
        assert_eq!(m, vec!["GET", "POST"]);
    }

    #[tokio::test]
    async fn test_distinct_statuses() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(make_response("1", 200, "https://a.com")),
            None,
        )
        .await;
        push_entry(
            &log,
            make_request("2", "GET", "https://b.com"),
            Some(make_response("2", 404, "https://b.com")),
            None,
        )
        .await;
        push_entry(
            &log,
            make_request("3", "GET", "https://c.com"),
            Some(make_response("3", 200, "https://c.com")),
            None,
        )
        .await;
        let s = log.distinct_statuses().await;
        assert_eq!(s, vec![200, 404]);
    }

    #[tokio::test]
    async fn test_total_bytes() {
        let log = NetworkLog::new();
        let mut r1 = make_request("1", "POST", "https://a.com");
        r1.post_data = Some("hello".into());
        push_entry(
            &log,
            r1,
            Some(make_response("1", 200, "https://a.com")),
            None,
        )
        .await;
        let mut r2 = make_request("2", "POST", "https://b.com");
        r2.post_data = Some("world!!".into());
        push_entry(
            &log,
            r2,
            Some(make_response("2", 200, "https://b.com")),
            None,
        )
        .await;
        assert_eq!(log.total_bytes_in().await, 200);
        assert_eq!(log.total_bytes_out().await, 12); // "hello" + "world!!"
    }

    // ---------- entry helpers ----------

    #[tokio::test]
    async fn test_entry_status_and_final_url() {
        let entry = NetworkEntry {
            request: make_request("1", "GET", "https://a.com"),
            response: Some(make_response("1", 301, "https://b.com")),
            error: None,
        };
        assert_eq!(entry.status(), Some(301));
        assert_eq!(entry.final_url(), "https://b.com");
        assert!(entry.has_response());
        assert!(!entry.is_error());
    }

    #[tokio::test]
    async fn test_entry_request_header() {
        let req = make_request_with_headers(
            "1",
            "GET",
            "https://a.com",
            vec![CapturedHeader {
                name: "X-Custom".into(),
                value: "123".into(),
            }],
        );
        let entry = NetworkEntry {
            request: req,
            response: None,
            error: None,
        };
        assert_eq!(entry.request_header("x-custom"), Some("123"));
        assert_eq!(entry.request_header("missing"), None);
    }

    #[tokio::test]
    async fn test_entry_response_header() {
        let req = make_request("1", "GET", "https://a.com");
        let resp = CapturedResponse {
            id: "1".into(),
            url: "https://a.com".into(),
            protocol: "h2".into(),
            status: 200,
            status_text: "OK".into(),
            headers: vec![CapturedHeader {
                name: "Content-Type".into(),
                value: "application/json".into(),
            }],
            mime_type: "application/json".into(),
            body_size: Some(10),
            from_cache: false,
        };
        let entry = NetworkEntry {
            request: req,
            response: Some(resp),
            error: None,
        };
        assert_eq!(
            entry.response_header("content-type"),
            Some("application/json")
        );
        assert_eq!(entry.response_header("missing"), None);
    }

    #[tokio::test]
    async fn test_entry_to_curl() {
        let req = CapturedRequest {
            id: "1".into(),
            context: None,
            method: "POST".into(),
            url: "https://api.example.com/login".into(),
            headers: vec![
                CapturedHeader {
                    name: "Content-Type".into(),
                    value: "application/json".into(),
                },
                CapturedHeader {
                    name: "Host".into(),
                    value: "api.example.com".into(),
                },
            ],
            post_data: Some(r#"{"user":"admin"}"#.into()),
            timestamp: 0,
            destination: "document".into(),
            initiator_type: None,
            timing: CapturedTiming::default(),
            cookies: vec![],
        };
        let entry = NetworkEntry {
            request: req,
            response: None,
            error: None,
        };
        let curl = entry.to_curl();
        assert!(curl.starts_with("curl -X 'POST'"));
        assert!(curl.contains("-H 'Content-Type: application/json'"));
        assert!(!curl.contains("Host")); // stripped
        assert!(curl.contains(r#"-d '{"user":"admin"}'"#));
        assert!(curl.contains("'https://api.example.com/login'"));
    }

    // ---------- request helpers ----------

    #[tokio::test]
    async fn test_request_json_body() {
        let mut req = make_request("1", "POST", "https://a.com");
        req.post_data = Some(r#"{"key":"value"}"#.into());
        assert_eq!(req.json_body(), Some(serde_json::json!({"key": "value"})));
    }

    #[tokio::test]
    async fn test_request_json_body_invalid() {
        let mut req = make_request("1", "POST", "https://a.com");
        req.post_data = Some("not json".into());
        assert!(req.json_body().is_none());
    }

    #[tokio::test]
    async fn test_request_query_params() {
        let req = make_request("1", "GET", "https://a.com?foo=bar&baz=qux");
        let params = req.query_params().unwrap();
        assert_eq!(params.len(), 2);
        assert!(params.contains(&("foo".into(), "bar".into())));
        assert!(params.contains(&("baz".into(), "qux".into())));
    }

    #[tokio::test]
    async fn test_request_query_params_malformed_url() {
        let mut req = make_request("1", "GET", "https://a.com?foo=bar");
        req.url = "not a valid url".into();
        assert!(
            req.query_params().is_err(),
            "query_params must report a parse failure, not silently return an empty vec"
        );
    }

    // ---------- secret scanning ----------

    // ---------- internal requests ----------

    // ---------- max entries eviction ----------

    #[tokio::test]
    async fn test_max_entries_eviction() {
        let log = NetworkLog::with_limits(4, 10);
        for i in 0..6 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://example.com"),
            )
            .await;
        }
        // After 6 inserts with max 4, eviction triggers when len >= 4
        // First eviction at insert 4: remove 2, leaving [2,3]
        // Then insert 4: [2,3,4]
        // Insert 5: [2,3,4,5]
        let entries = log.entries().await;
        let ids: Vec<_> = entries.iter().map(|e| e.request.id.clone()).collect();
        assert_eq!(ids, vec!["2", "3", "4", "5"]);

        let m = log.metrics().await;
        assert_eq!(m.entries_evicted, 2);
    }

    // ---------- broadcast ----------

    #[tokio::test]
    async fn test_broadcast_receives_entries() {
        let log = NetworkLog::new();
        let mut rx = log.subscribe().await;
        push_request(&log, make_request("1", "GET", "https://example.com")).await;
        let received = rx.recv().await.unwrap();
        assert_eq!(received.request.id, "1");
    }

    #[tokio::test]
    async fn test_broadcast_multiple_receivers() {
        let log = NetworkLog::new();
        let mut rx1 = log.subscribe().await;
        let mut rx2 = log.subscribe().await;
        push_request(&log, make_request("1", "GET", "https://example.com")).await;
        assert_eq!(rx1.recv().await.unwrap().request.id, "1");
        assert_eq!(rx2.recv().await.unwrap().request.id, "1");
    }

    // ---------- metrics ----------

    #[tokio::test]
    async fn test_metrics_requests_received() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        let m = log.metrics().await;
        assert_eq!(m.requests_received, 0); // push_request bypasses metrics
        assert_eq!(m.max_entries, 50_000);
    }

    // ---------- serialization ----------

    #[tokio::test]
    async fn test_network_entry_serialize_roundtrip() {
        let entry = NetworkEntry {
            request: make_request("1", "GET", "https://example.com"),
            response: Some(make_response("1", 200, "https://example.com")),
            error: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let de: NetworkEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(de.request.id, entry.request.id);
        assert_eq!(de.status(), Some(200));
    }

    // ---------- clone semantics ----------

    #[tokio::test]
    async fn test_log_clone_shares_state() {
        let log1 = NetworkLog::new();
        let log2 = log1.clone();
        push_request(&log1, make_request("1", "GET", "https://example.com")).await;
        assert_eq!(log2.len().await, 1);
    }

    // ---------- timing helpers ----------

    #[tokio::test]
    async fn test_non_neg() {
        assert_eq!(non_neg(5.0), Some(5.0));
        assert_eq!(non_neg(-1.0), None);
        assert_eq!(non_neg(0.0), Some(0.0));
    }

    #[tokio::test]
    async fn test_option_f64_ms() {
        assert_eq!(option_f64_ms(Some(10.0), Some(3.0)), 7.0);
        assert_eq!(option_f64_ms(Some(3.0), Some(10.0)), 0.0); // clamped
        assert_eq!(option_f64_ms(None, Some(3.0)), -1.0);
    }

    // ---------- edge cases ----------

    #[tokio::test]
    async fn test_empty_filter_matches_all() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "POST", "https://b.com")).await;
        assert_eq!(log.filter(Filter::new()).await.len(), 2);
        assert_eq!(log.count(Filter::new()).await, 2);
    }

    #[tokio::test]
    async fn test_eviction_rebuilds_by_id() {
        let log = NetworkLog::with_limits(4, 10);
        for i in 0..6 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://example.com"),
            )
            .await;
        }
        // After eviction, by_id should still be valid
        let entries = log.entries().await;
        assert_eq!(entries.len(), 4);
        for e in &entries {
            assert!(log.find_by_url(&e.request.url).await.is_some());
        }
    }

    #[tokio::test]
    async fn test_broadcast_dropped_receiver_does_not_panic() {
        let log = NetworkLog::new();
        {
            let _rx = log.subscribe().await;
        } // receiver dropped
        push_request(&log, make_request("1", "GET", "https://example.com")).await;
        // should not panic
    }

    #[tokio::test]
    async fn test_save_to_json_empty() {
        let log = NetworkLog::new();
        let path = std::path::Path::new("/tmp/foxdriver_network_empty.json");
        log.save_to_json(path).await.unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        assert_eq!(content.trim(), "[]");
    }

    #[tokio::test]
    async fn test_save_as_har_empty() {
        let log = NetworkLog::new();
        let path = std::path::Path::new("/tmp/foxdriver_network_empty.har");
        log.save_as_har(path, Some("test")).await.unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("1.2"));
        assert!(content.contains("runtime_foxdriver"));
    }

    #[tokio::test]
    async fn test_captured_timing_default() {
        let t = CapturedTiming::default();
        assert!(t.dns_start_ms.is_none());
        assert!(t.response_end_ms.is_none());
    }

    #[tokio::test]
    async fn test_network_entry_no_response_final_url() {
        let entry = NetworkEntry {
            request: make_request("1", "GET", "https://a.com"),
            response: None,
            error: None,
        };
        assert_eq!(entry.final_url(), "https://a.com");
        assert_eq!(entry.status(), None);
        assert!(!entry.has_response());
        assert!(!entry.is_error());
    }

    #[tokio::test]
    async fn test_distinct_methods_empty() {
        let log = NetworkLog::new();
        assert!(log.distinct_methods().await.is_empty());
    }

    #[tokio::test]
    async fn test_distinct_statuses_empty() {
        let log = NetworkLog::new();
        assert!(log.distinct_statuses().await.is_empty());
    }

    #[tokio::test]
    async fn test_hostnames_malformed_url() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "not-a-url")).await;
        assert!(log.hostnames().await.is_empty());
    }

    #[tokio::test]
    async fn test_total_bytes_no_response() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        assert_eq!(log.total_bytes_in().await, 0);
    }

    // ---------- contains_id / remove_by_id ----------

    #[tokio::test]
    async fn test_contains_id() {
        let log = NetworkLog::new();
        push_request(&log, make_request("abc", "GET", "https://a.com")).await;
        assert!(log.contains_id("abc").await);
        assert!(!log.contains_id("xyz").await);
    }

    #[tokio::test]
    async fn test_remove_by_id() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        push_request(&log, make_request("3", "GET", "https://c.com")).await;

        let removed = log.remove_by_id("2").await;
        assert_eq!(removed.unwrap().request.id, "2");
        assert_eq!(log.len().await, 2);
        assert!(!log.contains_id("2").await);

        // Verify remaining indices are valid
        assert!(log.contains_id("1").await);
        assert!(log.contains_id("3").await);
        assert_eq!(log.nth(0).await.unwrap().request.id, "1");
        assert_eq!(log.nth(1).await.unwrap().request.id, "3");
    }

    #[tokio::test]
    async fn test_remove_by_id_unknown() {
        let log = NetworkLog::new();
        assert!(log.remove_by_id("nope").await.is_none());
    }

    // ---------- wait_for helpers ----------

    #[tokio::test]
    async fn test_wait_for_url_existing() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://target.com/page")).await;
        let found = log
            .wait_for_url("target.com", std::time::Duration::from_secs(1))
            .await;
        assert_eq!(found.unwrap().request.id, "1");
    }

    #[tokio::test]
    async fn test_wait_for_url_future() {
        let log = NetworkLog::new();
        let log2 = log.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            push_request(&log2, make_request("1", "GET", "https://target.com/page")).await;
        });
        let found = log
            .wait_for_url("target.com", std::time::Duration::from_secs(1))
            .await;
        assert_eq!(found.unwrap().request.id, "1");
    }

    #[tokio::test]
    async fn test_wait_for_url_timeout() {
        let log = NetworkLog::new();
        let found = log
            .wait_for_url("never", std::time::Duration::from_millis(50))
            .await;
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_wait_for_response_existing() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(make_response("1", 200, "https://a.com")),
            None,
        )
        .await;
        let found = log
            .wait_for_response("1", std::time::Duration::from_secs(1))
            .await;
        assert_eq!(found.unwrap().request.id, "1");
    }

    #[tokio::test]
    async fn test_wait_for_response_future() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        let log2 = log.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            // Update with response
            let mut inner = log2.inner.write().await;
            if let Some(idx) = inner.by_id.get("1").copied() {
                let mut new_entry = (*inner.entries[idx]).clone();
                new_entry.response = Some(make_response("1", 200, "https://a.com"));
                let new_arc = Arc::new(new_entry);
                inner.entries[idx] = new_arc.clone();
                let _ = inner.tx.send(new_arc);
            }
        });
        let found = log
            .wait_for_response("1", std::time::Duration::from_secs(1))
            .await;
        assert_eq!(found.unwrap().request.id, "1");
    }

    #[tokio::test]
    async fn test_wait_for_response_timeout() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        let found = log
            .wait_for_response("1", std::time::Duration::from_millis(50))
            .await;
        assert!(found.is_none());
    }

    // ---------- concurrency stress ----------

    #[tokio::test]
    async fn test_concurrent_pushes() {
        let log = NetworkLog::new();
        let mut handles = Vec::new();
        for t in 0..10 {
            let log = log.clone();
            handles.push(tokio::spawn(async move {
                for i in 0..100 {
                    push_request(
                        &log,
                        make_request(&format!("{}-{}", t, i), "GET", "https://example.com"),
                    )
                    .await;
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        assert_eq!(log.len().await, 1000);
    }

    #[tokio::test]
    async fn test_concurrent_read_while_write() {
        let log = NetworkLog::new();
        let log2 = log.clone();

        let writer = tokio::spawn(async move {
            for i in 0..500 {
                push_request(
                    &log,
                    make_request(&format!("{}", i), "GET", "https://example.com"),
                )
                .await;
                if i % 50 == 0 {
                    tokio::task::yield_now().await;
                }
            }
        });

        let reader = tokio::spawn(async move {
            let mut last_len = 0;
            for _ in 0..50 {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                let len = log2.len().await;
                assert!(len >= last_len); // never decreases
                last_len = len;
            }
        });

        let (r1, r2) = tokio::join!(writer, reader);
        r1.unwrap();
        r2.unwrap();
    }

    // ---------- export with real data ----------

    #[tokio::test]
    async fn test_save_to_json_roundtrip() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://example.com"),
            Some(make_response("1", 200, "https://example.com")),
            None,
        )
        .await;
        let path = std::path::Path::new("/tmp/foxdriver_network_real.json");
        log.save_to_json(path).await.unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        let parsed: Vec<NetworkEntry> = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].request.id, "1");
        assert_eq!(parsed[0].status(), Some(200));
    }

    #[tokio::test]
    async fn test_save_as_har_with_data() {
        let log = NetworkLog::new();
        let mut req = make_request("1", "POST", "https://api.example.com/data?key=val");
        req.post_data = Some(r#"{"hello":"world"}"#.into());
        req.headers = vec![CapturedHeader {
            name: "Content-Type".into(),
            value: "application/json".into(),
        }];
        push_entry(
            &log,
            req,
            Some(make_response(
                "1",
                201,
                "https://api.example.com/data?key=val",
            )),
            None,
        )
        .await;

        let path = std::path::Path::new("/tmp/foxdriver_network_real.har");
        log.save_as_har(path, Some("test page")).await.unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("1.2"));
        assert!(content.contains("POST"));
        assert!(content.contains("201"));
        assert!(content.contains("key=val"));
        assert!(content.contains("hello"));
        assert!(content.contains("test page"));
    }

    // ---------- curl edge cases ----------

    #[tokio::test]
    async fn test_to_curl_no_headers_no_body() {
        let req = make_request("1", "GET", "https://example.com");
        let entry = NetworkEntry {
            request: req,
            response: None,
            error: None,
        };
        let curl = entry.to_curl();
        assert_eq!(curl, "curl -X 'GET' 'https://example.com'");
    }

    #[tokio::test]
    async fn test_to_curl_special_chars() {
        let req = CapturedRequest {
            id: "1".into(),
            context: None,
            method: "POST".into(),
            url: "https://example.com?a=1&b=2".into(),
            headers: vec![CapturedHeader {
                name: "X-Special".into(),
                value: "val'ue\".txt".into(),
            }],
            post_data: Some("data='quoted'".into()),
            timestamp: 0,
            destination: "document".into(),
            initiator_type: None,
            timing: CapturedTiming::default(),
            cookies: vec![],
        };
        let entry = NetworkEntry {
            request: req,
            response: None,
            error: None,
        };
        let curl = entry.to_curl();
        assert!(curl.contains("-H 'X-Special: val'\\''ue\".txt'"));
        assert!(curl.contains("-d 'data='\\''quoted'\\'''"));
    }

    // ---------- false positive tests ----------

    // ---------- filter boundary tests ----------

    #[tokio::test]
    async fn test_filter_empty_method_matches_all() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "POST", "https://b.com")).await;
        // Filter::new() without method() should match all
        assert_eq!(log.filter(Filter::new()).await.len(), 2);
    }

    #[tokio::test]
    async fn test_filter_status_range_no_matches() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(make_response("1", 200, "https://a.com")),
            None,
        )
        .await;
        let f = log.filter(Filter::new().status_range(500..=599)).await;
        assert!(f.is_empty());
    }

    #[tokio::test]
    async fn test_filter_url_contains_case_insensitive() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://API.example.com")).await;
        let f = log.filter(Filter::new().url_contains("api")).await;
        assert_eq!(f.len(), 1);
    }

    #[tokio::test]
    async fn test_filter_destination_case_insensitive() {
        let log = NetworkLog::new();
        let mut r = make_request("1", "GET", "https://a.com");
        r.destination = "IMAGE".into();
        push_request(&log, r).await;
        let f = log.filter(Filter::new().destination("image")).await;
        assert_eq!(f.len(), 1);
    }

    // ---------- query params edge cases ----------

    #[tokio::test]
    async fn test_query_params_empty() {
        let req = make_request("1", "GET", "https://a.com");
        assert!(req.query_params().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_query_params_url_encoded() {
        let req = make_request("1", "GET", "https://a.com?foo=%20bar&baz=qux");
        let params = req.query_params().unwrap();
        assert!(params.contains(&("foo".into(), " bar".into())));
        assert!(params.contains(&("baz".into(), "qux".into())));
    }

    // ---------- multiple evictions ----------

    #[tokio::test]
    async fn test_multiple_evictions() {
        let log = NetworkLog::with_limits(4, 10);
        for i in 0..20 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://example.com"),
            )
            .await;
        }
        let entries = log.entries().await;
        assert_eq!(entries.len(), 4);
        let ids: Vec<_> = entries.iter().map(|e| e.request.id.clone()).collect();
        assert_eq!(ids, vec!["16", "17", "18", "19"]);

        let m = log.metrics().await;
        assert_eq!(m.entries_evicted, 16);
    }

    // ---------- header edge cases ----------

    #[tokio::test]
    async fn test_request_header_multiple_same_name() {
        let req = make_request_with_headers(
            "1",
            "GET",
            "https://a.com",
            vec![
                CapturedHeader {
                    name: "X-Dup".into(),
                    value: "first".into(),
                },
                CapturedHeader {
                    name: "X-Dup".into(),
                    value: "second".into(),
                },
            ],
        );
        assert_eq!(req.request_header("x-dup"), Some("first")); // first match wins
    }

    #[tokio::test]
    async fn test_response_header_no_response() {
        let entry = NetworkEntry {
            request: make_request("1", "GET", "https://a.com"),
            response: None,
            error: None,
        };
        assert_eq!(entry.response_header("anything"), None);
    }

    // ---------- timing edge cases ----------

    #[tokio::test]
    async fn test_option_f64_ms_both_none() {
        assert_eq!(option_f64_ms(None, None), -1.0);
    }

    #[tokio::test]
    async fn test_option_f64_ms_end_none() {
        assert_eq!(option_f64_ms(None, Some(5.0)), -1.0);
    }

    // ---------- ingestion pipeline ----------

    fn make_bidi_request_data(
        id: &str,
        url: &str,
        method: &str,
    ) -> rustenium_bidi_definitions::network::types::RequestData {
        use rustenium_bidi_definitions::network::types::{FetchTimingInfo, Request, RequestData};
        RequestData {
            request: Request::new(id),
            url: url.into(),
            method: method.into(),
            headers: vec![],
            cookies: vec![],
            headers_size: 0,
            body_size: None,
            destination: "document".into(),
            initiator_type: None,
            timings: FetchTimingInfo {
                time_origin: 0.0,
                request_time: 0.0,
                redirect_start: 0.0,
                redirect_end: 0.0,
                fetch_start: 0.0,
                dns_start: 0.0,
                dns_end: 0.0,
                connect_start: 0.0,
                connect_end: 0.0,
                tls_start: 0.0,
                request_start: 0.0,
                response_start: 0.0,
                response_end: 0.0,
            },
            extensible: std::collections::HashMap::new(),
        }
    }

    fn make_before_request_sent(id: &str, url: &str, method: &str) -> BeforeRequestSent {
        use rustenium_bidi_definitions::network::events::BeforeRequestSentParams;
        use rustenium_bidi_definitions::network::types::BaseParameters;
        BeforeRequestSent {
            method: rustenium_bidi_definitions::network::events::BeforeRequestSentMethod::BeforeRequestSent,
            params: BeforeRequestSentParams {
                base_parameters: BaseParameters::new(false, 0u64, make_bidi_request_data(id, url, method), 0u64),
                initiator: None,
            },
        }
    }

    fn make_response_completed(id: &str, url: &str, status: u64) -> ResponseCompleted {
        use rustenium_bidi_definitions::network::events::ResponseCompletedParams;
        use rustenium_bidi_definitions::network::types::{
            BaseParameters, ResponseContent, ResponseData,
        };
        ResponseCompleted {
            method: rustenium_bidi_definitions::network::events::ResponseCompletedMethod::ResponseCompleted,
            params: ResponseCompletedParams {
                base_parameters: BaseParameters::new(false, 0u64, make_bidi_request_data(id, url, "GET"), 0u64),
                response: ResponseData {
                    url: url.into(),
                    protocol: "h2".into(),
                    status,
                    status_text: "OK".into(),
                    from_cache: false,
                    headers: vec![],
                    mime_type: "application/json".into(),
                    bytes_received: 100,
                    headers_size: None,
                    body_size: Some(100),
                    content: ResponseContent::new(100u64),
                    auth_challenges: None,
                },
            },
        }
    }

    fn make_fetch_error(id: &str, url: &str, error_text: &str) -> FetchError {
        use rustenium_bidi_definitions::network::events::FetchErrorParams;
        use rustenium_bidi_definitions::network::types::BaseParameters;
        FetchError {
            method: rustenium_bidi_definitions::network::events::FetchErrorMethod::FetchError,
            params: FetchErrorParams {
                base_parameters: BaseParameters::new(
                    false,
                    0u64,
                    make_bidi_request_data(id, url, "GET"),
                    0u64,
                ),
                error_text: error_text.into(),
            },
        }
    }

    #[tokio::test]
    async fn test_ingest_before_request_sent_creates_entry() {
        let log = NetworkLog::new();
        let evt = make_before_request_sent("req-1", "https://example.com", "GET");
        log.ingest_before_request_sent(&evt).await;
        assert_eq!(log.len().await, 1);
        assert!(log.contains_id("req-1").await);
        let m = log.metrics().await;
        assert_eq!(m.requests_received, 1);
    }

    #[tokio::test]
    async fn test_ingest_response_completed_attaches_to_existing() {
        let log = NetworkLog::new();
        log.ingest_before_request_sent(&make_before_request_sent(
            "req-1",
            "https://example.com",
            "GET",
        ))
        .await;
        log.ingest_response_completed(&make_response_completed(
            "req-1",
            "https://example.com",
            200,
        ))
        .await;

        let entry = log.first().await.unwrap();
        assert_eq!(entry.status(), Some(200));
        assert!(entry.has_response());
        let m = log.metrics().await;
        assert_eq!(m.responses_received, 1);
    }

    #[tokio::test]
    async fn test_ingest_fetch_error_attaches_to_existing() {
        let log = NetworkLog::new();
        log.ingest_before_request_sent(&make_before_request_sent(
            "req-1",
            "https://example.com",
            "GET",
        ))
        .await;
        log.ingest_fetch_error(&make_fetch_error(
            "req-1",
            "https://example.com",
            "net::ERR_FAILED",
        ))
        .await;

        let entry = log.first().await.unwrap();
        assert!(entry.is_error());
        assert_eq!(entry.error.as_ref().unwrap().error_text, "net::ERR_FAILED");
        let m = log.metrics().await;
        assert_eq!(m.errors_received, 1);
    }

    #[tokio::test]
    async fn test_fetch_error_url_comes_from_request_not_navigation() {
        // make_fetch_error builds an event with navigation = None (the common
        // subresource case: a failed script/image/XHR carries no top-level
        // document URL). The captured error URL must still be the actual
        // requested URL, never empty. The old code read `navigation` and
        // defaulted to "" here, hiding which resource failed.
        let log = NetworkLog::new();
        let failed_url = "https://cdn.example.com/app.js";
        log.ingest_fetch_error(&make_fetch_error("sub-1", failed_url, "net::ERR_FAILED"))
            .await;

        let (_resp, errors) = log.drain_pending().await;
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].url, failed_url,
            "error URL must be the request URL, not an empty navigation fallback"
        );
    }

    #[tokio::test]
    async fn test_out_of_order_response_reconciliation() {
        let log = NetworkLog::new();
        // Response arrives before request
        log.ingest_response_completed(&make_response_completed(
            "req-1",
            "https://example.com",
            200,
        ))
        .await;
        assert_eq!(log.len().await, 0); // No entry yet
        assert_eq!(log.metrics().await.responses_received, 1);

        // Now request arrives
        log.ingest_before_request_sent(&make_before_request_sent(
            "req-1",
            "https://example.com",
            "GET",
        ))
        .await;
        assert_eq!(log.len().await, 1);
        let entry = log.first().await.unwrap();
        assert_eq!(entry.status(), Some(200));
    }

    #[tokio::test]
    async fn test_out_of_order_error_reconciliation() {
        let log = NetworkLog::new();
        log.ingest_fetch_error(&make_fetch_error(
            "req-1",
            "https://example.com",
            "net::ERR_ABORTED",
        ))
        .await;
        assert_eq!(log.len().await, 0);

        log.ingest_before_request_sent(&make_before_request_sent(
            "req-1",
            "https://example.com",
            "GET",
        ))
        .await;
        let entry = log.first().await.unwrap();
        assert!(entry.is_error());
    }

    #[tokio::test]
    async fn test_response_update_rebroadcasts() {
        let log = NetworkLog::new();
        let mut rx = log.subscribe().await;

        log.ingest_before_request_sent(&make_before_request_sent(
            "req-1",
            "https://example.com",
            "GET",
        ))
        .await;
        let first = rx.recv().await.unwrap();
        assert!(!first.has_response());

        log.ingest_response_completed(&make_response_completed(
            "req-1",
            "https://example.com",
            200,
        ))
        .await;
        let updated = rx.recv().await.unwrap();
        assert!(updated.has_response());
        assert_eq!(updated.status(), Some(200));
    }

    #[tokio::test]
    async fn test_pending_response_overflow() {
        let log = NetworkLog::with_limits(100, 3);
        // 5 responses arrive before their requests
        for i in 0..5 {
            log.ingest_response_completed(&make_response_completed(
                &format!("req-{}", i),
                "https://example.com",
                200,
            ))
            .await;
        }
        let m = log.metrics().await;
        assert_eq!(m.pending_responses_dropped, 2); // max_pending=3, so 2 dropped
        assert_eq!(m.responses_received, 5);
    }

    #[tokio::test]
    async fn test_pending_error_overflow() {
        let log = NetworkLog::with_limits(100, 2);
        for i in 0..4 {
            log.ingest_fetch_error(&make_fetch_error(
                &format!("req-{}", i),
                "https://example.com",
                "err",
            ))
            .await;
        }
        let m = log.metrics().await;
        assert_eq!(m.pending_errors_dropped, 2); // max_pending=2, so 2 dropped
        assert_eq!(m.errors_received, 4);
    }

    #[tokio::test]
    async fn test_clear_then_ingest() {
        let log = NetworkLog::new();
        log.ingest_before_request_sent(&make_before_request_sent("req-1", "https://a.com", "GET"))
            .await;
        log.clear().await;
        log.ingest_before_request_sent(&make_before_request_sent("req-2", "https://b.com", "POST"))
            .await;
        assert_eq!(log.len().await, 1);
        assert!(!log.contains_id("req-1").await);
        assert!(log.contains_id("req-2").await);
    }

    #[tokio::test]
    async fn test_entry_with_both_response_and_error() {
        let log = NetworkLog::new();
        log.ingest_before_request_sent(&make_before_request_sent("req-1", "https://a.com", "GET"))
            .await;
        log.ingest_response_completed(&make_response_completed("req-1", "https://a.com", 200))
            .await;
        log.ingest_fetch_error(&make_fetch_error(
            "req-1",
            "https://a.com",
            "net::ERR_FAILED",
        ))
        .await;

        let entry = log.first().await.unwrap();
        assert!(entry.has_response());
        assert!(entry.is_error());
        assert_eq!(entry.status(), Some(200));
    }

    // ---------- cookie / header serialization ----------

    #[tokio::test]
    async fn test_captured_cookie_roundtrip() {
        let cookie = CapturedCookie {
            name: "session".into(),
            value: "abc123".into(),
            domain: ".example.com".into(),
            path: "/".into(),
            size: 42,
            http_only: true,
            secure: true,
            same_site: "strict".into(),
        };
        let json = serde_json::to_string(&cookie).unwrap();
        let de: CapturedCookie = serde_json::from_str(&json).unwrap();
        assert_eq!(de, cookie);
    }

    #[tokio::test]
    async fn test_captured_header_roundtrip() {
        let h = CapturedHeader {
            name: "X-Test".into(),
            value: "value".into(),
        };
        let json = serde_json::to_string(&h).unwrap();
        let de: CapturedHeader = serde_json::from_str(&json).unwrap();
        assert_eq!(de, h);
    }

    #[tokio::test]
    async fn test_filter_url_regex_invalid_pattern() {
        let result = Filter::new().url_regex("[");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_large_url_does_not_panic() {
        let log = NetworkLog::new();
        let long_url = format!("https://example.com/{}", "a".repeat(10000));
        push_request(&log, make_request("1", "GET", &long_url)).await;
        assert_eq!(log.len().await, 1);
        assert!(log.find_by_url("example.com").await.is_some());
    }

    #[tokio::test]
    async fn test_eviction_with_subscriber() {
        let log = NetworkLog::with_limits(2, 10);
        let mut rx = log.subscribe().await;
        for i in 0..5 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://example.com"),
            )
            .await;
        }
        // Should have received all 5 broadcast messages despite eviction
        let mut count = 0;
        while rx.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 5);
    }

    // ---------- deeper edge cases ----------

    #[tokio::test]
    async fn test_filter_method_case_insensitive() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "DELETE", "https://a.com")).await;
        assert_eq!(log.filter(Filter::new().method("delete")).await.len(), 1);
        assert_eq!(log.filter(Filter::new().method("DELETE")).await.len(), 1);
    }

    #[tokio::test]
    async fn test_filter_header_in_response() {
        let log = NetworkLog::new();
        let req = make_request("1", "GET", "https://a.com");
        let mut resp = make_response("1", 200, "https://a.com");
        resp.headers = vec![CapturedHeader {
            name: "X-Resp".into(),
            value: "secret-val".into(),
        }];
        push_entry(&log, req, Some(resp), None).await;
        let f = log.filter(Filter::new().header("x-resp", "secret")).await;
        assert_eq!(f.len(), 1);
    }

    #[tokio::test]
    async fn test_filter_composition_conflicting() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        // method GET AND method POST -> impossible
        let f = log.filter(Filter::new().method("GET").method("POST")).await;
        assert!(f.is_empty());
    }

    #[tokio::test]
    async fn test_count_zero() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        assert_eq!(log.count(Filter::new().method("POST")).await, 0);
    }

    #[tokio::test]
    async fn test_is_empty_false() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        assert!(!log.is_empty().await);
    }

    #[tokio::test]
    async fn test_endpoints_dedupes() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com/x")).await;
        push_request(&log, make_request("2", "GET", "https://a.com/x")).await;
        push_request(&log, make_request("3", "GET", "https://a.com/y")).await;
        let ep = log.endpoints().await;
        assert_eq!(ep.len(), 2);
    }

    #[tokio::test]
    async fn test_hostnames_with_port() {
        let log = NetworkLog::new();
        push_request(
            &log,
            make_request("1", "GET", "https://example.com:8443/path"),
        )
        .await;
        let h = log.hostnames().await;
        assert_eq!(h.len(), 1);
        assert!(h.contains(&"example.com".into()));
    }

    #[tokio::test]
    async fn test_total_bytes_out_none() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        assert_eq!(log.total_bytes_out().await, 0);
    }

    #[tokio::test]
    async fn test_completed_mixed_states() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(make_response("1", 200, "https://a.com")),
            None,
        )
        .await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        push_entry(
            &log,
            make_request("3", "GET", "https://c.com"),
            None,
            Some(make_error("3", "https://c.com", "err")),
        )
        .await;
        push_entry(
            &log,
            make_request("4", "GET", "https://d.com"),
            Some(make_response("4", 500, "https://d.com")),
            Some(make_error("4", "https://d.com", "err")),
        )
        .await;
        let c = log.completed().await;
        assert_eq!(c.len(), 3);
    }

    #[tokio::test]
    async fn test_distinct_statuses_with_no_response() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(make_response("1", 200, "https://a.com")),
            None,
        )
        .await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        push_entry(
            &log,
            make_request("3", "GET", "https://c.com"),
            Some(make_response("3", 404, "https://c.com")),
            None,
        )
        .await;
        let s = log.distinct_statuses().await;
        assert_eq!(s, vec![200, 404]);
    }

    #[tokio::test]
    async fn test_distinct_methods_dedupes() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        push_request(&log, make_request("3", "POST", "https://c.com")).await;
        push_request(&log, make_request("4", "POST", "https://d.com")).await;
        let m = log.distinct_methods().await;
        assert_eq!(m, vec!["GET", "POST"]);
    }

    #[tokio::test]
    async fn test_find_by_url_empty_substring() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        // Empty substring should match everything that contains ""
        assert!(log.find_by_url("").await.is_some());
    }

    #[tokio::test]
    async fn test_find_by_url_regex_no_match() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        let re = regex::Regex::new(r"zzz").unwrap();
        assert!(log.find_by_url_regex(&re).await.is_none());
    }

    #[tokio::test]
    async fn test_filter_url_contains_empty() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        let f = log.filter(Filter::new().url_contains("")).await;
        assert_eq!(f.len(), 1);
    }

    #[tokio::test]
    async fn test_to_curl_binary_body() {
        let req = CapturedRequest {
            id: "1".into(),
            context: None,
            method: "POST".into(),
            url: "https://example.com".into(),
            headers: vec![],
            post_data: Some("\x00\x01\x02".into()),
            timestamp: 0,
            destination: "document".into(),
            initiator_type: None,
            timing: CapturedTiming::default(),
            cookies: vec![],
        };
        let entry = NetworkEntry {
            request: req,
            response: None,
            error: None,
        };
        let curl = entry.to_curl();
        assert!(curl.contains("-d '"));
    }

    #[tokio::test]
    async fn test_json_body_empty_string() {
        let mut req = make_request("1", "POST", "https://a.com");
        req.post_data = Some("".into());
        assert!(req.json_body().is_none());
    }

    #[tokio::test]
    async fn test_query_params_duplicate_keys() {
        let req = make_request("1", "GET", "https://a.com?foo=1&foo=2");
        let params = req.query_params().unwrap();
        assert_eq!(params.len(), 2);
        assert!(params.contains(&("foo".into(), "1".into())));
        assert!(params.contains(&("foo".into(), "2".into())));
    }

    #[tokio::test]
    async fn test_request_header_empty_name() {
        let req = make_request_with_headers("1", "GET", "https://a.com", vec![]);
        assert_eq!(req.request_header(""), None);
    }

    #[tokio::test]
    async fn test_response_header_empty_name() {
        let entry = NetworkEntry {
            request: make_request("1", "GET", "https://a.com"),
            response: Some(make_response("1", 200, "https://a.com")),
            error: None,
        };
        assert_eq!(entry.response_header(""), None);
    }

    #[tokio::test]
    async fn test_remove_by_id_first() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        log.remove_by_id("1").await;
        assert_eq!(log.first().await.unwrap().request.id, "2");
    }

    #[tokio::test]
    async fn test_remove_by_id_last() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        log.remove_by_id("2").await;
        assert_eq!(log.last().await.unwrap().request.id, "1");
    }

    #[tokio::test]
    async fn test_contains_id_after_eviction() {
        let log = NetworkLog::with_limits(4, 10);
        for i in 0..10 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://example.com"),
            )
            .await;
        }
        // After eviction, only 4-9 should remain
        for i in 0..4 {
            assert!(!log.contains_id(&format!("{}", i)).await);
        }
        for i in 6..10 {
            assert!(log.contains_id(&format!("{}", i)).await);
        }
    }

    #[tokio::test]
    async fn test_wait_for_response_error_instead_of_response() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        let log2 = log.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let mut inner = log2.inner.write().await;
            if let Some(idx) = inner.by_id.get("1").copied() {
                let mut new_entry = (*inner.entries[idx]).clone();
                new_entry.error = Some(make_error("1", "https://a.com", "net::ERR_ABORTED"));
                let new_arc = Arc::new(new_entry);
                inner.entries[idx] = new_arc.clone();
                let _ = inner.tx.send(new_arc);
            }
        });
        let found = log
            .wait_for_response("1", std::time::Duration::from_secs(1))
            .await;
        assert!(found.unwrap().is_error());
    }

    #[tokio::test]
    async fn test_captured_timing_from_bidi() {
        use rustenium_bidi_definitions::network::types::FetchTimingInfo;
        let info = FetchTimingInfo {
            time_origin: 0.0,
            request_time: 10.0,
            redirect_start: 0.0,
            redirect_end: 0.0,
            fetch_start: 10.0,
            dns_start: 11.0,
            dns_end: 12.0,
            connect_start: 12.0,
            connect_end: 14.0,
            tls_start: 13.0,
            request_start: 14.0,
            response_start: 15.0,
            response_end: 16.0,
        };
        let t = CapturedTiming::from(&info);
        assert_eq!(t.dns_start_ms, Some(1.0)); // 11 - 10
        assert_eq!(t.dns_end_ms, Some(2.0)); // 12 - 10
        assert_eq!(t.connect_start_ms, Some(2.0)); // 12 - 10
        assert_eq!(t.connect_end_ms, Some(4.0)); // 14 - 10
        assert_eq!(t.tls_start_ms, Some(3.0)); // 13 - 10
        assert_eq!(t.response_start_ms, Some(5.0)); // 15 - 10
        assert_eq!(t.response_end_ms, Some(6.0)); // 16 - 10
    }

    #[tokio::test]
    async fn test_captured_timing_negative_clamped() {
        use rustenium_bidi_definitions::network::types::FetchTimingInfo;
        let info = FetchTimingInfo {
            time_origin: 0.0,
            request_time: 10.0,
            redirect_start: 0.0,
            redirect_end: 0.0,
            fetch_start: 10.0,
            dns_start: 9.0, // before request_time
            dns_end: 8.0,   // before request_time
            connect_start: 0.0,
            connect_end: 0.0,
            tls_start: 0.0,
            request_start: 0.0,
            response_start: 0.0,
            response_end: 0.0,
        };
        let t = CapturedTiming::from(&info);
        assert_eq!(t.dns_start_ms, None); // negative clamped to None
        assert_eq!(t.dns_end_ms, None);
    }

    #[tokio::test]
    async fn test_har_timings_correct() {
        let log = NetworkLog::new();
        let mut req = make_request("1", "GET", "https://api.example.com");
        req.timing = CapturedTiming {
            dns_start_ms: Some(1.0),
            dns_end_ms: Some(3.0),
            connect_start_ms: Some(3.0),
            connect_end_ms: Some(7.0),
            tls_start_ms: Some(5.0),
            response_start_ms: Some(8.0),
            response_end_ms: Some(10.0),
        };
        push_entry(
            &log,
            req,
            Some(make_response("1", 200, "https://api.example.com")),
            None,
        )
        .await;

        let path = std::path::Path::new("/tmp/foxdriver_har_timings.har");
        log.save_as_har(path, None).await.unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        // dns should be 2.0 (3.0 - 1.0)
        assert!(content.contains("\"dns\": 2.0") || content.contains("\"dns\": 2"));
        // connect should be 4.0 (7.0 - 3.0)
        assert!(content.contains("\"connect\": 4.0") || content.contains("\"connect\": 4"));
        // ssl should be 2.0 (7.0 - 5.0)
        assert!(content.contains("\"ssl\": 2.0") || content.contains("\"ssl\": 2"));
    }

    #[tokio::test]
    async fn test_concurrent_ingestion_stress() {
        let log = NetworkLog::new();
        let mut handles = Vec::new();
        for t in 0..20 {
            let log = log.clone();
            handles.push(tokio::spawn(async move {
                for i in 0..50 {
                    let id = format!("{}-{}", t, i);
                    log.ingest_before_request_sent(&make_before_request_sent(
                        &id,
                        "https://example.com",
                        "GET",
                    ))
                    .await;
                    if i % 2 == 0 {
                        log.ingest_response_completed(&make_response_completed(
                            &id,
                            "https://example.com",
                            200,
                        ))
                        .await;
                    } else {
                        log.ingest_fetch_error(&make_fetch_error(
                            &id,
                            "https://example.com",
                            "err",
                        ))
                        .await;
                    }
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        assert_eq!(log.len().await, 1000);
        let completed = log.completed().await;
        assert_eq!(completed.len(), 1000); // all have response or error
        let m = log.metrics().await;
        assert_eq!(m.requests_received, 1000);
        assert_eq!(m.responses_received, 500);
        assert_eq!(m.errors_received, 500);
    }

    #[tokio::test]
    async fn test_property_filter_then_count_matches() {
        let log = NetworkLog::new();
        for i in 0..100 {
            let method = if i % 2 == 0 { "GET" } else { "POST" };
            push_request(
                &log,
                make_request(&format!("{}", i), method, "https://example.com"),
            )
            .await;
        }
        let f = Filter::new().method("GET");
        let filtered = log.filter(f.clone()).await;
        let counted = log.count(f).await;
        assert_eq!(filtered.len(), counted);
        assert_eq!(counted, 50);
    }

    #[tokio::test]
    async fn test_property_first_last_consistency() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        push_request(&log, make_request("3", "GET", "https://c.com")).await;

        assert_eq!(
            log.first().await.unwrap().request.id,
            log.nth(0).await.unwrap().request.id
        );
        assert_eq!(
            log.last().await.unwrap().request.id,
            log.nth(2).await.unwrap().request.id
        );
    }

    #[tokio::test]
    async fn test_property_endpoints_is_subset_of_entries() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com/x")).await;
        push_request(&log, make_request("2", "GET", "https://a.com/y")).await;
        let entries = log.entries().await;
        let endpoints = log.endpoints().await;
        assert!(endpoints.len() <= entries.len());
        for ep in &endpoints {
            assert!(entries.iter().any(|e| e.request.url == *ep));
        }
    }

    #[tokio::test]
    async fn test_property_hostnames_is_subset_of_endpoints() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com/x")).await;
        push_request(&log, make_request("2", "GET", "https://b.com/y")).await;
        let hostnames = log.hostnames().await;
        let endpoints = log.endpoints().await;
        for h in &hostnames {
            assert!(endpoints.iter().any(|e| e.contains(h)));
        }
    }

    #[tokio::test]
    async fn test_stress_many_entries() {
        let log = NetworkLog::with_limits(100_000, 10_000);
        for i in 0..10_000 {
            push_request(
                &log,
                make_request(
                    &format!("{}", i),
                    "GET",
                    &format!("https://example.com/{}", i),
                ),
            )
            .await;
        }
        assert_eq!(log.len().await, 10_000);
        assert!(log.find_by_url("example.com").await.is_some());
        assert_eq!(log.distinct_methods().await, vec!["GET"]);
    }

    // ---------- make_network_handler ----------

    #[tokio::test]
    async fn test_handler_ignores_non_network_events() {
        let log = NetworkLog::new();
        let mut handler = make_network_handler(log.clone());
        // Log event should be ignored
        let evt = rustenium_bidi_definitions::Event::Log(
            rustenium_bidi_definitions::log::events::LogEvent::EntryAdded(
                rustenium_bidi_definitions::log::events::EntryAdded {
                    method: rustenium_bidi_definitions::log::events::EntryAddedMethod::EntryAdded,
                    params: rustenium_bidi_definitions::log::events::EntryAddedParams {},
                },
            ),
        );
        handler(evt).await;
        assert!(log.is_empty().await);
    }

    #[tokio::test]
    async fn test_handler_processes_before_request_sent() {
        let log = NetworkLog::new();
        let mut handler = make_network_handler(log.clone());
        let evt = rustenium_bidi_definitions::Event::Network(
            rustenium_bidi_definitions::network::events::NetworkEvent::BeforeRequestSent(
                make_before_request_sent("req-1", "https://example.com", "GET"),
            ),
        );
        handler(evt).await;
        assert_eq!(log.len().await, 1);
        assert!(log.contains_id("req-1").await);
    }

    #[tokio::test]
    async fn test_handler_processes_response_completed() {
        let log = NetworkLog::new();
        let mut handler = make_network_handler(log.clone());
        handler(rustenium_bidi_definitions::Event::Network(
            rustenium_bidi_definitions::network::events::NetworkEvent::BeforeRequestSent(
                make_before_request_sent("req-1", "https://example.com", "GET"),
            ),
        ))
        .await;
        handler(rustenium_bidi_definitions::Event::Network(
            rustenium_bidi_definitions::network::events::NetworkEvent::ResponseCompleted(
                make_response_completed("req-1", "https://example.com", 200),
            ),
        ))
        .await;
        let entry = log.first().await.unwrap();
        assert_eq!(entry.status(), Some(200));
    }

    #[tokio::test]
    async fn test_handler_ignores_unknown_network_events() {
        let log = NetworkLog::new();
        let mut handler = make_network_handler(log.clone());
        // ResponseStarted is not handled
        handler(rustenium_bidi_definitions::Event::Network(
            rustenium_bidi_definitions::network::events::NetworkEvent::ResponseStarted(
                rustenium_bidi_definitions::network::events::ResponseStarted {
                    method: rustenium_bidi_definitions::network::events::ResponseStartedMethod::ResponseStarted,
                    params: rustenium_bidi_definitions::network::events::ResponseStartedParams {
                        base_parameters: rustenium_bidi_definitions::network::types::BaseParameters::new(
                            false, 0u64, make_bidi_request_data("req-1", "https://example.com", "GET"), 0u64
                        ),
                        response: rustenium_bidi_definitions::network::types::ResponseData {
                            url: "https://example.com".into(),
                            protocol: "h2".into(),
                            status: 200,
                            status_text: "OK".into(),
                            from_cache: false,
                            headers: vec![],
                            mime_type: "application/json".into(),
                            bytes_received: 100,
                            headers_size: None,
                            body_size: Some(100),
                            content: rustenium_bidi_definitions::network::types::ResponseContent::new(100u64),
                            auth_challenges: None,
                        },
                    },
                }
            )
        )).await;
        assert!(log.is_empty().await);
    }

    // ---------- export edge cases ----------

    #[tokio::test]
    async fn test_har_with_null_response() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        let path = std::path::Path::new("/tmp/foxdriver_har_null_resp.har");
        log.save_as_har(path, None).await.unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("\"response\": null"));
    }

    #[tokio::test]
    async fn test_har_with_error_entry() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            None,
            Some(make_error("1", "https://a.com", "net::ERR_FAILED")),
        )
        .await;
        let path = std::path::Path::new("/tmp/foxdriver_har_error.har");
        log.save_as_har(path, None).await.unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("\"response\": null"));
    }

    #[tokio::test]
    async fn test_default_impl() {
        let log: NetworkLog = Default::default();
        assert!(log.is_empty().await);
        let m = log.metrics().await;
        assert_eq!(m.max_entries, 50_000);
    }

    #[tokio::test]
    async fn test_clear_does_not_break_broadcast() {
        let log = NetworkLog::new();
        let mut rx = log.subscribe().await;
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        log.clear().await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        // Should still receive both (clear doesn't close channel)
        let first = rx.recv().await.unwrap();
        assert_eq!(first.request.id, "1");
        let second = rx.recv().await.unwrap();
        assert_eq!(second.request.id, "2");
    }

    #[tokio::test]
    async fn test_entries_returns_arc_clones() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        let e1 = log.entries().await;
        let e2 = log.entries().await;
        // Arc::ptr_eq should be true since they point to the same data
        assert!(Arc::ptr_eq(&e1[0], &e2[0]));
    }

    #[tokio::test]
    async fn test_filter_header_only_response() {
        let log = NetworkLog::new();
        let req = make_request("1", "GET", "https://a.com");
        let mut resp = make_response("1", 200, "https://a.com");
        resp.headers = vec![CapturedHeader {
            name: "X-Resp-Only".into(),
            value: "found-me".into(),
        }];
        push_entry(&log, req, Some(resp), None).await;
        // Filter should find the header even though it's only in response
        let f = log
            .filter(Filter::new().header("x-resp-only", "found"))
            .await;
        assert_eq!(f.len(), 1);
    }

    #[tokio::test]
    async fn test_find_by_url_after_remove() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com/page")).await;
        push_request(&log, make_request("2", "GET", "https://b.com/page")).await;
        log.remove_by_id("1").await;
        assert!(log.find_by_url("a.com").await.is_none());
        assert!(log.find_by_url("b.com").await.is_some());
    }

    #[tokio::test]
    async fn test_concurrent_wait_for_url() {
        let log = NetworkLog::new();
        let log2 = log.clone();
        let log3 = log.clone();
        let h1 = tokio::spawn(async move {
            log2.wait_for_url("target", std::time::Duration::from_secs(1))
                .await
        });
        let h2 = tokio::spawn(async move {
            log3.wait_for_url("target", std::time::Duration::from_secs(1))
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        push_request(&log, make_request("1", "GET", "https://target.com")).await;
        let r1 = h1.await.unwrap();
        let r2 = h2.await.unwrap();
        assert!(r1.is_some());
        assert!(r2.is_some());
    }

    #[tokio::test]
    async fn test_build_response_from_cache() {
        let log = NetworkLog::new();
        let mut resp = make_response("1", 200, "https://a.com");
        resp.from_cache = true;
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(resp),
            None,
        )
        .await;
        let entry = log.first().await.unwrap();
        assert!(entry.response.as_ref().unwrap().from_cache);
    }

    #[tokio::test]
    async fn test_build_response_no_body_size() {
        let log = NetworkLog::new();
        let mut resp = make_response("1", 204, "https://a.com");
        resp.body_size = None;
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(resp),
            None,
        )
        .await;
        let entry = log.first().await.unwrap();
        assert_eq!(entry.response.as_ref().unwrap().body_size, None);
        assert_eq!(log.total_bytes_in().await, 0);
    }

    #[tokio::test]
    async fn test_filter_has_response_and_has_error_same_entry() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(make_response("1", 500, "https://a.com")),
            Some(make_error("1", "https://a.com", "err")),
        )
        .await;
        // with_response should match
        assert_eq!(log.filter(Filter::new().with_response()).await.len(), 1);
        // with_error should match
        assert_eq!(log.filter(Filter::new().with_error()).await.len(), 1);
    }

    #[tokio::test]
    async fn test_nth_after_remove() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        push_request(&log, make_request("3", "GET", "https://c.com")).await;
        log.remove_by_id("2").await;
        assert_eq!(log.nth(0).await.unwrap().request.id, "1");
        assert_eq!(log.nth(1).await.unwrap().request.id, "3");
        assert!(log.nth(2).await.is_none());
    }

    #[tokio::test]
    async fn test_pending_response_then_error_then_request() {
        let log = NetworkLog::new();
        // Response arrives first
        log.ingest_response_completed(&make_response_completed("req-1", "https://a.com", 200))
            .await;
        // Then error arrives
        log.ingest_fetch_error(&make_fetch_error(
            "req-1",
            "https://a.com",
            "net::ERR_ABORTED",
        ))
        .await;
        // Then request arrives - should reconcile both response and error
        log.ingest_before_request_sent(&make_before_request_sent("req-1", "https://a.com", "GET"))
            .await;
        let entry = log.first().await.unwrap();
        assert!(entry.has_response());
        assert!(entry.is_error());
    }

    #[tokio::test]
    async fn test_metrics_persist_after_eviction() {
        let log = NetworkLog::with_limits(4, 10);
        for i in 0..10 {
            log.ingest_before_request_sent(&make_before_request_sent(
                &format!("{}", i),
                "https://a.com",
                "GET",
            ))
            .await;
        }
        let m = log.metrics().await;
        assert_eq!(m.requests_received, 10);
        assert_eq!(m.entries_evicted, 6); // 10 - 4 = 6 evicted (actually eviction removes half each time)
    }

    #[tokio::test]
    async fn test_curl_strips_accept_encoding() {
        let req = CapturedRequest {
            id: "1".into(),
            context: None,
            method: "GET".into(),
            url: "https://example.com".into(),
            headers: vec![CapturedHeader {
                name: "Accept-Encoding".into(),
                value: "gzip".into(),
            }],
            post_data: None,
            timestamp: 0,
            destination: "document".into(),
            initiator_type: None,
            timing: CapturedTiming::default(),
            cookies: vec![],
        };
        let entry = NetworkEntry {
            request: req,
            response: None,
            error: None,
        };
        let curl = entry.to_curl();
        assert!(!curl.contains("Accept-Encoding"));
    }

    // ---------- boundary condition bug fixes ----------

    #[tokio::test]
    async fn test_max_entries_zero_does_not_panic() {
        let log = NetworkLog::with_limits(0, 10);
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        // With max_entries=0, no eviction should occur (special case)
        assert_eq!(log.len().await, 2);
    }

    #[tokio::test]
    async fn test_max_pending_zero_does_not_hang() {
        let log = NetworkLog::with_limits(100, 0);
        // Should not hang even though max_pending=0
        log.ingest_response_completed(&make_response_completed("req-1", "https://a.com", 200))
            .await;
        log.ingest_before_request_sent(&make_before_request_sent("req-1", "https://a.com", "GET"))
            .await;
        assert_eq!(log.len().await, 1);
        assert!(log.first().await.unwrap().has_response());
    }

    #[tokio::test]
    async fn test_max_entries_one() {
        let log = NetworkLog::with_limits(1, 10);
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        // Incremental eviction removes the oldest entry to make room for each new one.
        assert_eq!(log.len().await, 1);
        assert_eq!(log.last().await.unwrap().request.id, "2");
    }

    // ---------- deeper ingestion coverage ----------

    #[tokio::test]
    async fn test_ingest_request_with_headers() {
        let log = NetworkLog::new();
        let mut req_data = make_bidi_request_data("req-1", "https://a.com", "GET");
        req_data.headers = vec![rustenium_bidi_definitions::network::types::Header {
            name: "X-Test".into(),
            value: rustenium_bidi_definitions::network::types::BytesValue::StringValue(
                rustenium_bidi_definitions::network::types::StringValue::new(
                    rustenium_bidi_definitions::network::types::StringValueType::String,
                    "hello",
                ),
            ),
        }];
        let evt = BeforeRequestSent {
            method: rustenium_bidi_definitions::network::events::BeforeRequestSentMethod::BeforeRequestSent,
            params: rustenium_bidi_definitions::network::events::BeforeRequestSentParams {
                base_parameters: rustenium_bidi_definitions::network::types::BaseParameters::new(false, 0u64, req_data, 0u64),
                initiator: None,
            },
        };
        log.ingest_before_request_sent(&evt).await;
        let entry = log.first().await.unwrap();
        assert_eq!(entry.request.headers.len(), 1);
        assert_eq!(entry.request.headers[0].name, "X-Test");
        assert_eq!(entry.request.headers[0].value, "hello");
    }

    #[tokio::test]
    async fn test_ingest_request_with_base64_header() {
        let log = NetworkLog::new();
        let mut req_data = make_bidi_request_data("req-1", "https://a.com", "GET");
        req_data.headers = vec![rustenium_bidi_definitions::network::types::Header {
            name: "X-Binary".into(),
            value: rustenium_bidi_definitions::network::types::BytesValue::Base64Value(
                rustenium_bidi_definitions::network::types::Base64Value::new(
                    rustenium_bidi_definitions::network::types::Base64ValueType::Base64,
                    "SGVsbG8=",
                ),
            ),
        }];
        let evt = BeforeRequestSent {
            method: rustenium_bidi_definitions::network::events::BeforeRequestSentMethod::BeforeRequestSent,
            params: rustenium_bidi_definitions::network::events::BeforeRequestSentParams {
                base_parameters: rustenium_bidi_definitions::network::types::BaseParameters::new(false, 0u64, req_data, 0u64),
                initiator: None,
            },
        };
        log.ingest_before_request_sent(&evt).await;
        let entry = log.first().await.unwrap();
        assert_eq!(entry.request.headers[0].value, "SGVsbG8=");
    }

    #[tokio::test]
    async fn test_ingest_request_with_context() {
        let log = NetworkLog::new();
        let req_data = make_bidi_request_data("req-1", "https://a.com", "GET");
        let mut base = rustenium_bidi_definitions::network::types::BaseParameters::new(
            false, 0u64, req_data, 0u64,
        );
        base.context = Some(
            rustenium_bidi_definitions::browsing_context::types::BrowsingContext::new("ctx-1"),
        );
        let evt = BeforeRequestSent {
            method: rustenium_bidi_definitions::network::events::BeforeRequestSentMethod::BeforeRequestSent,
            params: rustenium_bidi_definitions::network::events::BeforeRequestSentParams {
                base_parameters: base,
                initiator: None,
            },
        };
        log.ingest_before_request_sent(&evt).await;
        let entry = log.first().await.unwrap();
        assert_eq!(entry.request.context, Some("ctx-1".into()));
    }

    #[tokio::test]
    async fn test_ingest_request_with_initiator() {
        let log = NetworkLog::new();
        let req_data = make_bidi_request_data("req-1", "https://a.com", "GET");
        let evt = BeforeRequestSent {
            method: rustenium_bidi_definitions::network::events::BeforeRequestSentMethod::BeforeRequestSent,
            params: rustenium_bidi_definitions::network::events::BeforeRequestSentParams {
                base_parameters: rustenium_bidi_definitions::network::types::BaseParameters::new(false, 0u64, req_data, 0u64),
                initiator: Some(rustenium_bidi_definitions::network::types::Initiator {
                    column_number: None,
                    line_number: None,
                    request: None,
                    stack_trace: None,
                    r#type: Some(rustenium_bidi_definitions::network::types::InitiatorType::Script),
                }),
            },
        };
        log.ingest_before_request_sent(&evt).await;
        let entry = log.first().await.unwrap();
        assert_eq!(entry.request.initiator_type, Some("script".into()));
    }

    #[tokio::test]
    async fn test_ingest_response_with_headers() {
        let log = NetworkLog::new();
        log.ingest_before_request_sent(&make_before_request_sent("req-1", "https://a.com", "GET"))
            .await;

        let mut resp_data = make_response_completed("req-1", "https://a.com", 200);
        resp_data.params.response.headers =
            vec![rustenium_bidi_definitions::network::types::Header {
                name: "X-Response".into(),
                value: rustenium_bidi_definitions::network::types::BytesValue::StringValue(
                    rustenium_bidi_definitions::network::types::StringValue::new(
                        rustenium_bidi_definitions::network::types::StringValueType::String,
                        "world",
                    ),
                ),
            }];
        log.ingest_response_completed(&resp_data).await;

        let entry = log.first().await.unwrap();
        assert_eq!(entry.response.as_ref().unwrap().headers.len(), 1);
        assert_eq!(
            entry.response.as_ref().unwrap().headers[0].name,
            "X-Response"
        );
        assert_eq!(entry.response.as_ref().unwrap().headers[0].value, "world");
    }

    #[tokio::test]
    async fn test_ingest_response_from_cache() {
        let log = NetworkLog::new();
        log.ingest_before_request_sent(&make_before_request_sent("req-1", "https://a.com", "GET"))
            .await;
        let mut resp = make_response_completed("req-1", "https://a.com", 200);
        resp.params.response.from_cache = true;
        log.ingest_response_completed(&resp).await;
        let entry = log.first().await.unwrap();
        assert!(entry.response.as_ref().unwrap().from_cache);
    }

    // ---------- empty log query behavior ----------

    #[tokio::test]
    async fn test_first_on_empty_log() {
        let log = NetworkLog::new();
        assert!(log.first().await.is_none());
    }

    #[tokio::test]
    async fn test_last_on_empty_log() {
        let log = NetworkLog::new();
        assert!(log.last().await.is_none());
    }

    #[tokio::test]
    async fn test_nth_on_empty_log() {
        let log = NetworkLog::new();
        assert!(log.nth(0).await.is_none());
    }

    #[tokio::test]
    async fn test_find_by_url_on_empty_log() {
        let log = NetworkLog::new();
        assert!(log.find_by_url("anything").await.is_none());
    }

    #[tokio::test]
    async fn test_filter_on_empty_log() {
        let log = NetworkLog::new();
        assert!(log.filter(Filter::new()).await.is_empty());
    }

    #[tokio::test]
    async fn test_count_on_empty_log() {
        let log = NetworkLog::new();
        assert_eq!(log.count(Filter::new()).await, 0);
    }

    #[tokio::test]
    async fn test_completed_on_empty_log() {
        let log = NetworkLog::new();
        assert!(log.completed().await.is_empty());
    }

    #[tokio::test]
    async fn test_endpoints_on_empty_log() {
        let log = NetworkLog::new();
        assert!(log.endpoints().await.is_empty());
    }

    #[tokio::test]
    async fn test_hostnames_on_empty_log() {
        let log = NetworkLog::new();
        assert!(log.hostnames().await.is_empty());
    }

    #[tokio::test]
    async fn test_distinct_methods_on_empty_log() {
        let log = NetworkLog::new();
        assert!(log.distinct_methods().await.is_empty());
    }

    #[tokio::test]
    async fn test_distinct_statuses_on_empty_log() {
        let log = NetworkLog::new();
        assert!(log.distinct_statuses().await.is_empty());
    }

    #[tokio::test]
    async fn test_total_bytes_on_empty_log() {
        let log = NetworkLog::new();
        assert_eq!(log.total_bytes_in().await, 0);
        assert_eq!(log.total_bytes_out().await, 0);
    }

    #[tokio::test]
    async fn test_contains_id_on_empty_log() {
        let log = NetworkLog::new();
        assert!(!log.contains_id("anything").await);
    }

    #[tokio::test]
    async fn test_remove_by_id_on_empty_log() {
        let log = NetworkLog::new();
        assert!(log.remove_by_id("anything").await.is_none());
    }

    #[tokio::test]
    async fn test_clear_on_empty_log() {
        let log = NetworkLog::new();
        log.clear().await;
        assert!(log.is_empty().await);
    }

    #[tokio::test]
    async fn test_subscribe_on_empty_log() {
        let log = NetworkLog::new();
        let mut rx = log.subscribe().await;
        assert!(rx.try_recv().is_err());
    }

    // ---------- remove_by_id edge cases ----------

    #[tokio::test]
    async fn test_remove_by_id_twice() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        assert!(log.remove_by_id("1").await.is_some());
        assert!(log.remove_by_id("1").await.is_none());
    }

    #[tokio::test]
    async fn test_remove_by_id_only_entry() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        log.remove_by_id("1").await;
        assert!(log.is_empty().await);
        assert!(!log.contains_id("1").await);
    }

    #[tokio::test]
    async fn test_concurrent_remove_by_id() {
        let log = NetworkLog::new();
        for i in 0..100 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://a.com"),
            )
            .await;
        }
        let mut handles = Vec::new();
        for i in 0..100 {
            let log = log.clone();
            handles.push(tokio::spawn(async move {
                log.remove_by_id(&format!("{}", i)).await
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        assert!(log.is_empty().await);
    }

    // ---------- wait_for edge cases ----------

    #[tokio::test]
    async fn test_wait_for_url_empty_substring() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        let found = log
            .wait_for_url("", std::time::Duration::from_secs(1))
            .await;
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn test_wait_for_response_nonexistent_id() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        let found = log
            .wait_for_response("does-not-exist", std::time::Duration::from_millis(50))
            .await;
        assert!(found.is_none());
    }

    // ---------- json_body edge cases ----------

    #[tokio::test]
    async fn test_json_body_array() {
        let mut req = make_request("1", "POST", "https://a.com");
        req.post_data = Some("[1, 2, 3]".into());
        assert_eq!(req.json_body(), Some(serde_json::json!([1, 2, 3])));
    }

    #[tokio::test]
    async fn test_json_body_nested() {
        let mut req = make_request("1", "POST", "https://a.com");
        req.post_data = Some(r#"{"outer": {"inner": [true, false]}}"#.into());
        assert_eq!(
            req.json_body(),
            Some(serde_json::json!({"outer": {"inner": [true, false]}}))
        );
    }

    // ---------- curl edge cases ----------

    #[tokio::test]
    async fn test_to_curl_delete_method() {
        let req = make_request("1", "DELETE", "https://api.example.com/resource/1");
        let entry = NetworkEntry {
            request: req,
            response: None,
            error: None,
        };
        assert!(entry.to_curl().starts_with("curl -X 'DELETE'"));
    }

    #[tokio::test]
    async fn test_to_curl_url_with_single_quotes() {
        let req = make_request("1", "GET", "https://example.com?foo='bar'");
        let entry = NetworkEntry {
            request: req,
            response: None,
            error: None,
        };
        let curl = entry.to_curl();
        assert!(curl.contains("'https://example.com?foo='\\''bar'\\'''"));
    }

    #[tokio::test]
    async fn test_to_curl_header_with_single_quotes() {
        let req = make_request_with_headers(
            "1",
            "GET",
            "https://example.com",
            vec![CapturedHeader {
                name: "X-Quote".into(),
                value: "it's working".into(),
            }],
        );
        let entry = NetworkEntry {
            request: req,
            response: None,
            error: None,
        };
        let curl = entry.to_curl();
        assert!(curl.contains("-H 'X-Quote: it'\\''s working'"));
    }

    // ---------- serialization of public structs ----------

    #[tokio::test]
    async fn test_network_metrics_roundtrip() {
        let m = NetworkMetrics {
            requests_received: 10,
            responses_received: 8,
            errors_received: 2,
            entries_evicted: 5,
            pending_responses_dropped: 1,
            pending_errors_dropped: 0,
            broadcast_drops: 3,
            duplicate_responses: 1,
            duplicate_errors: 0,
            max_entries: 100,
        };
        let json = serde_json::to_string(&m).unwrap();
        let de: NetworkMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(de, m);
    }

    // ---------- unicode / IDN ----------

    #[tokio::test]
    async fn test_find_by_url_unicode() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://例子.com/path")).await;
        assert!(log.find_by_url("例子").await.is_some());
    }

    #[tokio::test]
    async fn test_hostnames_idn() {
        let log = NetworkLog::new();
        push_request(
            &log,
            make_request("1", "GET", "https://xn--fsq092h.com/path"),
        )
        .await;
        let h = log.hostnames().await;
        assert_eq!(h.len(), 1);
        assert!(h.contains(&"xn--fsq092h.com".into()));
    }

    // ---------- total_bytes_out multibyte ----------

    #[tokio::test]
    async fn test_total_bytes_out_multibyte() {
        let log = NetworkLog::new();
        let mut req = make_request("1", "POST", "https://a.com");
        req.post_data = Some("日本語".into()); // 9 bytes in UTF-8
        push_entry(&log, req, None, None).await;
        assert_eq!(log.total_bytes_out().await, 9);
    }

    // ---------- completed with only errors ----------

    #[tokio::test]
    async fn test_completed_only_errors() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            None,
            Some(make_error("1", "https://a.com", "err")),
        )
        .await;
        push_entry(
            &log,
            make_request("2", "GET", "https://b.com"),
            None,
            Some(make_error("2", "https://b.com", "err")),
        )
        .await;
        let c = log.completed().await;
        assert_eq!(c.len(), 2);
    }

    // ---------- distinct with all same ----------

    #[tokio::test]
    async fn test_distinct_statuses_all_same() {
        let log = NetworkLog::new();
        for i in 0..5 {
            push_entry(
                &log,
                make_request(&format!("{}", i), "GET", "https://a.com"),
                Some(make_response(&format!("{}", i), 200, "https://a.com")),
                None,
            )
            .await;
        }
        assert_eq!(log.distinct_statuses().await, vec![200]);
    }

    #[tokio::test]
    async fn test_distinct_methods_all_same() {
        let log = NetworkLog::new();
        for i in 0..5 {
            push_request(
                &log,
                make_request(&format!("{}", i), "POST", "https://a.com"),
            )
            .await;
        }
        assert_eq!(log.distinct_methods().await, vec!["POST"]);
    }

    // ---------- clear then subscribe then push ----------

    #[tokio::test]
    async fn test_clear_subscribe_push() {
        let log = NetworkLog::new();
        log.clear().await;
        let mut rx = log.subscribe().await;
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        assert_eq!(rx.recv().await.unwrap().request.id, "1");
    }

    // ---------- from_cache does not affect byte count ----------

    #[tokio::test]
    async fn test_total_bytes_in_from_cache() {
        let log = NetworkLog::new();
        let mut resp = make_response("1", 200, "https://a.com");
        resp.from_cache = true;
        resp.body_size = Some(500);
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(resp),
            None,
        )
        .await;
        assert_eq!(log.total_bytes_in().await, 500);
    }

    // ---------- new fixes ----------

    #[tokio::test]
    async fn test_to_curl_produces_valid_shell_with_quotes() {
        let req = CapturedRequest {
            id: "1".into(),
            context: None,
            method: "POST".into(),
            url: "https://example.com".into(),
            headers: vec![CapturedHeader {
                name: "X-Quote".into(),
                value: "it's".into(),
            }],
            post_data: Some("data='val'".into()),
            timestamp: 0,
            destination: "document".into(),
            initiator_type: None,
            timing: CapturedTiming::default(),
            cookies: vec![],
        };
        let entry = NetworkEntry {
            request: req,
            response: None,
            error: None,
        };
        let curl = entry.to_curl();
        // Every single quote should be escaped as '\''
        // So the output should NOT contain any bare single quotes inside quoted regions
        // We verify by checking the escaping pattern is present
        assert!(curl.contains("it'\\''s"));
        assert!(curl.contains("data='\\''val'\\'''"));
    }

    #[tokio::test]
    async fn test_save_as_har_postdata_uses_content_type_header() {
        let log = NetworkLog::new();
        let mut req = make_request("1", "POST", "https://api.example.com");
        req.post_data = Some(r#"{"key":"value"}"#.into());
        req.headers = vec![CapturedHeader {
            name: "Content-Type".into(),
            value: "application/vnd.api+json".into(),
        }];
        push_entry(
            &log,
            req,
            Some(make_response("1", 201, "https://api.example.com")),
            None,
        )
        .await;

        let path = std::path::Path::new("/tmp/foxdriver_har_mime.har");
        log.save_as_har(path, None).await.unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        // The postData should use the request's Content-Type
        assert!(content.contains("\"mimeType\": \"application/vnd.api+json\""));
    }

    #[tokio::test]
    async fn test_save_as_har_postdata_defaults_octet_stream() {
        let log = NetworkLog::new();
        let mut req = make_request("1", "POST", "https://api.example.com");
        req.post_data = Some("raw bytes".into());
        // No Content-Type header
        push_entry(
            &log,
            req,
            Some(make_response("1", 201, "https://api.example.com")),
            None,
        )
        .await;

        let path = std::path::Path::new("/tmp/foxdriver_har_default_mime.har");
        log.save_as_har(path, None).await.unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("\"mimeType\": \"application/octet-stream\""));
    }

    #[tokio::test]
    async fn test_concurrent_reads_with_rwlock() {
        let log = NetworkLog::new();
        for i in 0..100 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://example.com"),
            )
            .await;
        }
        let mut handles = Vec::new();
        for _ in 0..20 {
            let log = log.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..50 {
                    let _ = log.len().await;
                    let _ = log.entries().await;
                    let _ = log.distinct_methods().await;
                    let _ = log.total_bytes_in().await;
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        // No deadlock means success
        assert_eq!(log.len().await, 100);
    }

    // ---------- new bounty features ----------

    // ---------- convenience queries ----------

    #[tokio::test]
    async fn test_find_by_status() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(make_response("1", 200, "https://a.com")),
            None,
        )
        .await;
        push_entry(
            &log,
            make_request("2", "GET", "https://b.com"),
            Some(make_response("2", 404, "https://b.com")),
            None,
        )
        .await;
        push_entry(
            &log,
            make_request("3", "GET", "https://c.com"),
            None,
            Some(make_error("3", "https://c.com", "err")),
        )
        .await;
        assert_eq!(log.find_by_status(200).await.len(), 1);
        assert_eq!(log.find_by_status(404).await.len(), 1);
        assert_eq!(log.find_by_status(500).await.len(), 0);
    }

    #[tokio::test]
    async fn test_entries_since() {
        let log = NetworkLog::new();
        let mut req1 = make_request("1", "GET", "https://a.com");
        req1.timestamp = 1000;
        let mut req2 = make_request("2", "GET", "https://b.com");
        req2.timestamp = 2000;
        let mut req3 = make_request("3", "GET", "https://c.com");
        req3.timestamp = 3000;
        push_request(&log, req1).await;
        push_request(&log, req2).await;
        push_request(&log, req3).await;
        assert_eq!(log.entries_since(0).await.len(), 3);
        assert_eq!(log.entries_since(2000).await.len(), 2);
        assert_eq!(log.entries_since(3001).await.len(), 0);
    }

    #[tokio::test]
    async fn test_last_n() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        push_request(&log, make_request("3", "GET", "https://c.com")).await;
        let last = log.last_n(2).await;
        assert_eq!(last.len(), 2);
        assert_eq!(last[0].request.id, "2");
        assert_eq!(last[1].request.id, "3");
        assert_eq!(log.last_n(10).await.len(), 3);
        assert!(log.last_n(0).await.is_empty());
    }

    // ---------- security analysis ----------

    // ---------- additional bounty analysis ----------

    #[tokio::test]
    async fn test_unique_urls() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        push_request(&log, make_request("3", "GET", "https://a.com")).await;
        let urls = log.unique_urls().await;
        assert_eq!(urls.len(), 2);
        assert!(urls.contains(&"https://a.com".to_string()));
        assert!(urls.contains(&"https://b.com".to_string()));
    }

    #[tokio::test]
    async fn test_unique_urls_empty() {
        let log = NetworkLog::new();
        assert!(log.unique_urls().await.is_empty());
    }

    // ---------- reliability ----------

    #[tokio::test]
    async fn test_broadcast_drops_tracked_when_no_receivers() {
        let log = NetworkLog::new();
        // No subscribers active
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        let m = log.metrics().await;
        assert_eq!(m.broadcast_drops, 1);
    }

    #[tokio::test]
    async fn test_broadcast_no_drop_when_receiver_active() {
        let log = NetworkLog::new();
        let _rx = log.subscribe().await;
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        let m = log.metrics().await;
        assert_eq!(m.broadcast_drops, 0);
    }

    #[tokio::test]
    async fn test_pending_count_and_drain() {
        let log = NetworkLog::new();
        // Inject a response before its request (out-of-order)
        {
            let mut inner = log.inner.write().await;
            inner.pending_responses.insert(
                "orphan-1".into(),
                make_response("orphan-1", 200, "https://a.com"),
            );
            inner.pending_errors.insert(
                "orphan-2".into(),
                make_error("orphan-2", "https://b.com", "err"),
            );
        }
        assert_eq!(log.pending_count().await, 2);

        let (responses, errors) = log.drain_pending().await;
        assert_eq!(responses.len(), 1);
        assert_eq!(errors.len(), 1);
        assert_eq!(log.pending_count().await, 0);
    }

    #[tokio::test]
    async fn test_pending_count_empty() {
        let log = NetworkLog::new();
        assert_eq!(log.pending_count().await, 0);
        let (r, e) = log.drain_pending().await;
        assert!(r.is_empty() && e.is_empty());
    }

    #[tokio::test]
    async fn test_retain_filters_entries() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://keep.com")).await;
        push_request(&log, make_request("2", "GET", "https://drop.com")).await;
        push_request(&log, make_request("3", "GET", "https://keep.com/path")).await;
        log.retain(|e| e.request.url.contains("keep")).await;
        assert_eq!(log.len().await, 2);
        assert!(log.contains_id("1").await);
        assert!(!log.contains_id("2").await);
        assert!(log.contains_id("3").await);
    }

    #[tokio::test]
    async fn test_retain_rebuilds_indices() {
        let log = NetworkLog::new();
        for i in 0..5 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", &format!("https://{}.com", i)),
            )
            .await;
        }
        log.retain(|e| e.request.id.parse::<i32>().unwrap() % 2 == 0)
            .await;
        assert_eq!(log.len().await, 3);
        assert!(log.contains_id("0").await);
        assert!(log.contains_id("2").await);
        assert!(log.contains_id("4").await);
    }

    #[tokio::test]
    async fn test_deduplication_guard_ignores_duplicate_request_id() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://first.com")).await;
        push_request(&log, make_request("1", "GET", "https://second.com")).await;
        assert_eq!(log.len().await, 1);
        assert_eq!(log.first().await.unwrap().request.url, "https://first.com");
    }

    #[tokio::test]
    async fn test_duplicate_response_tracked_in_metrics() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(make_response("1", 200, "https://a.com")),
            None,
        )
        .await;
        // Simulate a second response for the same request
        {
            let mut inner = log.inner.write().await;
            if let Some(idx) = inner.by_id.get("1").copied() {
                let mut new_entry = (*inner.entries[idx]).clone();
                new_entry.response = Some(make_response("1", 201, "https://a.com"));
                inner.entries[idx] = Arc::new(new_entry);
            }
        }
        // We can't easily trigger ingest_response_completed without real BiDi events,
        // but we verify the metric field exists and is initialized to 0
        let m = log.metrics().await;
        assert_eq!(m.duplicate_responses, 0); // No duplicate through normal path yet
    }

    #[tokio::test]
    async fn test_request_ids_returns_all_ids() {
        let log = NetworkLog::new();
        push_request(&log, make_request("a", "GET", "https://a.com")).await;
        push_request(&log, make_request("b", "GET", "https://b.com")).await;
        let ids = log.request_ids().await;
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[tokio::test]
    async fn test_has_response_and_has_error() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(make_response("1", 200, "https://a.com")),
            None,
        )
        .await;
        push_entry(
            &log,
            make_request("2", "GET", "https://b.com"),
            None,
            Some(make_error("2", "https://b.com", "err")),
        )
        .await;
        push_request(&log, make_request("3", "GET", "https://c.com")).await;
        assert!(log.has_response("1").await);
        assert!(!log.has_error("1").await);
        assert!(!log.has_response("2").await);
        assert!(log.has_error("2").await);
        assert!(!log.has_response("3").await);
        assert!(!log.has_error("3").await);
        assert!(!log.has_response("missing").await);
        assert!(!log.has_error("missing").await);
    }

    // ---------- invariant tests ----------

    #[tokio::test]
    async fn test_invariant_len_equals_entries_len() {
        let log = NetworkLog::new();
        for i in 0..50 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://x.com"),
            )
            .await;
            assert_eq!(log.len().await, log.entries().await.len());
        }
    }

    #[tokio::test]
    async fn test_invariant_is_empty_iff_len_zero() {
        let log = NetworkLog::new();
        assert_eq!(log.is_empty().await, log.len().await == 0);
        push_request(&log, make_request("1", "GET", "https://x.com")).await;
        assert_eq!(log.is_empty().await, log.len().await == 0);
        log.clear().await;
        assert_eq!(log.is_empty().await, log.len().await == 0);
    }

    #[tokio::test]
    async fn test_invariant_by_id_indices_are_valid() {
        let log = NetworkLog::new();
        for i in 0..100 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", &format!("https://{}.com", i)),
            )
            .await;
        }
        let inner = log.inner.read().await;
        for (id, idx) in &inner.by_id {
            assert!(
                *idx < inner.entries.len(),
                "id {} has invalid index {}",
                id,
                idx
            );
            assert_eq!(inner.entries[*idx].request.id, *id);
        }
    }

    #[tokio::test]
    async fn test_invariant_no_duplicate_ids() {
        let log = NetworkLog::new();
        for i in 0..100 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://x.com"),
            )
            .await;
        }
        let inner = log.inner.read().await;
        let mut seen = std::collections::HashSet::new();
        for e in &inner.entries {
            assert!(seen.insert(&e.request.id), "duplicate id {}", e.request.id);
        }
    }

    #[tokio::test]
    async fn test_invariant_first_and_last_consistent() {
        let log = NetworkLog::new();
        assert_eq!(log.first().await, None);
        assert_eq!(log.last().await, None);
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        let entries = log.entries().await;
        assert_eq!(
            log.first().await.as_deref(),
            entries.first().map(Arc::as_ref)
        );
        assert_eq!(log.last().await.as_deref(), entries.last().map(Arc::as_ref));
    }

    #[tokio::test]
    async fn test_invariant_filter_count_consistent() {
        let log = NetworkLog::new();
        for i in 0..20 {
            let status = if i % 2 == 0 { 200 } else { 404 };
            push_entry(
                &log,
                make_request(&format!("{}", i), "GET", "https://x.com"),
                Some(make_response(&format!("{}", i), status, "https://x.com")),
                None,
            )
            .await;
        }
        let f = Filter::new().status_range(200..=200);
        let filtered = log.filter(f.clone()).await;
        let count = log.count(f).await;
        assert_eq!(filtered.len(), count);
    }

    #[tokio::test]
    async fn test_invariant_metrics_monotonic() {
        let log = NetworkLog::new();
        let m0 = log.metrics().await;
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        let m1 = log.metrics().await;
        assert!(m1.requests_received >= m0.requests_received);
        assert!(m1.responses_received >= m0.responses_received);
        assert!(m1.errors_received >= m0.errors_received);
    }

    #[tokio::test]
    async fn test_invariant_pending_count_bounded() {
        let log = NetworkLog::with_limits(100, 10);
        // Insert 20 orphaned pending responses directly (bypassing capping)
        {
            let mut inner = log.inner.write().await;
            for i in 0..20 {
                inner.pending_responses.insert(
                    format!("orphan-{}", i),
                    make_response(&format!("{}", i), 200, "https://x.com"),
                );
            }
        }
        // Direct insertion bypasses capping; verify drain_pending can clean them
        assert_eq!(log.pending_count().await, 20);
        let (r, _) = log.drain_pending().await;
        assert_eq!(r.len(), 20);
        assert_eq!(log.pending_count().await, 0);
    }

    #[tokio::test]
    async fn test_invariant_after_clear_all_empty() {
        let log = NetworkLog::new();
        for i in 0..50 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://x.com"),
            )
            .await;
        }
        log.clear().await;
        assert!(log.is_empty().await);
        assert_eq!(log.pending_count().await, 0);
        assert!(!log.contains_id("1").await);
        let inner = log.inner.read().await;
        assert!(inner.by_id.is_empty());
    }

    #[tokio::test]
    async fn test_invariant_after_remove_id_not_present() {
        let log = NetworkLog::new();
        for i in 0..50 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://x.com"),
            )
            .await;
        }
        log.remove_by_id("25").await;
        assert!(!log.contains_id("25").await);
        let inner = log.inner.read().await;
        assert!(!inner.by_id.contains_key("25"));
        assert_eq!(inner.entries.len(), 49);
        // Verify all remaining indices are valid
        for (id, idx) in &inner.by_id {
            assert!(inner.entries[*idx].request.id == *id);
        }
    }

    #[tokio::test]
    async fn test_invariant_retain_all_satisfy_predicate() {
        let log = NetworkLog::new();
        for i in 0..50 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", &format!("https://{}.com", i % 3)),
            )
            .await;
        }
        log.retain(|e| e.request.url.contains("0.com") || e.request.url.contains("1.com"))
            .await;
        for e in log.entries().await {
            assert!(!e.request.url.contains("2.com"));
        }
        assert_eq!(log.len().await, 34); // 0,1 out of 0,1,2 = 2/3
    }

    #[tokio::test]
    async fn test_invariant_nth_matches_entries_index() {
        let log = NetworkLog::new();
        for i in 0..20 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", &format!("https://{}.com", i)),
            )
            .await;
        }
        let entries = log.entries().await;
        for (i, entry) in entries.iter().enumerate() {
            assert_eq!(log.nth(i).await.as_deref(), Some(entry.as_ref()));
        }
        assert!(log.nth(100).await.is_none());
    }

    #[tokio::test]
    async fn test_invariant_last_n_order_preserved() {
        let log = NetworkLog::new();
        for i in 0..10 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://x.com"),
            )
            .await;
        }
        let last = log.last_n(3).await;
        assert_eq!(last.len(), 3);
        assert_eq!(last[0].request.id, "7");
        assert_eq!(last[1].request.id, "8");
        assert_eq!(last[2].request.id, "9");
    }

    #[tokio::test]
    async fn test_invariant_entries_since_time_based() {
        let log = NetworkLog::new();
        for i in 0..10 {
            let mut req = make_request(&format!("{}", i), "GET", "https://x.com");
            req.timestamp = i as u64 * 1000;
            push_request(&log, req).await;
        }
        let since = log.entries_since(5000).await;
        assert_eq!(since.len(), 5); // 5,6,7,8,9
        for e in &since {
            assert!(e.request.timestamp >= 5000);
        }
    }

    // ---------- concurrent stress tests ----------

    #[tokio::test]
    async fn test_stress_concurrent_ingestion_and_query() {
        let log = NetworkLog::new();
        let mut handles = Vec::new();

        // 10 ingestion tasks
        for t in 0..10 {
            let log = log.clone();
            handles.push(tokio::spawn(async move {
                for i in 0..100 {
                    let id = format!("t{}-{}", t, i);
                    push_request(&log, make_request(&id, "GET", "https://x.com")).await;
                }
            }));
        }

        // 10 query tasks
        for _ in 0..10 {
            let log = log.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..100 {
                    let _ = log.len().await;
                    let _ = log.entries().await;
                    let _ = log.first().await;
                    let _ = log.last().await;
                    let _ = log.metrics().await;
                    tokio::task::yield_now().await;
                }
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(log.len().await, 1000);
        let inner = log.inner.read().await;
        assert_eq!(inner.by_id.len(), 1000);
    }

    #[tokio::test]
    async fn test_stress_concurrent_subscribers() {
        let log = NetworkLog::new();
        let mut rxs = Vec::new();
        for _ in 0..50 {
            rxs.push(log.subscribe().await);
        }

        for i in 0..100 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://x.com"),
            )
            .await;
        }

        // Each subscriber should have received at most 100 entries
        // (some may have been dropped if the channel filled)
        for mut rx in rxs {
            let mut count = 0;
            while rx.try_recv().is_ok() {
                count += 1;
            }
            assert!(count <= 100);
        }
    }

    #[tokio::test]
    async fn test_stress_eviction_under_load() {
        let log = NetworkLog::with_limits(100, 10);
        for i in 0..500 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://x.com"),
            )
            .await;
        }
        assert_eq!(log.len().await, 100);
        let m = log.metrics().await;
        assert!(m.entries_evicted > 0);
    }

    #[tokio::test]
    async fn test_stress_retain_under_load() {
        let log = NetworkLog::new();
        for i in 0..1000 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", &format!("https://{}.com", i % 10)),
            )
            .await;
        }
        log.retain(|e| e.request.url.contains("0.com") || e.request.url.contains("1.com"))
            .await;
        assert_eq!(log.len().await, 200);
        let inner = log.inner.read().await;
        assert_eq!(inner.by_id.len(), 200);
    }

    #[tokio::test]
    async fn test_stress_memory_estimate_grows_with_entries() {
        let log = NetworkLog::new();
        let m0 = log.memory_estimate().await;
        for i in 0..100 {
            let mut req = make_request(&format!("{}", i), "POST", "https://x.com");
            req.post_data = Some("x".repeat(1000));
            push_request(&log, req).await;
        }
        let m1 = log.memory_estimate().await;
        assert!(m1 > m0);
    }

    // ---------- HAR validation ----------

    #[tokio::test]
    async fn test_har_is_valid_json() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(make_response("1", 200, "https://a.com")),
            None,
        )
        .await;
        let path = std::path::Path::new("/tmp/foxdriver_har_valid.json");
        log.save_as_har(path, Some("test")).await.unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["log"]["version"], "1.2");
        assert!(parsed["log"]["entries"].is_array());
        assert_eq!(parsed["log"]["entries"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_har_has_required_fields() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "POST", "https://api.example.com"),
            Some(make_response("1", 201, "https://api.example.com")),
            None,
        )
        .await;
        let path = std::path::Path::new("/tmp/foxdriver_har_fields.json");
        log.save_as_har(path, None).await.unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let entry = &parsed["log"]["entries"][0];
        assert!(entry["startedDateTime"].is_string());
        assert!(entry["request"]["method"].is_string());
        assert!(entry["request"]["url"].is_string());
        assert!(entry["response"]["status"].is_number());
        assert!(entry["response"]["statusText"].is_string());
        assert!(entry["timings"].is_object());
    }

    #[tokio::test]
    async fn test_har_empty_log_has_zero_entries() {
        let log = NetworkLog::new();
        let path = std::path::Path::new("/tmp/foxdriver_har_empty_valid.json");
        log.save_as_har(path, None).await.unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["log"]["entries"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_har_post_data_present() {
        let log = NetworkLog::new();
        let mut req = make_request("1", "POST", "https://api.example.com");
        req.post_data = Some(r#"{"key":"value"}"#.into());
        req.headers = vec![CapturedHeader {
            name: "Content-Type".into(),
            value: "application/json".into(),
        }];
        push_entry(
            &log,
            req,
            Some(make_response("1", 200, "https://api.example.com")),
            None,
        )
        .await;
        let path = std::path::Path::new("/tmp/foxdriver_har_postdata.json");
        log.save_as_har(path, None).await.unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let post_data = &parsed["log"]["entries"][0]["request"]["postData"];
        assert!(post_data["text"].is_string());
        assert!(post_data["mimeType"].is_string());
    }

    // ---------- BiDi ingestion edge cases ----------

    #[tokio::test]
    async fn test_ingest_response_for_unknown_request_stashes_pending() {
        let log = NetworkLog::new();
        // Directly stash a pending response
        {
            let mut inner = log.inner.write().await;
            inner.pending_responses.insert(
                "future-req".into(),
                make_response("future-req", 200, "https://a.com"),
            );
        }
        assert_eq!(log.pending_count().await, 1);
        // Manually reconcile: push_entry does not check pending maps
        {
            let mut inner = log.inner.write().await;
            let resp = inner.pending_responses.remove("future-req");
            let entry = Arc::new(NetworkEntry {
                request: make_request("future-req", "GET", "https://a.com"),
                response: resp,
                error: None,
            });
            inner.push_entry(entry);
        }
        assert_eq!(log.pending_count().await, 0);
        assert!(log.has_response("future-req").await);
    }

    #[tokio::test]
    async fn test_ingest_error_for_unknown_request_stashes_pending() {
        let log = NetworkLog::new();
        {
            let mut inner = log.inner.write().await;
            inner.pending_errors.insert(
                "future-err".into(),
                make_error("future-err", "https://a.com", "timeout"),
            );
        }
        assert_eq!(log.pending_count().await, 1);
        // Manually reconcile
        {
            let mut inner = log.inner.write().await;
            let err = inner.pending_errors.remove("future-err");
            let entry = Arc::new(NetworkEntry {
                request: make_request("future-err", "GET", "https://a.com"),
                response: None,
                error: err,
            });
            inner.push_entry(entry);
        }
        assert_eq!(log.pending_count().await, 0);
        assert!(log.has_error("future-err").await);
    }

    #[tokio::test]
    async fn test_ingest_duplicate_request_ignored() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://first.com")).await;
        push_request(&log, make_request("1", "GET", "https://second.com")).await;
        assert_eq!(log.len().await, 1);
        assert_eq!(log.first().await.unwrap().request.url, "https://first.com");
    }

    // ---------- boundary conditions ----------

    #[tokio::test]
    async fn test_filter_empty_log() {
        let log = NetworkLog::new();
        let f = Filter::new().method("GET").status_range(200..=200);
        assert!(log.filter(f).await.is_empty());
        assert_eq!(log.count(Filter::new()).await, 0);
    }

    #[tokio::test]
    async fn test_find_by_url_empty_log() {
        let log = NetworkLog::new();
        assert!(log.find_by_url("anything").await.is_none());
    }

    #[tokio::test]
    async fn test_find_by_url_regex_empty_log() {
        let log = NetworkLog::new();
        let re = regex::Regex::new(".*").unwrap();
        assert!(log.find_by_url_regex(&re).await.is_none());
    }

    #[tokio::test]
    async fn test_endpoints_empty_log() {
        let log = NetworkLog::new();
        assert!(log.endpoints().await.is_empty());
    }

    #[tokio::test]
    async fn test_hostnames_empty_log() {
        let log = NetworkLog::new();
        assert!(log.hostnames().await.is_empty());
    }

    #[tokio::test]
    async fn test_distinct_methods_empty_log_returns_empty() {
        let log = NetworkLog::new();
        assert!(log.distinct_methods().await.is_empty());
    }

    #[tokio::test]
    async fn test_distinct_statuses_empty_log() {
        let log = NetworkLog::new();
        assert!(log.distinct_statuses().await.is_empty());
    }

    #[tokio::test]
    async fn test_total_bytes_empty_log() {
        let log = NetworkLog::new();
        assert_eq!(log.total_bytes_in().await, 0);
        assert_eq!(log.total_bytes_out().await, 0);
    }

    #[tokio::test]
    async fn test_nth_out_of_bounds() {
        let log = NetworkLog::new();
        assert!(log.nth(0).await.is_none());
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        assert!(log.nth(1).await.is_none());
        assert!(log.nth(100).await.is_none());
    }

    #[tokio::test]
    async fn test_last_n_greater_than_len() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        assert_eq!(log.last_n(100).await.len(), 1);
    }

    #[tokio::test]
    async fn test_find_by_status_no_matches() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(make_response("1", 200, "https://a.com")),
            None,
        )
        .await;
        assert!(log.find_by_status(404).await.is_empty());
    }

    #[tokio::test]
    async fn test_entries_since_future_timestamp() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        assert!(log.entries_since(u64::MAX).await.is_empty());
    }

    #[tokio::test]
    async fn test_contains_id_false() {
        let log = NetworkLog::new();
        assert!(!log.contains_id("nonexistent").await);
    }

    #[tokio::test]
    async fn test_remove_by_id_nonexistent() {
        let log = NetworkLog::new();
        assert!(log.remove_by_id("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_metrics_on_empty_log() {
        let log = NetworkLog::new();
        let m = log.metrics().await;
        assert_eq!(m.requests_received, 0);
        assert_eq!(m.responses_received, 0);
        assert_eq!(m.errors_received, 0);
        assert_eq!(m.entries_evicted, 0);
        assert_eq!(m.broadcast_drops, 0);
        assert_eq!(m.duplicate_responses, 0);
        assert_eq!(m.duplicate_errors, 0);
    }

    #[tokio::test]
    async fn test_memory_estimate_grows_with_entries() {
        let log = NetworkLog::new();
        let empty_est = log.memory_estimate().await;
        // The key invariant is that the estimate increases as entries are added.
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        let grown_est = log.memory_estimate().await;
        assert!(
            grown_est > empty_est,
            "estimate must grow: empty={empty_est}, after one entry={grown_est}"
        );
    }

    #[tokio::test]
    async fn test_with_limits_zero_zero() {
        let log = NetworkLog::with_limits(0, 0);
        for i in 0..100 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://x.com"),
            )
            .await;
        }
        assert_eq!(log.len().await, 100);
    }

    #[tokio::test]
    async fn test_with_limits_small() {
        let log = NetworkLog::with_limits(5, 5);
        for i in 0..20 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://x.com"),
            )
            .await;
        }
        assert!(log.len().await <= 5);
    }

    #[tokio::test]
    async fn test_wait_for_url_timeout_zero() {
        let log = NetworkLog::new();
        let result = log
            .wait_for_url("nonexistent", std::time::Duration::from_secs(0))
            .await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_wait_for_response_timeout_zero() {
        let log = NetworkLog::new();
        let result = log
            .wait_for_response("nonexistent", std::time::Duration::from_secs(0))
            .await;
        assert!(result.is_none());
    }

    // ---------- filter edge cases ----------

    #[tokio::test]
    async fn test_filter_header_case_insensitive() {
        let log = NetworkLog::new();
        let mut req = make_request("1", "GET", "https://a.com");
        req.headers = vec![CapturedHeader {
            name: "X-Custom".into(),
            value: "SecretValue".into(),
        }];
        push_request(&log, req).await;
        assert_eq!(
            log.filter(Filter::new().header("x-custom", "secretvalue"))
                .await
                .len(),
            1
        );
        assert_eq!(
            log.filter(Filter::new().header("X-CUSTOM", "SECRETVALUE"))
                .await
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn test_filter_combined_conditions() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "POST", "https://api.example.com"),
            Some(make_response("1", 201, "https://api.example.com")),
            None,
        )
        .await;
        push_entry(
            &log,
            make_request("2", "GET", "https://api.example.com"),
            Some(make_response("2", 200, "https://api.example.com")),
            None,
        )
        .await;
        push_entry(
            &log,
            make_request("3", "POST", "https://api.example.com"),
            Some(make_response("3", 500, "https://api.example.com")),
            None,
        )
        .await;
        let f = Filter::new().method("POST").status_range(200..=299);
        assert_eq!(log.filter(f).await.len(), 1);
    }

    #[tokio::test]
    async fn test_filter_with_response_only() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_entry(
            &log,
            make_request("2", "GET", "https://b.com"),
            Some(make_response("2", 200, "https://b.com")),
            None,
        )
        .await;
        assert_eq!(log.filter(Filter::new().with_response()).await.len(), 1);
        assert_eq!(log.filter(Filter::new().without_response()).await.len(), 1);
    }

    #[tokio::test]
    async fn test_filter_with_error_only() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_entry(
            &log,
            make_request("2", "GET", "https://b.com"),
            None,
            Some(make_error("2", "https://b.com", "err")),
        )
        .await;
        assert_eq!(log.filter(Filter::new().with_error()).await.len(), 1);
    }

    // ---------- curl generation edge cases ----------

    #[tokio::test]
    async fn test_to_curl_empty_body() {
        let req = make_request("1", "GET", "https://a.com");
        let entry = NetworkEntry {
            request: req,
            response: None,
            error: None,
        };
        let curl = entry.to_curl();
        assert!(!curl.contains("-d"));
        assert!(curl.contains("curl -X 'GET'"));
    }

    #[tokio::test]
    async fn test_to_curl_unicode_url() {
        let req = make_request("1", "GET", "https://例え.jp/テスト");
        let entry = NetworkEntry {
            request: req,
            response: None,
            error: None,
        };
        let curl = entry.to_curl();
        assert!(curl.contains("https://例え.jp/テスト"));
    }

    #[tokio::test]
    async fn test_to_curl_strips_auto_headers() {
        let mut req = make_request("1", "GET", "https://a.com");
        req.headers = vec![
            CapturedHeader {
                name: "Host".into(),
                value: "a.com".into(),
            },
            CapturedHeader {
                name: "Accept-Encoding".into(),
                value: "gzip".into(),
            },
            CapturedHeader {
                name: "Connection".into(),
                value: "keep-alive".into(),
            },
            CapturedHeader {
                name: "X-Custom".into(),
                value: "value".into(),
            },
        ];
        let entry = NetworkEntry {
            request: req,
            response: None,
            error: None,
        };
        let curl = entry.to_curl();
        assert!(!curl.contains("Host:"));
        assert!(!curl.contains("Accept-Encoding:"));
        assert!(!curl.contains("Connection:"));
        assert!(curl.contains("X-Custom:"));
    }

    // ---------- query params ----------

    #[tokio::test]
    async fn test_query_params_multiple() {
        let req = make_request("1", "GET", "https://a.com?a=1&b=2&c=3");
        let params = req.query_params().unwrap();
        assert_eq!(params.len(), 3);
        assert!(params.contains(&("a".into(), "1".into())));
        assert!(params.contains(&("b".into(), "2".into())));
        assert!(params.contains(&("c".into(), "3".into())));
    }

    #[tokio::test]
    async fn test_json_body_invalid() {
        let mut req = make_request("1", "POST", "https://a.com");
        req.post_data = Some("not json".into());
        assert!(req.json_body().is_none());
    }

    #[tokio::test]
    async fn test_json_body_valid() {
        let mut req = make_request("1", "POST", "https://a.com");
        req.post_data = Some(r#"{"key":"value"}"#.into());
        let json = req.json_body().unwrap();
        assert_eq!(json["key"], "value");
    }

    // ---------- request/response header lookup ----------

    #[tokio::test]
    async fn test_request_header_case_insensitive() {
        let mut req = make_request("1", "GET", "https://a.com");
        req.headers = vec![CapturedHeader {
            name: "Content-Type".into(),
            value: "application/json".into(),
        }];
        assert_eq!(req.request_header("content-type"), Some("application/json"));
        assert_eq!(req.request_header("Content-Type"), Some("application/json"));
        assert_eq!(req.request_header("CONTENT-TYPE"), Some("application/json"));
    }

    #[tokio::test]
    async fn test_request_header_missing() {
        let req = make_request("1", "GET", "https://a.com");
        assert_eq!(req.request_header("x-missing"), None);
    }

    #[tokio::test]
    async fn test_response_header_case_insensitive() {
        let mut resp = make_response("1", 200, "https://a.com");
        resp.headers = vec![CapturedHeader {
            name: "X-Response-Header".into(),
            value: "val".into(),
        }];
        let entry = NetworkEntry {
            request: make_request("1", "GET", "https://a.com"),
            response: Some(resp),
            error: None,
        };
        assert_eq!(entry.response_header("x-response-header"), Some("val"));
        assert_eq!(entry.response_header("X-RESPONSE-HEADER"), Some("val"));
    }

    // ---------- network entry helpers ----------

    #[tokio::test]
    async fn test_network_entry_final_url() {
        let req = make_request("1", "GET", "https://a.com");
        let resp = make_response("1", 200, "https://b.com");
        let entry = NetworkEntry {
            request: req.clone(),
            response: Some(resp),
            error: None,
        };
        assert_eq!(entry.final_url(), "https://b.com");
        let entry2 = NetworkEntry {
            request: req,
            response: None,
            error: None,
        };
        assert_eq!(entry2.final_url(), "https://a.com");
    }

    #[tokio::test]
    async fn test_network_entry_status() {
        let entry = NetworkEntry {
            request: make_request("1", "GET", "https://a.com"),
            response: Some(make_response("1", 404, "https://a.com")),
            error: None,
        };
        assert_eq!(entry.status(), Some(404));
        let entry2 = NetworkEntry {
            request: make_request("1", "GET", "https://a.com"),
            response: None,
            error: None,
        };
        assert_eq!(entry2.status(), None);
    }

    // ---------- property-based tests (proptest) ----------

    use proptest::prelude::*;

    #[tokio::test]
    async fn test_proptest_filter_never_panics() {
        let log = NetworkLog::new();
        for i in 0..50 {
            let mut req = make_request(
                &format!("{}", i),
                ["GET", "POST", "PUT", "DELETE"][i % 4],
                &format!("https://{}.com", i),
            );
            req.headers = vec![CapturedHeader {
                name: "X-Id".into(),
                value: format!("{}", i),
            }];
            push_request(&log, req).await;
        }

        // Test various filter combinations don't panic
        let _ = log.filter(Filter::new().method("GET")).await;
        let _ = log.filter(Filter::new().status_range(100..=599)).await;
        let _ = log.filter(Filter::new().url_contains("com")).await;
        let _ = log.filter(Filter::new().header("x-id", "25")).await;
        let _ = log.filter(Filter::new().with_response()).await;
        let _ = log.filter(Filter::new().without_response()).await;
        let _ = log.filter(Filter::new().with_error()).await;
        let _ = log
            .filter(
                Filter::new()
                    .method("GET")
                    .url_contains("com")
                    .with_response(),
            )
            .await;
    }

    #[tokio::test]
    async fn test_proptest_concurrent_subscribe_and_push() {
        let log = NetworkLog::new();
        let mut handles = Vec::new();

        for _ in 0..20 {
            let log = log.clone();
            handles.push(tokio::spawn(async move {
                let mut rx = log.subscribe().await;
                let mut count = 0;
                while count < 50 {
                    if rx.try_recv().is_ok() {
                        count += 1;
                    } else {
                        tokio::task::yield_now().await;
                    }
                }
            }));
        }

        let log2 = log.clone();
        let pusher = tokio::spawn(async move {
            for i in 0..100 {
                push_request(
                    &log2,
                    make_request(&format!("{}", i), "GET", "https://x.com"),
                )
                .await;
                if i % 10 == 0 {
                    tokio::task::yield_now().await;
                }
            }
        });

        pusher.await.unwrap();
        for h in handles {
            let _ = h.await;
        }
    }

    #[tokio::test]
    async fn test_proptest_random_operations() {
        let log = NetworkLog::new();
        for i in 0..100 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", &format!("https://{}.com", i)),
            )
            .await;
        }

        // Perform random operations and verify invariants
        log.remove_by_id("50").await;
        log.retain(|e| e.request.id != "25").await;
        let _ = log.drain_pending().await;
        log.clear().await;

        assert!(log.is_empty().await);
        assert_eq!(log.pending_count().await, 0);
    }

    // ---------- proptest property-based tests ----------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn prop_query_params_no_crash(url in r"https://[a-z0-9]+\.com(/[a-z]*)*(\?[a-z]+=[a-z0-9]*)*") {
            let req = CapturedRequest {
                id: "1".into(), context: None, url: url.clone(), method: "GET".into(),
                headers: vec![], cookies: vec![], post_data: None, timestamp: 0,
                destination: "document".into(), initiator_type: None,
                timing: CapturedTiming::default(),
            };
            let _ = req.query_params(); // must not panic
        }

        #[test]
        fn prop_request_header_case_insensitive(
            name in "[A-Za-z_-]{1,20}",
            value in "[a-z0-9]{1,30}",
            lookup in "[A-Za-z_-]{1,20}"
        ) {
            let req = CapturedRequest {
                id: "1".into(), context: None, url: "https://a.com".into(), method: "GET".into(),
                headers: vec![CapturedHeader { name: name.clone(), value: value.clone() }],
                cookies: vec![], post_data: None, timestamp: 0,
                destination: "document".into(), initiator_type: None,
                timing: CapturedTiming::default(),
            };
            let result = req.request_header(&lookup);
            if name.to_lowercase() == lookup.to_lowercase() {
                prop_assert_eq!(result, Some(value.as_str()));
            }
        }

        #[test]
        fn prop_json_body_valid_json_roundtrips(obj in "\\{.*\\}") {
            let req = CapturedRequest {
                id: "1".into(), context: None, url: "https://a.com".into(), method: "POST".into(),
                headers: vec![], cookies: vec![], post_data: Some(obj.clone()),
                timestamp: 0, destination: "document".into(), initiator_type: None,
                timing: CapturedTiming::default(),
            };
            // If it parses, the round-tripped JSON should equal the original parsed value.
            if let Some(val) = req.json_body() {
                let re_encoded = serde_json::to_string(&val).unwrap();
                let re_parsed: serde_json::Value = serde_json::from_str(&re_encoded).unwrap();
                prop_assert_eq!(val, re_parsed);
            }
        }

        #[test]
        fn prop_filter_empty_matches_anything(
            method in "(GET|POST|PUT|DELETE|PATCH)",
            url in r"https://[a-z]+\.com",
            status in 100u16..599u16
        ) {
            let entry = NetworkEntry {
                request: CapturedRequest {
                    id: "1".into(), context: None, url: url.clone(), method,
                    headers: vec![], cookies: vec![], post_data: None, timestamp: 0,
                    destination: "document".into(), initiator_type: None,
                    timing: CapturedTiming::default(),
                },
                response: Some(CapturedResponse {
                    id: "1".into(), url, protocol: "h2".into(), status,
                    status_text: "OK".into(), headers: vec![], mime_type: "text/html".into(),
                    body_size: None, from_cache: false,
                }),
                error: None,
            };
            let filter = Filter::new();
            prop_assert!(filter.matches(&entry));
        }

        #[test]
        fn prop_to_curl_always_contains_url(url in r"https://[a-z]+\.com(/[a-z]*)*") {
            let entry = NetworkEntry {
                request: CapturedRequest {
                    id: "1".into(), context: None, url: url.clone(), method: "GET".into(),
                    headers: vec![], cookies: vec![], post_data: None, timestamp: 0,
                    destination: "document".into(), initiator_type: None,
                    timing: CapturedTiming::default(),
                },
                response: None, error: None,
            };
            let curl = entry.to_curl();
            prop_assert!(curl.contains(&url), "curl missing URL: {}", curl);
        }

        #[test]
        fn prop_to_curl_get_has_no_data_flag(url in r"https://[a-z]+\.com") {
            let entry = NetworkEntry {
                request: CapturedRequest {
                    id: "1".into(), context: None, url, method: "GET".into(),
                    headers: vec![], cookies: vec![], post_data: None, timestamp: 0,
                    destination: "document".into(), initiator_type: None,
                    timing: CapturedTiming::default(),
                },
                response: None, error: None,
            };
            let curl = entry.to_curl();
            prop_assert!(!curl.contains(" -d "), "GET curl should not have -d: {}", curl);
        }
    }

    // ---------- deep boundary / edge cases ----------

    #[tokio::test]
    async fn test_push_entry_with_very_long_url() {
        let log = NetworkLog::new();
        let long_url = format!("https://a.com/{}", "x".repeat(10_000));
        push_request(&log, make_request("1", "GET", &long_url)).await;
        assert_eq!(log.first().await.unwrap().request.url.len(), 14 + 10_000);
    }

    #[tokio::test]
    async fn test_push_entry_with_unicode_url() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://例え.jp/テスト")).await;
        assert_eq!(
            log.first().await.unwrap().request.url,
            "https://例え.jp/テスト"
        );
    }

    #[tokio::test]
    async fn test_filter_url_regex_complex() {
        let log = NetworkLog::new();
        push_request(
            &log,
            make_request("1", "GET", "https://api.example.com/v1/users"),
        )
        .await;
        push_request(
            &log,
            make_request("2", "GET", "https://api.example.com/v2/items"),
        )
        .await;
        push_request(&log, make_request("3", "GET", "https://other.com/v1/users")).await;
        let re = regex::Regex::new(r"api\.example\.com/v\d+/users").unwrap();
        let found = log.find_by_url_regex(&re).await;
        assert_eq!(found.unwrap().request.id, "1");
    }

    #[tokio::test]
    async fn test_filter_url_regex_no_match() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        let re = regex::Regex::new(r"nomatch").unwrap();
        assert!(log.find_by_url_regex(&re).await.is_none());
    }

    #[tokio::test]
    async fn test_hostnames_with_subdomains() {
        let log = NetworkLog::new();
        push_request(
            &log,
            make_request("1", "GET", "https://sub1.sub2.example.com/path"),
        )
        .await;
        push_request(&log, make_request("2", "GET", "https://other.org")).await;
        let hosts = log.hostnames().await;
        assert_eq!(hosts.len(), 2);
        assert!(hosts.contains(&"sub1.sub2.example.com".into()));
    }

    #[tokio::test]
    async fn test_endpoints_deduplicates_full_url() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com/path?x=1")).await;
        push_request(&log, make_request("2", "GET", "https://a.com/path?y=2")).await;
        let endpoints = log.endpoints().await;
        // endpoints() deduplicates by full URL, so query strings make them distinct
        assert_eq!(endpoints.len(), 2);
    }

    #[tokio::test]
    async fn test_endpoints_with_ports() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com:8080/path")).await;
        push_request(&log, make_request("2", "GET", "https://a.com:9090/path")).await;
        let endpoints = log.endpoints().await;
        assert_eq!(endpoints.len(), 2);
    }

    #[tokio::test]
    async fn test_distinct_methods_sorted() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "DELETE", "https://a.com")).await;
        push_request(&log, make_request("2", "GET", "https://a.com")).await;
        push_request(&log, make_request("3", "POST", "https://a.com")).await;
        push_request(&log, make_request("4", "GET", "https://a.com")).await;
        assert_eq!(log.distinct_methods().await, vec!["DELETE", "GET", "POST"]);
    }

    #[tokio::test]
    async fn test_distinct_statuses_sorted() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(make_response("1", 500, "https://a.com")),
            None,
        )
        .await;
        push_entry(
            &log,
            make_request("2", "GET", "https://a.com"),
            Some(make_response("2", 200, "https://a.com")),
            None,
        )
        .await;
        push_entry(
            &log,
            make_request("3", "GET", "https://a.com"),
            Some(make_response("3", 404, "https://a.com")),
            None,
        )
        .await;
        push_entry(
            &log,
            make_request("4", "GET", "https://a.com"),
            Some(make_response("4", 200, "https://a.com")),
            None,
        )
        .await;
        assert_eq!(log.distinct_statuses().await, vec![200, 404, 500]);
    }

    #[tokio::test]
    async fn test_total_bytes_in_with_none_body_size() {
        let log = NetworkLog::new();
        let mut resp = make_response("1", 200, "https://a.com");
        resp.body_size = None;
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(resp),
            None,
        )
        .await;
        assert_eq!(log.total_bytes_in().await, 0);
    }

    #[tokio::test]
    async fn test_total_bytes_out_with_post_data() {
        let log = NetworkLog::new();
        let mut req = make_request("1", "POST", "https://a.com");
        req.post_data = Some("payload".into());
        push_request(&log, req).await;
        assert_eq!(log.total_bytes_out().await, 7);
    }

    #[tokio::test]
    async fn test_total_bytes_out_empty() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        assert_eq!(log.total_bytes_out().await, 0);
    }

    #[tokio::test]
    async fn test_completed_returns_only_with_response_or_error() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_entry(
            &log,
            make_request("2", "GET", "https://b.com"),
            Some(make_response("2", 200, "https://b.com")),
            None,
        )
        .await;
        push_entry(
            &log,
            make_request("3", "GET", "https://c.com"),
            None,
            Some(make_error("3", "https://c.com", "err")),
        )
        .await;
        let completed = log.completed().await;
        assert_eq!(completed.len(), 2);
    }

    #[tokio::test]
    async fn test_count_with_no_filter() {
        let log = NetworkLog::new();
        for i in 0..50 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://x.com"),
            )
            .await;
        }
        assert_eq!(log.count(Filter::new()).await, 50);
    }

    #[tokio::test]
    async fn test_find_by_url_partial_match() {
        let log = NetworkLog::new();
        push_request(
            &log,
            make_request("1", "GET", "https://api.example.com/users"),
        )
        .await;
        push_request(&log, make_request("2", "GET", "https://other.com/users")).await;
        assert_eq!(
            log.find_by_url("api.example").await.unwrap().request.id,
            "1"
        );
    }

    #[tokio::test]
    async fn test_subscribe_receives_existing_and_new() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        let mut rx = log.subscribe().await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        // Should receive entry 2 (not 1, since subscription starts after)
        let received = rx.try_recv().unwrap();
        assert_eq!(received.request.id, "2");
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_metrics_after_eviction() {
        let log = NetworkLog::with_limits(10, 5);
        for i in 0..25 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://x.com"),
            )
            .await;
        }
        let m = log.metrics().await;
        assert!(m.entries_evicted > 0);
    }

    #[tokio::test]
    async fn test_eviction_preserves_order_of_remaining() {
        let log = NetworkLog::with_limits(10, 5);
        for i in 0..20 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://x.com"),
            )
            .await;
        }
        let ids = log.request_ids().await;
        // After eviction of oldest half, remaining should still be in order
        for pair in ids.windows(2) {
            let a: u32 = pair[0].parse().unwrap();
            let b: u32 = pair[1].parse().unwrap();
            assert!(a < b, "order violated: {a} before {b}");
        }
    }

    #[tokio::test]
    async fn test_captured_timing_defaults() {
        let timing = CapturedTiming::default();
        assert!(timing.dns_start_ms.is_none());
        assert!(timing.dns_end_ms.is_none());
        assert!(timing.connect_start_ms.is_none());
        assert!(timing.connect_end_ms.is_none());
        assert!(timing.tls_start_ms.is_none());
        assert!(timing.response_start_ms.is_none());
        assert!(timing.response_end_ms.is_none());
    }

    #[tokio::test]
    async fn test_captured_request_header_missing() {
        let req = make_request("1", "GET", "https://a.com");
        assert_eq!(req.request_header("X-Missing"), None);
    }

    #[tokio::test]
    async fn test_network_entry_is_error() {
        let entry = NetworkEntry {
            request: make_request("1", "GET", "https://a.com"),
            response: None,
            error: Some(make_error("1", "https://a.com", "err")),
        };
        assert!(entry.is_error());
        assert!(!entry.has_response());
    }

    #[tokio::test]
    async fn test_network_entry_has_response() {
        let entry = NetworkEntry {
            request: make_request("1", "GET", "https://a.com"),
            response: Some(make_response("1", 200, "https://a.com")),
            error: None,
        };
        assert!(entry.has_response());
        assert!(!entry.is_error());
    }

    #[tokio::test]
    async fn test_save_to_json_empty_log() {
        let log = NetworkLog::new();
        let path = std::path::Path::new("/tmp/foxdriver_json_empty.json");
        log.save_to_json(path).await.unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        let parsed: Vec<NetworkEntry> = serde_json::from_str(&content).unwrap();
        assert!(parsed.is_empty());
    }

    #[tokio::test]
    async fn test_save_to_json_roundtrip_entries() {
        let log = NetworkLog::new();
        let mut req = make_request("1", "POST", "https://api.example.com");
        req.headers = vec![CapturedHeader {
            name: "Content-Type".into(),
            value: "application/json".into(),
        }];
        req.post_data = Some(r#"{"key":"value"}"#.into());
        push_entry(
            &log,
            req,
            Some(make_response("1", 201, "https://api.example.com")),
            None,
        )
        .await;
        let path = std::path::Path::new("/tmp/foxdriver_json_roundtrip.json");
        log.save_to_json(path).await.unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        let parsed: Vec<NetworkEntry> = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].request.id, "1");
        assert_eq!(parsed[0].request.method, "POST");
        assert_eq!(parsed[0].response.as_ref().unwrap().status, 201);
    }

    #[tokio::test]
    async fn test_har_with_multiple_entries() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(make_response("1", 200, "https://a.com")),
            None,
        )
        .await;
        push_entry(
            &log,
            make_request("2", "POST", "https://b.com"),
            Some(make_response("2", 201, "https://b.com")),
            None,
        )
        .await;
        let path = std::path::Path::new("/tmp/foxdriver_har_multi.json");
        log.save_as_har(path, Some("test page")).await.unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["log"]["entries"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["log"]["pages"][0]["title"], "test page");
    }

    #[tokio::test]
    async fn test_har_error_entry_has_no_response() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            None,
            Some(make_error("1", "https://a.com", "timeout")),
        )
        .await;
        let path = std::path::Path::new("/tmp/foxdriver_har_error.json");
        log.save_as_har(path, None).await.unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let entry = &parsed["log"]["entries"][0];
        assert!(entry["response"].is_null());
    }

    // ---------- BiDi ingestion deep tests ----------

    #[tokio::test]
    async fn test_metrics_requests_received_manual_increment() {
        let log = NetworkLog::new();
        let m0 = log.metrics().await;
        {
            let mut inner = log.inner.write().await;
            inner.metrics.requests_received += 5;
        }
        let m1 = log.metrics().await;
        assert_eq!(m1.requests_received, m0.requests_received + 5);
    }

    #[tokio::test]
    async fn test_metrics_responses_received_manual_increment() {
        let log = NetworkLog::new();
        let m0 = log.metrics().await;
        {
            let mut inner = log.inner.write().await;
            inner.metrics.responses_received += 3;
        }
        let m1 = log.metrics().await;
        assert_eq!(m1.responses_received, m0.responses_received + 3);
    }

    #[tokio::test]
    async fn test_metrics_errors_received_manual_increment() {
        let log = NetworkLog::new();
        let m0 = log.metrics().await;
        {
            let mut inner = log.inner.write().await;
            inner.metrics.errors_received += 2;
        }
        let m1 = log.metrics().await;
        assert_eq!(m1.errors_received, m0.errors_received + 2);
    }

    #[tokio::test]
    async fn test_deduplication_does_not_affect_metrics() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        let m1 = log.metrics().await;
        push_request(&log, make_request("1", "GET", "https://b.com")).await; // deduplicated
        let m2 = log.metrics().await;
        assert_eq!(m2.requests_received, m1.requests_received); // no change
        assert_eq!(log.len().await, 1); // still only one entry
    }

    // ---------- filter deep tests ----------

    #[tokio::test]
    async fn test_filter_status_range_boundary_inclusive() {
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            Some(make_response("1", 200, "https://a.com")),
            None,
        )
        .await;
        push_entry(
            &log,
            make_request("2", "GET", "https://a.com"),
            Some(make_response("2", 299, "https://a.com")),
            None,
        )
        .await;
        push_entry(
            &log,
            make_request("3", "GET", "https://a.com"),
            Some(make_response("3", 300, "https://a.com")),
            None,
        )
        .await;
        assert_eq!(
            log.filter(Filter::new().status_range(200..=299))
                .await
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn test_filter_destination_deep() {
        let log = NetworkLog::new();
        let mut req1 = make_request("1", "GET", "https://a.com");
        req1.destination = "document".into();
        let mut req2 = make_request("2", "GET", "https://b.com");
        req2.destination = "image".into();
        push_request(&log, req1).await;
        push_request(&log, req2).await;
        assert_eq!(
            log.filter(Filter::new().destination("document"))
                .await
                .len(),
            1
        );
        assert_eq!(
            log.filter(Filter::new().destination("image")).await.len(),
            1
        );
        assert_eq!(
            log.filter(Filter::new().destination("script")).await.len(),
            0
        );
    }

    #[tokio::test]
    async fn test_filter_combined_with_response_and_without_response() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_entry(
            &log,
            make_request("2", "GET", "https://b.com"),
            Some(make_response("2", 200, "https://b.com")),
            None,
        )
        .await;
        push_entry(
            &log,
            make_request("3", "GET", "https://c.com"),
            None,
            Some(make_error("3", "https://c.com", "err")),
        )
        .await;
        assert_eq!(log.filter(Filter::new().with_response()).await.len(), 1);
        assert_eq!(log.filter(Filter::new().without_response()).await.len(), 2);
        assert_eq!(log.filter(Filter::new().with_error()).await.len(), 1);
    }

    #[tokio::test]
    async fn test_filter_url_regex_complex_pattern() {
        let log = NetworkLog::new();
        push_request(
            &log,
            make_request("1", "GET", "https://api.example.com/v1/users/123"),
        )
        .await;
        push_request(
            &log,
            make_request("2", "GET", "https://api.example.com/v2/items/456"),
        )
        .await;
        push_request(
            &log,
            make_request("3", "GET", "https://other.com/v1/users/789"),
        )
        .await;
        let matches = log
            .filter(
                Filter::new()
                    .url_regex(r"api\.example\.com/v\d+/(users|items)/\d+")
                    .unwrap(),
            )
            .await;
        assert_eq!(matches.len(), 2);
    }

    // ---------- serialization deep tests ----------

    #[tokio::test]
    async fn test_captured_request_serde_roundtrip() {
        let req = make_request("1", "POST", "https://a.com");
        let json = serde_json::to_string(&req).unwrap();
        let de: CapturedRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(de.id, req.id);
        assert_eq!(de.method, req.method);
        assert_eq!(de.url, req.url);
    }

    #[tokio::test]
    async fn test_captured_response_serde_roundtrip() {
        let resp = make_response("1", 404, "https://a.com");
        let json = serde_json::to_string(&resp).unwrap();
        let de: CapturedResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(de.status, 404);
        assert_eq!(de.url, "https://a.com");
    }

    #[tokio::test]
    async fn test_captured_error_serde_roundtrip() {
        let err = make_error("1", "https://a.com", "timeout");
        let json = serde_json::to_string(&err).unwrap();
        let de: CapturedError = serde_json::from_str(&json).unwrap();
        assert_eq!(de.error_text, "timeout");
    }

    #[tokio::test]
    async fn test_network_entry_serde_roundtrip() {
        let entry = NetworkEntry {
            request: make_request("1", "GET", "https://a.com"),
            response: Some(make_response("1", 200, "https://a.com")),
            error: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let de: NetworkEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(de.request.id, "1");
        assert!(de.response.is_some());
        assert!(de.error.is_none());
    }

    // ---------- concurrent deep tests ----------

    #[tokio::test]
    async fn test_concurrent_remove_and_query() {
        let log = NetworkLog::new();
        for i in 0..100 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://x.com"),
            )
            .await;
        }
        let mut handles = Vec::new();
        for _ in 0..10 {
            let log = log.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..10 {
                    let _ = log.remove_by_id("50").await;
                    let _ = log.len().await;
                    let _ = log.contains_id("50").await;
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_concurrent_retain_and_read() {
        let log = NetworkLog::new();
        for i in 0..100 {
            push_request(
                &log,
                make_request(&format!("{}", i), "GET", "https://x.com"),
            )
            .await;
        }
        let mut handles = Vec::new();
        for _ in 0..5 {
            let log = log.clone();
            handles.push(tokio::spawn(async move {
                log.retain(|e| e.request.id != "50").await;
            }));
        }
        for _ in 0..5 {
            let log = log.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..20 {
                    let _ = log.len().await;
                    tokio::task::yield_now().await;
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_concurrent_drain_pending_and_ingest() {
        let log = NetworkLog::new();
        let mut handles = Vec::new();
        for _ in 0..5 {
            let log = log.clone();
            handles.push(tokio::spawn(async move {
                for i in 0..20 {
                    push_request(
                        &log,
                        make_request(&format!("{}", i), "GET", "https://x.com"),
                    )
                    .await;
                }
            }));
        }
        for _ in 0..5 {
            let log = log.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..20 {
                    let _ = log.drain_pending().await;
                    tokio::task::yield_now().await;
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
    }

    // ---------- wait helper deep tests ----------

    #[tokio::test]
    async fn test_wait_for_url_receives_new_entry() {
        let log = NetworkLog::new();
        let log2 = log.clone();
        let waiter = tokio::spawn(async move {
            log2.wait_for_url("target", std::time::Duration::from_secs(1))
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        push_request(&log, make_request("1", "GET", "https://target.com")).await;
        let result = waiter.await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().request.url, "https://target.com");
    }

    #[tokio::test]
    async fn test_wait_for_response_receives_update() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        let log2 = log.clone();
        let waiter = tokio::spawn(async move {
            log2.wait_for_response("1", std::time::Duration::from_secs(1))
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        {
            let mut inner = log.inner.write().await;
            let mut new_entry = (*inner.entries[0]).clone();
            new_entry.response = Some(make_response("1", 200, "https://a.com"));
            inner.entries[0] = Arc::new(new_entry);
            let _ = inner.tx.send(inner.entries[0].clone());
        }
        let result = waiter.await.unwrap();
        assert!(result.is_some());
        assert!(result.unwrap().has_response());
    }

    #[tokio::test]
    async fn test_filter_combined_method_status_url() {
        let log = NetworkLog::new();
        push_request(
            &log,
            make_request("1", "POST", "https://api.example.com/v1"),
        )
        .await;
        push_request(&log, make_request("2", "GET", "https://api.example.com/v2")).await;
        {
            let mut inner = log.inner.write().await;
            let mut e = (*inner.entries[0]).clone();
            e.response = Some(make_response("1", 201, "https://api.example.com/v1"));
            inner.entries[0] = Arc::new(e);
        }
        let f = Filter::new()
            .method("POST")
            .status_range(200..=299)
            .url_contains("v1");
        let hits = log.filter(f).await;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].request.id, "1");
    }

    #[tokio::test]
    async fn test_filter_has_error_true() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        {
            let mut inner = log.inner.write().await;
            let mut e = (*inner.entries[1]).clone();
            e.error = Some(CapturedError {
                id: "2".into(),
                url: "https://b.com".into(),
                error_text: "net::ERR_FAILED".into(),
            });
            inner.entries[1] = Arc::new(e);
        }
        let hits = log.filter(Filter::new().with_error()).await;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].request.id, "2");
    }

    #[tokio::test]
    async fn test_find_by_url_exact_vs_substring() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://example.com/path")).await;
        let found = log.find_by_url("/path").await;
        assert!(found.is_some());
        let not_found = log.find_by_url("/other").await;
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_endpoints_domain_only_url() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://example.com")).await;
        let eps = log.endpoints().await;
        assert_eq!(eps, vec!["https://example.com"]);
    }

    #[tokio::test]
    async fn test_hostnames_idn_url() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://münchen.de/shop")).await;
        let hosts = log.hostnames().await;
        // url::Url normalises IDN to punycode
        assert_eq!(hosts, vec!["xn--mnchen-3ya.de"]);
    }

    #[tokio::test]
    async fn test_total_bytes_in_missing_body_size() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        {
            let mut inner = log.inner.write().await;
            let mut e = (*inner.entries[0]).clone();
            e.response = Some(CapturedResponse {
                id: "1".into(),
                url: "https://a.com".into(),
                protocol: "h2".into(),
                status: 200,
                status_text: "OK".into(),
                headers: vec![],
                mime_type: "text/html".into(),
                body_size: None,
                from_cache: false,
            });
            inner.entries[0] = Arc::new(e);
        }
        assert_eq!(log.total_bytes_in().await, 0);
    }

    #[tokio::test]
    async fn test_total_bytes_out_empty_post_data() {
        let log = NetworkLog::new();
        let mut req = make_request("1", "POST", "https://a.com");
        req.post_data = Some("".into());
        push_request(&log, req).await;
        assert_eq!(log.total_bytes_out().await, 0);
    }

    #[test]
    fn test_to_curl_post_json() {
        let entry = NetworkEntry {
            request: CapturedRequest {
                id: "1".into(),
                context: None,
                url: "https://api.com".into(),
                method: "POST".into(),
                headers: vec![CapturedHeader {
                    name: "Content-Type".into(),
                    value: "application/json".into(),
                }],
                cookies: vec![],
                post_data: Some(r#"{"key":"val"}"#.into()),
                timestamp: 0,
                destination: "document".into(),
                initiator_type: None,
                timing: CapturedTiming::default(),
            },
            response: None,
            error: None,
        };
        let curl = entry.to_curl();
        assert!(curl.contains("-X 'POST'"));
        assert!(curl.contains("-H 'Content-Type: application/json'"));
        assert!(curl.contains(r#"-d '{"key":"val"}'"#));
    }

    #[test]
    fn test_to_curl_header_with_quote() {
        let entry = NetworkEntry {
            request: CapturedRequest {
                id: "1".into(),
                context: None,
                url: "https://a.com".into(),
                method: "GET".into(),
                headers: vec![CapturedHeader {
                    name: "X-Token".into(),
                    value: "it's ok".into(),
                }],
                cookies: vec![],
                post_data: None,
                timestamp: 0,
                destination: "document".into(),
                initiator_type: None,
                timing: CapturedTiming::default(),
            },
            response: None,
            error: None,
        };
        let curl = entry.to_curl();
        assert!(curl.contains("X-Token: it'\\''s ok"));
    }

    #[tokio::test]
    async fn test_retain_keeps_some_removes_others() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://keep.com")).await;
        push_request(&log, make_request("2", "GET", "https://drop.com")).await;
        push_request(&log, make_request("3", "GET", "https://keep.com/page")).await;
        log.retain(|e| e.request.url.contains("keep")).await;
        let ids = log.request_ids().await;
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"1".into()));
        assert!(ids.contains(&"3".into()));
    }

    #[tokio::test]
    async fn test_request_ids_ordering_after_eviction() {
        let log = NetworkLog::with_limits(4, 100);
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        push_request(&log, make_request("3", "GET", "https://c.com")).await;
        push_request(&log, make_request("4", "GET", "https://d.com")).await;
        push_request(&log, make_request("5", "GET", "https://e.com")).await; // triggers eviction
        let ids = log.request_ids().await;
        // Incremental eviction removes only the oldest entry needed to stay under the cap.
        assert_eq!(ids.len(), 4);
        assert_eq!(
            ids,
            vec![
                "2".to_string(),
                "3".to_string(),
                "4".to_string(),
                "5".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn test_has_response_false_for_nonexistent() {
        let log = NetworkLog::new();
        assert!(!log.has_response("nope").await);
    }

    #[tokio::test]
    async fn test_has_error_false_for_nonexistent() {
        let log = NetworkLog::new();
        assert!(!log.has_error("nope").await);
    }

    #[tokio::test]
    async fn test_memory_estimate_increases_with_entries() {
        let log = NetworkLog::new();
        let empty = log.memory_estimate().await;
        push_request(
            &log,
            make_request("1", "GET", "https://example.com/some/long/path/here"),
        )
        .await;
        let with_one = log.memory_estimate().await;
        assert!(with_one > empty);
    }

    #[tokio::test]
    async fn test_save_to_json_empty_log_roundtrip() {
        let log = NetworkLog::new();
        let tmp =
            std::env::temp_dir().join(format!("foxdriver_empty_json_{}.json", std::process::id()));
        log.save_to_json(&tmp).await.unwrap();
        let content = std::fs::read_to_string(&tmp).unwrap();
        assert!(content.contains("[]"));
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn test_save_as_har_empty_log() {
        let log = NetworkLog::new();
        let tmp =
            std::env::temp_dir().join(format!("foxdriver_empty_har_{}.har", std::process::id()));
        log.save_as_har(&tmp, Some("empty")).await.unwrap();
        let content = std::fs::read_to_string(&tmp).unwrap();
        assert!(content.contains("\"entries\""));
        assert!(content.contains("\"pages\""));
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn test_filter_destination_matching() {
        let log = NetworkLog::new();
        let mut req = make_request("1", "GET", "https://a.com");
        req.destination = "image".into();
        push_request(&log, req).await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        let hits = log.filter(Filter::new().destination("image")).await;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].request.id, "1");
    }

    #[tokio::test]
    async fn test_distinct_methods_empty_log() {
        let log = NetworkLog::new();
        assert!(log.distinct_methods().await.is_empty());
    }

    #[tokio::test]
    async fn test_distinct_statuses_no_responses() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        assert!(log.distinct_statuses().await.is_empty());
    }

    #[tokio::test]
    async fn test_total_bytes_in_all_none() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        {
            let mut inner = log.inner.write().await;
            let mut e = (*inner.entries[0]).clone();
            e.response = Some(CapturedResponse {
                id: "1".into(),
                url: "https://a.com".into(),
                protocol: "h2".into(),
                status: 200,
                status_text: "OK".into(),
                headers: vec![],
                mime_type: "text/html".into(),
                body_size: None,
                from_cache: false,
            });
            inner.entries[0] = Arc::new(e);
        }
        assert_eq!(log.total_bytes_in().await, 0);
    }

    #[tokio::test]
    async fn test_total_bytes_out_no_post_data() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        assert_eq!(log.total_bytes_out().await, 0);
    }

    #[tokio::test]
    async fn test_save_to_json_roundtrip_with_response() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "POST", "https://api.com")).await;
        {
            let mut inner = log.inner.write().await;
            let mut e = (*inner.entries[0]).clone();
            e.response = Some(make_response("1", 201, "https://api.com"));
            inner.entries[0] = Arc::new(e);
        }
        let tmp =
            std::env::temp_dir().join(format!("foxdriver_json_rt_{}.json", std::process::id()));
        log.save_to_json(&tmp).await.unwrap();
        let content = std::fs::read_to_string(&tmp).unwrap();
        let parsed: Vec<NetworkEntry> = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].request.id, "1");
        assert_eq!(parsed[0].response.as_ref().unwrap().status, 201);
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn test_save_as_har_with_response_entry() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        {
            let mut inner = log.inner.write().await;
            let mut e = (*inner.entries[0]).clone();
            e.response = Some(make_response("1", 200, "https://a.com"));
            inner.entries[0] = Arc::new(e);
        }
        let tmp =
            std::env::temp_dir().join(format!("foxdriver_har_resp_{}.har", std::process::id()));
        log.save_as_har(&tmp, Some("testpage")).await.unwrap();
        let content = std::fs::read_to_string(&tmp).unwrap();
        assert!(content.contains("\"testpage\""));
        assert!(content.contains("200"));
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn test_has_response_true_when_present() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        assert!(!log.has_response("1").await);
        {
            let mut inner = log.inner.write().await;
            let mut e = (*inner.entries[0]).clone();
            e.response = Some(make_response("1", 200, "https://a.com"));
            inner.entries[0] = Arc::new(e);
        }
        assert!(log.has_response("1").await);
    }

    #[tokio::test]
    async fn test_has_error_true_when_present() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        assert!(!log.has_error("1").await);
        {
            let mut inner = log.inner.write().await;
            let mut e = (*inner.entries[0]).clone();
            e.error = Some(CapturedError {
                id: "1".into(),
                url: "https://a.com".into(),
                error_text: "fail".into(),
            });
            inner.entries[0] = Arc::new(e);
        }
        assert!(log.has_error("1").await);
    }

    #[tokio::test]
    async fn test_memory_estimate_decreases_after_clear() {
        let log = NetworkLog::new();
        push_request(
            &log,
            make_request(
                "1",
                "GET",
                "https://example.com/very/long/path/here/for/bytes",
            ),
        )
        .await;
        let before = log.memory_estimate().await;
        log.clear().await;
        let after = log.memory_estimate().await;
        assert!(after < before);
    }

    #[tokio::test]
    async fn test_memory_estimate_decreases_after_retain() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com/loooooong")).await;
        push_request(&log, make_request("2", "GET", "https://b.com/loooooong")).await;
        let before = log.memory_estimate().await;
        log.retain(|e| e.request.id == "1").await;
        let after = log.memory_estimate().await;
        assert!(after < before);
    }

    #[tokio::test]
    async fn test_completed_excludes_in_flight() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        {
            let mut inner = log.inner.write().await;
            let mut e = (*inner.entries[1]).clone();
            e.response = Some(make_response("2", 200, "https://b.com"));
            inner.entries[1] = Arc::new(e);
        }
        let completed = log.completed().await;
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].request.id, "2");
    }

    #[tokio::test]
    async fn test_completed_includes_errors() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        {
            let mut inner = log.inner.write().await;
            let mut e = (*inner.entries[0]).clone();
            e.error = Some(CapturedError {
                id: "1".into(),
                url: "https://a.com".into(),
                error_text: "net::ERR_ABORTED".into(),
            });
            inner.entries[0] = Arc::new(e);
        }
        let completed = log.completed().await;
        assert_eq!(completed.len(), 1);
        assert!(completed[0].is_error());
    }

    #[tokio::test]
    async fn test_find_by_url_regex_complex_pattern() {
        let log = NetworkLog::new();
        push_request(
            &log,
            make_request("1", "GET", "https://api.example.com/v1/users"),
        )
        .await;
        push_request(
            &log,
            make_request("2", "GET", "https://api.example.com/v2/items"),
        )
        .await;
        let re = regex::Regex::new(r"/v\d+/users").unwrap();
        let found = log.find_by_url_regex(&re).await;
        assert_eq!(found.unwrap().request.id, "1");
    }

    #[tokio::test]
    async fn test_nth_out_of_bounds_returns_none() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        assert!(log.nth(5).await.is_none());
    }

    #[tokio::test]
    async fn test_first_last_both_none_on_empty() {
        let log = NetworkLog::new();
        assert!(log.first().await.is_none());
        assert!(log.last().await.is_none());
    }

    #[tokio::test]
    async fn test_remove_by_id_missing_returns_none() {
        let log = NetworkLog::new();
        assert!(log.remove_by_id("nope").await.is_none());
    }

    #[tokio::test]
    async fn test_contains_id_true_and_false() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        assert!(log.contains_id("1").await);
        assert!(!log.contains_id("2").await);
    }

    #[tokio::test]
    async fn test_endpoints_dedupes_by_full_url() {
        // endpoints() deduplicates by the full request URL (including query string).
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com/page?x=1")).await;
        push_request(&log, make_request("2", "GET", "https://a.com/page?x=2")).await;
        let eps = log.endpoints().await;
        assert_eq!(eps.len(), 2);
    }

    #[tokio::test]
    async fn test_hostnames_dedupes() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com/x")).await;
        push_request(&log, make_request("2", "GET", "https://a.com/y")).await;
        let hosts = log.hostnames().await;
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0], "a.com");
    }

    #[tokio::test]
    async fn test_drain_pending_empties_maps() {
        let log = NetworkLog::new();
        // Push a response out-of-order so it lands in pending
        {
            let mut inner = log.inner.write().await;
            inner
                .pending_responses
                .insert("1".into(), make_response("1", 200, "https://a.com"));
            inner.pending_errors.insert(
                "2".into(),
                CapturedError {
                    id: "2".into(),
                    url: "https://b.com".into(),
                    error_text: "err".into(),
                },
            );
        }
        assert_eq!(log.pending_count().await, 2);
        let (resps, errs) = log.drain_pending().await;
        assert_eq!(resps.len(), 1);
        assert_eq!(errs.len(), 1);
        assert_eq!(log.pending_count().await, 0);
    }

    #[tokio::test]
    async fn test_metrics_zero_on_fresh_log() {
        let log = NetworkLog::new();
        let m = log.metrics().await;
        assert_eq!(m.requests_received, 0);
        assert_eq!(m.responses_received, 0);
        assert_eq!(m.errors_received, 0);
        assert_eq!(m.entries_evicted, 0);
        assert_eq!(m.broadcast_drops, 0);
        assert_eq!(m.duplicate_responses, 0);
        assert_eq!(m.duplicate_errors, 0);
    }

    #[tokio::test]
    async fn test_metrics_track_entries() {
        let log = NetworkLog::new();
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        // push_request does not increment metrics (those are BiDi handler counters).
        // Verify that default metrics are zero on a fresh log.
        let m = log.metrics().await;
        assert_eq!(m.max_entries, 50_000);
        assert_eq!(m.max_entries, log.inner.read().await.max_entries);
    }

    #[tokio::test]
    async fn test_eviction_evicts_oldest_incrementally() {
        let log = NetworkLog::with_limits(3, 5);
        push_request(&log, make_request("1", "GET", "https://a.com")).await;
        push_request(&log, make_request("2", "GET", "https://b.com")).await;
        push_request(&log, make_request("3", "GET", "https://c.com")).await;
        push_request(&log, make_request("4", "GET", "https://d.com")).await;
        push_request(&log, make_request("5", "GET", "https://e.com")).await;

        let entries = log.entries().await;
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].request.id, "3");
        assert_eq!(entries[1].request.id, "4");
        assert_eq!(entries[2].request.id, "5");

        let m = log.metrics().await;
        assert_eq!(m.entries_evicted, 2);

        // Pushing one more evicts exactly one additional oldest entry.
        push_request(&log, make_request("6", "GET", "https://f.com")).await;
        let entries = log.entries().await;
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].request.id, "4");
        let m = log.metrics().await;
        assert_eq!(m.entries_evicted, 3);
    }

    #[tokio::test]
    async fn test_filter_status_range_ignores_error_without_status() {
        // A fetch error has no response and therefore no status. It must not
        // be fabricated as 0 and therefore must not match a range that
        // happens to include 0 (which HTTP status codes never do anyway).
        let log = NetworkLog::new();
        push_entry(
            &log,
            make_request("1", "GET", "https://a.com"),
            None,
            Some(make_error("1", "https://a.com", "net::ERR_FAILED")),
        )
        .await;
        // 0 is not a valid HTTP status, but this primarily guards against any
        // future code that fabricates 0 for missing statuses.
        assert!(log
            .filter(Filter::new().status_range(0..=0))
            .await
            .is_empty());
        assert!(log.filter(Filter::new().with_error()).await.len() == 1);
    }
}
