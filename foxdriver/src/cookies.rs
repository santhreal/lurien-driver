//! Captured browser cookies, preserve a solved-captcha session
//! across page loads.
//!
//! When a captcha is solved, the upstream WAF / vendor typically
//! issues one or more cookies (`cf_clearance`, `_pxhd`, `datadome`,
//! etc.) that grant the browser a window of trusted access. Without
//! capturing + replaying these cookies, every navigation re-triggers
//! the captcha challenge.
//!
//! [`capture_from_page`] grabs every cookie from the live page after
//! a successful solve. [`apply_to_page`] re-installs them on a fresh
//! page so the next request rides the trusted session.
//!
//! The capture path uses WebDriver BiDi `storage.getCookies`; the apply
//! path uses `storage.setCookie`. Both are wrapped here so consumers
//! don't need to import rustenium BiDi storage types directly.
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// A single captured browser cookie. Fields mirror the subset of
/// `Network.Cookie` that's relevant for replay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapturedCookie {
    /// Cookie name.
    pub name: String,
    /// Cookie value.
    pub value: String,
    /// Cookie domain (leading-dot form preserved as-is).
    pub domain: String,
    /// Path the cookie applies to.
    pub path: String,
    /// Unix epoch seconds; `None` for session cookies that expire
    /// when the browser closes.
    pub expires: Option<i64>,
    /// The cookie is sent only over TLS.
    pub secure: bool,
    /// The cookie is unreadable from script.
    pub http_only: bool,
    /// Cookie SameSite attribute as a lowercase string ("strict" /
    /// "lax" / "none"); `None` if unset.
    pub same_site: Option<String>,
}

impl CapturedCookie {
    /// True iff the cookie has an explicit expiry that has already
    /// passed. Session cookies (no expiry) are NOT considered
    /// expired by this helper, caller decides whether to replay
    /// them across browser restarts.
    pub fn is_expired_now(&self) -> bool {
        let Some(exp) = self.expires else {
            return false;
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        exp <= now
    }

    /// Drop session cookies (no expiry) AND already-expired cookies
    /// from a slice. The remaining cookies are safe to persist
    /// across browser restarts and apply later.
    pub fn keep_persistent_alive(input: &[CapturedCookie]) -> Vec<CapturedCookie> {
        input
            .iter()
            .filter(|c| c.expires.is_some() && !c.is_expired_now())
            .cloned()
            .collect()
    }
}

/// Capture every cookie on `page` via BiDi `storage.getCookies`.
/// Includes HttpOnly cookies (no JavaScript limitation).
pub async fn capture_from_page(page: &crate::browser::Page) -> anyhow::Result<Vec<CapturedCookie>> {
    page.get_cookies().await
}

/// Apply previously-[`capture_from_page`]-captured cookies to a
/// fresh `page` via BiDi `storage.setCookie`. Skips entries that
/// have already expired; returns the count of cookies actually
/// installed.
pub async fn apply_to_page(
    page: &crate::browser::Page,
    cookies: &[CapturedCookie],
) -> anyhow::Result<usize> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut installed = 0usize;
    for c in cookies {
        if let Some(exp) = c.expires {
            if exp <= now {
                continue; // skip expired
            }
        }
        let same_site = c
            .same_site
            .as_ref()
            .and_then(|s| match s.to_lowercase().as_str() {
                "strict" => Some(rustenium_bidi_definitions::network::types::SameSite::Strict),
                "lax" => Some(rustenium_bidi_definitions::network::types::SameSite::Lax),
                "none" | "no_restriction" | "no-restriction" | "no restriction" => {
                    Some(rustenium_bidi_definitions::network::types::SameSite::None)
                }
                _ => None,
            });
        page.set_cookie(
            &c.name,
            &c.value,
            &c.domain,
            Some(&c.path),
            c.expires.map(|e| e as u64),
            Some(c.secure),
            Some(c.http_only),
            same_site,
        )
        .await?;
        installed += 1;
    }
    Ok(installed)
}

/// Filter `cookies` to only those whose name matches one of `vendor`'s
/// known anti-bot tokens. Useful for trimming a full session capture
/// down to the minimum subset that proves a vendor's challenge passed
///: handy when you want to forward auth state to a non-browser HTTP
/// client (curl/reqwest) without leaking unrelated session data.
///
/// The vendor → cookie-name map is derived from the bundled rule
/// pack's `cookie_names` triggers.
pub fn vendor_cookies(input: &[CapturedCookie], vendor: &str) -> Vec<CapturedCookie> {
    let names: &[&str] = match vendor.to_lowercase().as_str() {
        "cloudflare" | "cf" => &["__cf_bm", "cf_chl_2", "cf_clearance"],
        "akamai" => &["_abck", "bm_sz", "ak_bmsc"],
        "datadome" => &["datadome", "_dd_s"],
        "perimeterx" | "human" => &["_px2", "_pxhd", "_px3", "_pxvid"],
        "incapsula" | "imperva" => &["visid_incap", "incap_ses"],
        "kasada" => &["KP_UIDz", "x-kpsdk-cd", "x-kpsdk-ct"],
        "fastly" => &["_fastly_ngwaf"],
        "sucuri" => &["sucuri_cloudproxy_uuid"],
        "anubis" => &["anubis-auth"],
        _ => return Vec::new(),
    };
    input
        .iter()
        .filter(|c| names.iter().any(|n| c.name.starts_with(*n) || c.name == *n))
        .cloned()
        .collect()
}

#[cfg(test)]
mod vendor_cookie_tests {
    use super::*;

    fn ck(name: &str) -> CapturedCookie {
        CapturedCookie {
            name: name.into(),
            value: "test".into(),
            domain: ".example.com".into(),
            path: "/".into(),
            expires: None,
            secure: false,
            http_only: false,
            same_site: None,
        }
    }

    #[test]
    fn vendor_cookies_filters_cloudflare_set() {
        let all = vec![
            ck("__cf_bm"),
            ck("cf_clearance"),
            ck("session_id"),
            ck("_ga"),
        ];
        let filtered = vendor_cookies(&all, "cloudflare");
        let names: Vec<&str> = filtered.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"__cf_bm"));
        assert!(names.contains(&"cf_clearance"));
        assert!(!names.contains(&"_ga"));
    }

    #[test]
    fn vendor_cookies_matches_prefixed_cookie_names() {
        // Imperva uses dynamic cookie suffixes like `visid_incap_<n>`.
        let all = vec![ck("visid_incap_12345"), ck("incap_ses_99_99")];
        let filtered = vendor_cookies(&all, "imperva");
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn vendor_cookies_unknown_vendor_returns_empty() {
        let all = vec![ck("__cf_bm")];
        let filtered = vendor_cookies(&all, "totally-not-a-vendor");
        assert!(filtered.is_empty());
    }

    #[test]
    fn vendor_cookies_is_case_insensitive_on_vendor() {
        let all = vec![ck("__cf_bm")];
        assert_eq!(vendor_cookies(&all, "Cloudflare").len(), 1);
        assert_eq!(vendor_cookies(&all, "CLOUDFLARE").len(), 1);
        assert_eq!(vendor_cookies(&all, "cf").len(), 1);
    }

    #[test]
    fn vendor_cookies_akamai_set() {
        let all = vec![ck("_abck"), ck("bm_sz"), ck("ak_bmsc"), ck("other")];
        let filtered = vendor_cookies(&all, "akamai");
        assert_eq!(filtered.len(), 3);
    }

    #[test]
    fn vendor_cookies_datadome_set() {
        let all = vec![ck("datadome"), ck("_dd_s"), ck("session")];
        let filtered = vendor_cookies(&all, "datadome");
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn vendor_cookies_perimeterx_aliases() {
        let all = vec![ck("_px2"), ck("_pxhd"), ck("_px3")];
        assert_eq!(vendor_cookies(&all, "perimeterx").len(), 3);
        assert_eq!(vendor_cookies(&all, "human").len(), 3);
    }

    #[test]
    fn vendor_cookies_kasada_set() {
        let all = vec![ck("KP_UIDz"), ck("x-kpsdk-cd")];
        assert_eq!(vendor_cookies(&all, "kasada").len(), 2);
    }

    #[test]
    fn vendor_cookies_fastly_set() {
        let all = vec![ck("_fastly_ngwaf"), ck("other")];
        assert_eq!(vendor_cookies(&all, "fastly").len(), 1);
    }

    #[test]
    fn vendor_cookies_sucuri_set() {
        let all = vec![ck("sucuri_cloudproxy_uuid")];
        assert_eq!(vendor_cookies(&all, "sucuri").len(), 1);
    }

    #[test]
    fn vendor_cookies_anubis_set() {
        let all = vec![ck("anubis-auth")];
        assert_eq!(vendor_cookies(&all, "anubis").len(), 1);
    }

    #[test]
    fn vendor_cookies_empty_input_returns_empty() {
        assert!(vendor_cookies(&[], "cloudflare").is_empty());
    }

    #[test]
    fn vendor_cookies_prefix_match_dynamic_suffix() {
        // Imperva cookies have dynamic suffixes; the vendor rule uses
        // `starts_with` so `visid_incap_` matches `visid_incap_12345`.
        let all = vec![ck("visid_incap_12345"), ck("visid_incap_99999")];
        let filtered = vendor_cookies(&all, "imperva");
        assert_eq!(filtered.len(), 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cookie(name: &str, expires: Option<i64>) -> CapturedCookie {
        CapturedCookie {
            name: name.into(),
            value: "v".into(),
            domain: ".example.com".into(),
            path: "/".into(),
            expires,
            secure: true,
            http_only: false,
            same_site: None,
        }
    }

    #[test]
    fn is_expired_now_true_for_past_expires() {
        let c = cookie("c", Some(1)); // 1970-01-01: definitely past
        assert!(c.is_expired_now());
    }

    #[test]
    fn is_expired_now_false_for_far_future_expires() {
        let c = cookie("c", Some(i64::MAX));
        assert!(!c.is_expired_now());
    }

    #[test]
    fn is_expired_now_false_for_session_cookie() {
        let c = cookie("c", None);
        assert!(!c.is_expired_now());
    }

    #[test]
    fn keep_persistent_alive_drops_session_cookies() {
        let cookies = vec![
            cookie("session", None),
            cookie("persistent", Some(i64::MAX)),
        ];
        let kept = CapturedCookie::keep_persistent_alive(&cookies);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].name, "persistent");
    }

    #[test]
    fn keep_persistent_alive_drops_expired_cookies() {
        let cookies = vec![cookie("expired", Some(1)), cookie("alive", Some(i64::MAX))];
        let kept = CapturedCookie::keep_persistent_alive(&cookies);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].name, "alive");
    }

    #[test]
    fn captured_cookie_serde_roundtrip() {
        let c = cookie("cf_clearance", Some(1234567890));
        let json = serde_json::to_string(&c).unwrap();
        let back: CapturedCookie = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn is_expired_now_false_at_exact_boundary() {
        // Since we compare `exp <= now`, a cookie that expires exactly at
        // the current second may or may not be expired depending on timing.
        // We test the structural property: `now` is >= 0 and `exp` = 0
        // should be expired because `0 <= now` is always true for now >= 0.
        let c = cookie("c", Some(0));
        assert!(c.is_expired_now());
    }

    #[test]
    fn is_expired_now_negative_expiry_treated_as_expired() {
        // Negative epoch seconds are in the past
        let c = cookie("c", Some(-1));
        assert!(c.is_expired_now());
    }

    #[test]
    fn keep_persistent_alive_empty_input() {
        let kept = CapturedCookie::keep_persistent_alive(&[]);
        assert!(kept.is_empty());
    }

    #[test]
    fn keep_persistent_alive_all_expired_returns_empty() {
        let cookies = vec![cookie("a", Some(1)), cookie("b", Some(2))];
        let kept = CapturedCookie::keep_persistent_alive(&cookies);
        assert!(kept.is_empty());
    }

    #[test]
    fn keep_persistent_alive_all_session_returns_empty() {
        let cookies = vec![cookie("a", None), cookie("b", None)];
        let kept = CapturedCookie::keep_persistent_alive(&cookies);
        assert!(kept.is_empty());
    }

    #[test]
    fn keep_persistent_alive_preserves_order() {
        let cookies = vec![
            cookie("first", Some(i64::MAX)),
            cookie("second", Some(i64::MAX - 1)),
        ];
        let kept = CapturedCookie::keep_persistent_alive(&cookies);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].name, "first");
        assert_eq!(kept[1].name, "second");
    }

    #[test]
    fn captured_cookie_equality() {
        let a = cookie("a", Some(100));
        let b = cookie("a", Some(100));
        let c = cookie("a", Some(200));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn captured_cookie_serde_with_all_fields() {
        let c = CapturedCookie {
            name: "session".into(),
            value: "abc123".into(),
            domain: ".example.com".into(),
            path: "/api".into(),
            expires: Some(1893456000),
            secure: true,
            http_only: true,
            same_site: Some("strict".into()),
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: CapturedCookie = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
        assert!(json.contains("same_site"));
        assert!(json.contains("http_only"));
    }

    #[test]
    fn same_site_case_insensitive_mapping() {
        let inputs = ["Strict", "STRICT", "Lax", "LAX", "None", "NONE"];
        for s in inputs {
            let same_site = Some(s.to_string()).as_ref().and_then(|str_val| {
                match str_val.to_lowercase().as_str() {
                    "strict" => Some(rustenium_bidi_definitions::network::types::SameSite::Strict),
                    "lax" => Some(rustenium_bidi_definitions::network::types::SameSite::Lax),
                    "none" => Some(rustenium_bidi_definitions::network::types::SameSite::None),
                    _ => None,
                }
            });
            assert!(same_site.is_some(), "failed to map same_site string '{s}'");
        }
    }
    #[test]
    fn same_site_no_restriction_variants_map_to_none() {
        let inputs = [
            "no_restriction",
            "NO_RESTRICTION",
            "no-restriction",
            "no restriction",
        ];
        for s in inputs {
            let same_site = Some(s.to_string()).as_ref().and_then(|str_val| {
                match str_val.to_lowercase().as_str() {
                    "strict" => Some(rustenium_bidi_definitions::network::types::SameSite::Strict),
                    "lax" => Some(rustenium_bidi_definitions::network::types::SameSite::Lax),
                    "none" | "no_restriction" | "no-restriction" | "no restriction" => {
                        Some(rustenium_bidi_definitions::network::types::SameSite::None)
                    }
                    _ => None,
                }
            });
            assert_eq!(
                same_site,
                Some(rustenium_bidi_definitions::network::types::SameSite::None),
                "failed to map variant '{s}'"
            );
        }
    }
}
