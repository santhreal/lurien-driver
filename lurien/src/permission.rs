//! What the browser answers when a page asks for a capability.
//!
//! A permission is a property of the profile, not of a live page: Gecko reads
//! `permissions.default.*` when it starts, and nothing a driver can send changes
//! it afterwards. So the policy is set at launch and reported honestly, and a
//! caller who wants a different answer relaunches. Every known permission is
//! written explicitly, so what the [`crate::verb`] `permissions` verb reports is
//! what the profile holds, never a guess at a browser default.
//!
//! The default is deny, not prompt: a prompt nobody can answer leaves the page
//! waiting forever, and a grant nobody was asked for is a session that has
//! already clicked a dialog it never saw.

use crate::error::Error;
use std::collections::BTreeMap;

/// The answer a permission request gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grant {
    /// Ask, and let the request hang until something answers the doorhanger.
    Prompt,
    /// Granted without asking.
    Allow,
    /// Refused without asking.
    Deny,
}

impl Grant {
    /// The `permissions.default.*` value Gecko reads.
    #[must_use]
    pub const fn pref_value(self) -> u8 {
        match self {
            Self::Prompt => 0,
            Self::Allow => 1,
            Self::Deny => 2,
        }
    }

    /// The name a face prints and accepts.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
}

/// Permission name a caller uses, and the Gecko pref suffix behind it. Data, so
/// the CLI flag, the JSON report and the prefs cannot drift apart.
const PERMISSIONS: &[(&str, &str)] = &[
    ("geolocation", "geo"),
    ("notifications", "desktop-notification"),
    ("camera", "camera"),
    ("microphone", "microphone"),
    ("midi", "midi"),
    ("persistent-storage", "persistent-storage"),
];

/// Every permission a session can decide, in report order.
#[must_use]
pub fn names() -> Vec<&'static str> {
    PERMISSIONS.iter().map(|(name, _)| *name).collect()
}

/// What this session answers, per permission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionPolicy {
    /// Only the permissions that differ from [`Grant::Deny`].
    granted: BTreeMap<&'static str, Grant>,
}

impl Default for PermissionPolicy {
    fn default() -> Self {
        Self::deny_all()
    }
}

impl PermissionPolicy {
    /// Refuse every permission without asking.
    #[must_use]
    pub fn deny_all() -> Self {
        Self {
            granted: BTreeMap::new(),
        }
    }

    /// Set one permission by name.
    ///
    /// # Errors
    ///
    /// [`Error::BadArgs`] when the name is not a permission this browser decides.
    pub fn set(&mut self, name: &str, grant: Grant) -> Result<(), Error> {
        let known = PERMISSIONS
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .ok_or_else(|| Error::BadArgs {
                verb: "permissions".to_string(),
                detail: format!("{name:?} is not a permission; pass one of {:?}", names()),
            })?;
        if grant == Grant::Deny {
            self.granted.remove(known.0);
        } else {
            self.granted.insert(known.0, grant);
        }
        Ok(())
    }

    /// The answer `name` gets. Unknown names are denied, like anything else this
    /// session was not asked to allow.
    #[must_use]
    pub fn grant_of(&self, name: &str) -> Grant {
        self.granted.get(name).copied().unwrap_or(Grant::Deny)
    }

    /// Build a policy from the names a face was given.
    ///
    /// # Errors
    ///
    /// [`Error::BadArgs`] when a name is not a permission, or is listed twice
    /// with two different answers.
    pub fn from_lists(allow: &[String], prompt: &[String]) -> Result<Self, Error> {
        let mut policy = Self::deny_all();
        for name in allow {
            policy.set(name.trim(), Grant::Allow)?;
        }
        for name in prompt {
            let name = name.trim();
            if policy.grant_of(name) == Grant::Allow {
                return Err(Error::BadArgs {
                    verb: "permissions".to_string(),
                    detail: format!("{name:?} is both allowed and prompted; pass it once"),
                });
            }
            policy.set(name, Grant::Prompt)?;
        }
        Ok(policy)
    }

    /// Split a comma-separated list, as a CLI flag or a wire argument carries it.
    #[must_use]
    pub fn split_list(value: &str) -> Vec<String> {
        value
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// Prefs for the profile. Every permission is written, including the denied
    /// ones, so the profile never falls back to a browser default.
    #[must_use]
    pub fn prefs(&self) -> String {
        let mut out = String::new();
        for (name, suffix) in PERMISSIONS {
            out.push_str(&format!(
                "user_pref(\"permissions.default.{suffix}\", {});\n",
                self.grant_of(name).pref_value()
            ));
        }
        out
    }

    /// What the `permissions` verb reports.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for name in names() {
            map.insert(
                name.to_string(),
                serde_json::Value::String(self.grant_of(name).as_str().to_string()),
            );
        }
        serde_json::Value::Object(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_granted_unless_it_was_asked_for() {
        let policy = PermissionPolicy::default();
        for name in names() {
            assert_eq!(policy.grant_of(name), Grant::Deny, "{name}");
        }
    }

    #[test]
    fn every_permission_reaches_the_profile() {
        // A permission the report names but the prefs omit would be a session
        // whose answer is whatever the browser felt like.
        let policy = PermissionPolicy::from_lists(&["geolocation".to_string()], &[]).expect("built");
        let prefs = policy.prefs();
        for (name, suffix) in PERMISSIONS {
            let want = policy.grant_of(name).pref_value();
            assert!(
                prefs.contains(&format!("user_pref(\"permissions.default.{suffix}\", {want});")),
                "{name} is missing from {prefs}"
            );
        }
        assert!(prefs.contains("user_pref(\"permissions.default.geo\", 1);"));
        assert!(prefs.contains("user_pref(\"permissions.default.camera\", 2);"));
    }

    #[test]
    fn the_report_covers_the_same_names_the_flags_accept() {
        let policy = PermissionPolicy::from_lists(&[], &["camera".to_string()]).expect("built");
        let report = policy.to_json();
        let object = report.as_object().expect("object");
        assert_eq!(object.len(), names().len());
        assert_eq!(object["camera"], "prompt");
        assert_eq!(object["geolocation"], "deny");
    }

    #[test]
    fn an_unknown_permission_names_the_ones_that_exist() {
        let err = PermissionPolicy::from_lists(&["telepathy".to_string()], &[])
            .expect_err("not a permission");
        let text = err.to_string();
        assert!(text.contains("telepathy"), "{text}");
        assert!(text.contains("geolocation"), "{text}");
    }

    #[test]
    fn one_permission_cannot_have_two_answers() {
        let err = PermissionPolicy::from_lists(&["camera".to_string()], &["camera".to_string()])
            .expect_err("contradiction");
        assert!(err.to_string().contains("once"), "{err}");
    }

    #[test]
    fn a_list_is_split_the_same_way_on_every_face() {
        assert_eq!(
            PermissionPolicy::split_list(" geolocation , camera ,"),
            vec!["geolocation".to_string(), "camera".to_string()]
        );
        assert!(PermissionPolicy::split_list("").is_empty());
    }
}
