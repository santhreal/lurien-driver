//! What happens to a request before it reaches the network.
//!
//! A route is applied by the engine on the channel, in the parent process,
//! before the request is sent: a page cannot cancel its own request, cannot see
//! the headers that will go out, and cannot be handed a response that never
//! left the browser. This module owns the shape of a route and the order routes
//! are tried in; the engine owns the matching and the channel.
//!
//! The most recently added route is tried first. A caller narrows behaviour by
//! adding a route, never by having to remove one, and `route` reports the table
//! in the order it is tried so precedence is readable rather than remembered.

use crate::error::Error;
use std::collections::BTreeMap;

/// Longest body a route may serve. The engine holds the same limit.
pub const MAX_BODY: usize = 8 * 1024 * 1024;

/// What a route does with the request it matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Answer from the browser; the request is never sent.
    Fulfil,
    /// Cancel the request, which the page sees as a network error.
    Abort,
    /// Send it, with the header edits this route names.
    Continue,
}

impl Action {
    /// The name the engine and every face use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fulfil => "fulfil",
            Self::Abort => "abort",
            Self::Continue => "continue",
        }
    }
}

/// One route: a pattern and what to do with what it matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// URL glob: `*` matches any run of characters, `?` matches one, and a
    /// pattern with neither matches one URL exactly.
    pub pattern: String,
    /// What happens to a matched request.
    pub action: Action,
    /// Response status, for a fulfil route.
    pub status: u16,
    /// Response reason phrase, for a fulfil route. Empty means the default.
    pub status_text: String,
    /// Response headers for a fulfil route, request headers otherwise.
    pub headers: BTreeMap<String, String>,
    /// Request headers to drop. Not for a fulfil route.
    pub remove: Vec<String>,
    /// Response body, for a fulfil route.
    pub body: String,
}

impl Route {
    /// Answer this pattern from the browser.
    ///
    /// # Errors
    ///
    /// [`Error::BadArgs`] when the pattern is empty, the status is not a real
    /// HTTP status, or the body is over [`MAX_BODY`].
    pub fn fulfil(
        pattern: &str,
        status: i64,
        status_text: &str,
        headers: BTreeMap<String, String>,
        body: &str,
    ) -> Result<Self, Error> {
        let pattern = checked_pattern("route-fulfil", pattern)?;
        let status = u16::try_from(status)
            .ok()
            .filter(|code| (100..=599).contains(code))
            .ok_or_else(|| {
                refused(
                    "route-fulfil",
                    &format!("status {status} is not 100 to 599. Use 200, or the status the real backend returns"),
                )
            })?;
        if body.len() > MAX_BODY {
            return Err(refused(
                "route-fulfil",
                &format!(
                    "a body of {} bytes is over the {MAX_BODY} byte limit. Use a smaller body, or point the route at a file the page can fetch",
                    body.len()
                ),
            ));
        }
        Ok(Self {
            pattern,
            action: Action::Fulfil,
            status,
            status_text: status_text.to_string(),
            headers,
            remove: Vec::new(),
            body: body.to_string(),
        })
    }

    /// Cancel every request this pattern matches.
    ///
    /// # Errors
    ///
    /// [`Error::BadArgs`] when the pattern is empty.
    pub fn abort(pattern: &str) -> Result<Self, Error> {
        Ok(Self {
            pattern: checked_pattern("route-abort", pattern)?,
            action: Action::Abort,
            status: 0,
            status_text: String::new(),
            headers: BTreeMap::new(),
            remove: Vec::new(),
            body: String::new(),
        })
    }

    /// Send it, with header edits.
    ///
    /// # Errors
    ///
    /// [`Error::BadArgs`] when the pattern is empty or the route edits nothing,
    /// which would be a route that matches requests and then does nothing to
    /// them while shadowing every route added before it.
    pub fn cont(
        pattern: &str,
        headers: BTreeMap<String, String>,
        remove: Vec<String>,
    ) -> Result<Self, Error> {
        let pattern = checked_pattern("route-continue", pattern)?;
        if headers.is_empty() && remove.is_empty() {
            return Err(refused(
                "route-continue",
                "the route changes no header. Use headers to set one, remove to drop one, or route-abort to stop the request",
            ));
        }
        Ok(Self {
            pattern,
            action: Action::Continue,
            status: 0,
            status_text: String::new(),
            headers,
            remove,
            body: String::new(),
        })
    }

    /// The route as the control channel carries it.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mut value = serde_json::json!({
            "pattern": self.pattern,
            "action": self.action.as_str(),
        });
        let map = value.as_object_mut().expect("object");
        if !self.headers.is_empty() {
            map.insert(
                "headers".to_string(),
                serde_json::Value::Object(
                    self.headers
                        .iter()
                        .map(|(name, value)| {
                            (name.clone(), serde_json::Value::String(value.clone()))
                        })
                        .collect(),
                ),
            );
        }
        if !self.remove.is_empty() {
            map.insert("remove".to_string(), serde_json::json!(self.remove));
        }
        if self.action == Action::Fulfil {
            map.insert("status".to_string(), serde_json::json!(self.status));
            if !self.status_text.is_empty() {
                map.insert(
                    "status_text".to_string(),
                    serde_json::json!(self.status_text),
                );
            }
            map.insert("body".to_string(), serde_json::json!(self.body));
        }
        value
    }

    /// The route as a face reports it. Never the body: a route can serve a
    /// megabyte and a report is read by a human.
    #[must_use]
    pub fn row(&self) -> serde_json::Value {
        let mut value = serde_json::json!({
            "pattern": self.pattern,
            "action": self.action.as_str(),
        });
        let map = value.as_object_mut().expect("object");
        if self.action == Action::Fulfil {
            map.insert("status".to_string(), serde_json::json!(self.status));
            map.insert("body_bytes".to_string(), serde_json::json!(self.body.len()));
        }
        if !self.headers.is_empty() {
            map.insert(
                "headers".to_string(),
                serde_json::json!(self.headers.keys().collect::<Vec<_>>()),
            );
        }
        if !self.remove.is_empty() {
            map.insert("remove".to_string(), serde_json::json!(self.remove));
        }
        value
    }
}

/// The whole table as the control channel carries it, in match order.
#[must_use]
pub fn table_json(routes: &[Route]) -> String {
    let rows: Vec<serde_json::Value> = routes.iter().map(Route::to_json).collect();
    serde_json::json!({ "routes": rows }).to_string()
}

/// Read a JSON object of headers, the way every face passes them.
///
/// # Errors
///
/// [`Error::BadArgs`] when the text is not a JSON object of strings.
pub fn parse_headers(verb: &'static str, text: &str) -> Result<BTreeMap<String, String>, Error> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(BTreeMap::new());
    }
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| {
        refused(
            verb,
            &format!(
                "headers {text:?} is not JSON: {e}. Use an object like {{\"X-Trace\":\"1\"}}"
            ),
        )
    })?;
    let object = value.as_object().ok_or_else(|| {
        refused(
            verb,
            &format!("headers {text:?} is not an object. Use an object like {{\"X-Trace\":\"1\"}}"),
        )
    })?;
    let mut headers = BTreeMap::new();
    for (name, value) in object {
        let text = value.as_str().map(str::to_string).unwrap_or_else(|| match value {
            serde_json::Value::Number(number) => number.to_string(),
            serde_json::Value::Bool(flag) => flag.to_string(),
            other => other.to_string(),
        });
        if name.trim().is_empty() {
            return Err(refused(verb, "a header with no name cannot be sent. Use a header name"));
        }
        headers.insert(name.clone(), text);
    }
    Ok(headers)
}

fn checked_pattern(verb: &'static str, pattern: &str) -> Result<String, Error> {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return Err(refused(
            verb,
            "a route needs a pattern. Use a URL glob like https://api.example.com/*",
        ));
    }
    Ok(pattern.to_string())
}

fn refused(verb: &'static str, detail: &str) -> Error {
    Error::BadArgs {
        verb: verb.to_string(),
        detail: detail.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn a_fulfil_route_carries_everything_the_engine_needs() {
        let route = Route::fulfil(
            "https://api.example.com/*",
            201,
            "Created",
            headers(&[("Content-Type", "application/json")]),
            "{\"ok\":true}",
        )
        .unwrap();
        let json = route.to_json();
        assert_eq!(json["action"], "fulfil");
        assert_eq!(json["status"], 201);
        assert_eq!(json["status_text"], "Created");
        assert_eq!(json["body"], "{\"ok\":true}");
        assert_eq!(json["headers"]["Content-Type"], "application/json");
        // A report names the body's size, never the body.
        let row = route.row();
        assert_eq!(row["body_bytes"], 11);
        assert!(row.get("body").is_none());
    }

    #[test]
    fn an_abort_route_and_a_continue_route_carry_only_what_they_use() {
        let abort = Route::abort("*/track").unwrap().to_json();
        assert_eq!(abort["action"], "abort");
        assert!(abort.get("status").is_none(), "{abort}");
        assert!(abort.get("body").is_none(), "{abort}");

        let cont = Route::cont(
            "*",
            headers(&[("X-Trace", "1")]),
            vec!["Referer".to_string()],
        )
        .unwrap()
        .to_json();
        assert_eq!(cont["action"], "continue");
        assert_eq!(cont["headers"]["X-Trace"], "1");
        assert_eq!(cont["remove"][0], "Referer");
        assert!(cont.get("body").is_none(), "{cont}");
    }

    #[test]
    fn a_route_that_cannot_work_is_refused_with_the_fix() {
        let cases: Vec<(Error, &str)> = vec![
            (Route::fulfil("", 200, "", BTreeMap::new(), "").unwrap_err(), "URL glob"),
            (
                Route::fulfil("*", 999, "", BTreeMap::new(), "").unwrap_err(),
                "100 to 599",
            ),
            (Route::abort("   ").unwrap_err(), "URL glob"),
            (
                Route::cont("*", BTreeMap::new(), Vec::new()).unwrap_err(),
                "route-abort",
            ),
        ];
        for (error, expected) in cases {
            let text = error.to_string();
            assert!(text.contains(expected), "{text:?} does not name {expected:?}");
        }
    }

    #[test]
    fn a_body_over_the_limit_is_refused_before_it_reaches_the_engine() {
        let body = "x".repeat(MAX_BODY + 1);
        let refused = Route::fulfil("*", 200, "", BTreeMap::new(), &body)
            .unwrap_err()
            .to_string();
        assert!(refused.contains("over the"), "{refused}");
    }

    #[test]
    fn headers_are_read_from_the_shape_every_face_passes() {
        assert!(parse_headers("route-continue", "").unwrap().is_empty());
        let parsed = parse_headers("route-continue", "{\"X-A\":\"1\",\"X-B\":2}").unwrap();
        assert_eq!(parsed["X-A"], "1");
        // A number is a header value spelled without quotes, not an error: a
        // JSON client that sends 2 means the string 2.
        assert_eq!(parsed["X-B"], "2");
        for bad in ["[1]", "{", "\"X-A\""] {
            let refused = parse_headers("route-continue", bad).unwrap_err().to_string();
            assert!(refused.contains("X-Trace"), "{bad:?} refused with {refused:?}");
        }
    }

    #[test]
    fn the_table_travels_in_match_order() {
        let routes = vec![
            Route::abort("*/late").unwrap(),
            Route::cont("*", headers(&[("X-Trace", "1")]), Vec::new()).unwrap(),
        ];
        let json: serde_json::Value = serde_json::from_str(&table_json(&routes)).unwrap();
        let rows = json["routes"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["pattern"], "*/late");
        assert_eq!(rows[1]["pattern"], "*");
    }
}
