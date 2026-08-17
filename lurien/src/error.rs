//! Typed failures from §6.2 of the product spec.

/// Every launcher-visible failure. Missing engine is never an `Option`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No `LURIEN_BIN` and no install path.
    #[error("lurien engine not installed. Run install.sh or set LURIEN_BIN.")]
    EngineMissing,
    /// Binary exists but is not executable.
    #[error("lurien engine is not executable: {path}. Check with: file {path}")]
    EngineNotExecutable {
        /// Path that failed the executable check.
        path: String,
    },
    /// Resolved path is not a Firefox-family engine.
    #[error("not a Firefox engine: {path}. Check with: file {path}{hint}")]
    NotFirefox {
        /// Path that was refused.
        path: String,
        /// Extra `file(1)` hint.
        hint: String,
    },
    /// Headful launch with no `DISPLAY`.
    #[error(
        "headful lurien needs DISPLAY. Start Xvfb or export DISPLAY. \
         Headless is weaker; pass headless=true only if you accept that."
    )]
    DisplayUnset,
    /// BiDi port never accepted.
    #[error("lurien BiDi port never accepted after {elapsed_ms}ms ({detail})")]
    BidiTimeout {
        /// Elapsed poll time.
        elapsed_ms: u64,
        /// Last errno / driver message.
        detail: String,
    },
    /// rustenium `session.new` timed out.
    #[error(
        "lurien session.new timed out ({timeout_ms}ms). Raise RUSTENIUM_COMMAND_TIMEOUT_SECS."
    )]
    SessionTimeout {
        /// Timeout that fired.
        timeout_ms: u64,
    },
    /// Persona failed the coherence gate.
    #[error("persona incoherent: {reason}")]
    PersonaIncoherent {
        /// Gate error text.
        reason: String,
    },
    /// Non-Firefox persona on the lurien engine.
    #[error("lurien only launches Firefox-family personas; {profile} ({family}) is refused")]
    NonFirefoxPersona {
        /// Persona name.
        profile: String,
        /// Detected UA family.
        family: String,
    },
    /// Cross-OS persona on this host (v1 default blocks the lie).
    #[error(
        "cross-OS persona {profile} on host {host}: fonts/WebGL/WebGPU would lie. \
         v1 is matched-host Linux Firefox only."
    )]
    CrossOsPersona {
        /// Persona name.
        profile: String,
        /// Host OS (`std::env::consts::OS`).
        host: String,
    },
    /// Proxy URL would not parse, or first connect failed without direct fallback.
    #[error("proxy unreachable or invalid ({url}): {detail}")]
    ProxyUnreachable {
        /// Proxy URL the caller supplied.
        url: String,
        /// Parse or connect error.
        detail: String,
    },
    /// Profile directory is locked by another Firefox.
    #[error("{path}: close the other Firefox or pick a new profile_dir.")]
    ProfileLocked {
        /// Locked profile directory.
        path: String,
    },
    /// `cookies.sqlite` is missing or not a SQLite database.
    #[error("corrupt or unreadable cookies.sqlite at {path}: {detail}")]
    CookiesCorrupt {
        /// Path of the cookies file.
        path: String,
        /// Why it was refused.
        detail: String,
    },
    /// Interactive captcha in v1.
    #[error("hard captcha; not claimed in v1.")]
    HardCaptcha {
        /// Catalog kind that is not claimed.
        kind: String,
    },
    /// Managed score-class challenge did not write a token.
    #[error("managed challenge failed: {detail}")]
    ScoreFailed {
        /// Classification or token-wait detail.
        detail: String,
    },
    /// Engine process died.
    #[error("lurien engine crashed. wrapper log: {log_path}")]
    EngineCrash {
        /// Path of the wrapper log.
        log_path: String,
    },
    /// A face sent a verb that is not in the registry.
    #[error("unknown verb {name:?}. Run `lurien verbs` for the registry.")]
    UnknownVerb {
        /// Verb name the face sent.
        name: String,
    },
    /// Arguments failed the verb's own spec: unknown key, missing required, or
    /// wrong type. Validated before anything launches.
    #[error("{verb}: {detail}")]
    BadArgs {
        /// Verb that refused the arguments.
        verb: String,
        /// What was wrong.
        detail: String,
    },
    /// MCP client sent an unknown tool (including `challenge`).
    #[error("unknown tool {name:?}. captcha is automatic; there is no challenge tool.")]
    UnknownMcpTool {
        /// Tool name the client sent.
        name: String,
    },
    /// Profile import skipped logins because a file was missing.
    #[error("logins skipped: {detail}")]
    LoginsSkipped {
        /// Which login file was missing.
        detail: String,
    },
    /// Import or launch IO / driver failure.
    #[error("{0}")]
    Other(
        /// Driver or IO message.
        String,
    ),
}

impl Error {
    /// Map a resolver `anyhow` into [`Error::EngineMissing`] when the sentence matches.
    #[must_use]
    pub fn from_resolve(err: anyhow::Error) -> Self {
        let msg = err.to_string();
        if msg.contains("lurien engine not installed") {
            Self::EngineMissing
        } else {
            Self::Other(msg)
        }
    }
}

impl From<anyhow::Error> for Error {
    fn from(err: anyhow::Error) -> Self {
        Self::from_resolve(err)
    }
}

/// True when `path` looks like a Firefox lock left by a live process.
#[must_use]
pub fn profile_looks_locked(dir: &std::path::Path) -> bool {
    dir.join("lock").exists() || dir.join(".parent.lock").exists()
}

/// SQLite magic. A truncated or HTML file is corrupt, not a profile.
#[must_use]
pub fn is_sqlite_file(path: &std::path::Path) -> bool {
    match std::fs::read(path) {
        Ok(bytes) if bytes.len() >= 16 => bytes.starts_with(b"SQLite format 3\0"),
        _ => false,
    }
}

/// Dest path as a display string.
#[must_use]
pub fn path_string(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_resolve_names_install() {
        let err = Error::from_resolve(anyhow::anyhow!(
            "lurien engine not installed. Run install.sh or set LURIEN_BIN."
        ));
        assert!(matches!(err, Error::EngineMissing));
        assert!(err.to_string().contains("install.sh"));
        assert!(err.to_string().contains("LURIEN_BIN"));
    }
}
