//! Typed failures from §6.2 of the product spec.

/// Every launcher-visible failure. Missing engine is never an `Option`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No `LURIEN_BIN` and no install path.
    #[error("lurien engine not installed. Run install.sh or set LURIEN_BIN.")]
    EngineMissing,
    /// Binary exists but is not executable.
    #[error("lurien engine is not executable: {path}. Run chmod +x {path}, or reinstall with install.sh.")]
    EngineNotExecutable {
        /// Path that failed the executable check.
        path: String,
    },
    /// Resolved path is not a Firefox-family engine.
    #[error("not a Firefox engine: {path}{hint}. Point LURIEN_BIN at a lurien-browser build.")]
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
    #[error(
        "lurien BiDi port never accepted after {elapsed_ms}ms ({detail}). \
         Read the wrapper log, then retry with a fresh profile_dir."
    )]
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
    #[error("persona incoherent: {reason}. Fix that field, or use the stock FirefoxLinux persona.")]
    PersonaIncoherent {
        /// Gate error text.
        reason: String,
    },
    /// Non-Firefox persona on the lurien engine.
    #[error(
        "lurien only launches Firefox-family personas; {profile} ({family}) is refused. \
         Use a Firefox persona such as FirefoxLinux."
    )]
    NonFirefoxPersona {
        /// Persona name.
        profile: String,
        /// Detected UA family.
        family: String,
    },
    /// Cross-OS persona on this host (v1 default blocks the lie).
    #[error(
        "cross-OS persona {profile} on host {host}: fonts/WebGL/WebGPU would lie. \
         v1 is matched-host Linux Firefox only, so use FirefoxLinux here."
    )]
    CrossOsPersona {
        /// Persona name.
        profile: String,
        /// Host OS (`std::env::consts::OS`).
        host: String,
    },
    /// Proxy URL would not parse, or first connect failed without direct fallback.
    #[error(
        "proxy unreachable or invalid ({url}): {detail}. \
         Fix the URL or start the proxy; there is no direct fallback."
    )]
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
    #[error(
        "corrupt or unreadable cookies.sqlite at {path}: {detail}. \
         Import from a closed Firefox, or start from a fresh profile."
    )]
    CookiesCorrupt {
        /// Path of the cookies file.
        path: String,
        /// Why it was refused.
        detail: String,
    },
    /// Interactive captcha in v1.
    #[error(
        "hard captcha ({kind}): not claimed, so no engine path clears it. \
         Check docs/bench-results/challenge-scorecard.md for the claimed kinds."
    )]
    HardCaptcha {
        /// Catalog kind that is not claimed.
        kind: String,
    },
    /// Managed score-class challenge did not write a token.
    #[error(
        "managed challenge failed: {detail}. \
         Read the evidence rows for this page, then retry with a fresh profile_dir."
    )]
    ScoreFailed {
        /// Classification or token-wait detail.
        detail: String,
    },
    /// Engine process died.
    #[error("lurien engine crashed. Read the wrapper log at {log_path}, then retry with a fresh profile_dir.")]
    EngineCrash {
        /// Path of the wrapper log.
        log_path: String,
    },
    /// The session's download directory cannot be created or written.
    #[error(
        "download directory {path} is unusable: {detail}. \
         Set LURIEN_DOWNLOAD_DIR to a writable directory, or fix its permissions."
    )]
    DownloadDirUnusable {
        /// Directory that was refused.
        path: String,
        /// Why it was refused.
        detail: String,
    },
    /// No download finished in time, the browser canceled one, or the bytes could
    /// not be copied where the caller asked.
    #[error(
        "download {file:?} did not arrive: {detail}. \
         Check `downloads` for what the page started, and raise timeout_ms for a slow file."
    )]
    DownloadFailed {
        /// Filename or pattern the caller asked for.
        file: String,
        /// What happened instead.
        detail: String,
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
    /// A selector never resolved to one element ready to be acted on.
    #[error("{selector}: {detail} after {waited_ms}ms. {action}")]
    Unresolved {
        /// Selector as the caller wrote it.
        selector: String,
        /// Why it did not resolve.
        detail: String,
        /// How long the wait lasted.
        waited_ms: u64,
        /// What to do next, and what was on screen instead.
        action: String,
    },
    /// A step of a `batch` failed. The message says how far the page got, since
    /// the steps before the failure already happened to it.
    #[error("batch step {step} ({verb}) failed: {detail}. ran: {ran}; {skipped} step(s) not run")]
    BatchFailed {
        /// One-based index of the step that failed.
        step: usize,
        /// Verb that failed.
        verb: String,
        /// That verb's own error.
        detail: String,
        /// Steps that completed, in order.
        ran: String,
        /// Steps after the failure, which did not run.
        skipped: usize,
    },
    /// MCP client sent an unknown tool (including `challenge`).
    #[error(
        "unknown tool {name:?}. captcha is automatic; there is no challenge tool. \
         Call tools/list for the registry."
    )]
    UnknownMcpTool {
        /// Tool name the client sent.
        name: String,
    },
    /// Profile import skipped logins because a file was missing.
    #[error("logins skipped: {detail}. Export them from a closed Firefox, or import without them.")]
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

    /// One instance of every variant. The `match` below is exhaustive, so adding
    /// a variant stops this file compiling until it is listed here: a new failure
    /// cannot ship without deciding what a caller should do about it.
    fn every_class() -> Vec<Error> {
        let all = vec![
            Error::EngineMissing,
            Error::EngineNotExecutable { path: "/opt/lurien/lurien".into() },
            Error::NotFirefox { path: "/bin/ls".into(), hint: " (ELF)".into() },
            Error::DisplayUnset,
            Error::BidiTimeout { elapsed_ms: 30_000, detail: "connection refused".into() },
            Error::SessionTimeout { timeout_ms: 60_000 },
            Error::PersonaIncoherent { reason: "UA says Windows, TLS says Linux".into() },
            Error::NonFirefoxPersona { profile: "ChromeLinux".into(), family: "chrome".into() },
            Error::CrossOsPersona { profile: "FirefoxWindows".into(), host: "linux".into() },
            Error::ProxyUnreachable { url: "http://127.0.0.1:9".into(), detail: "refused".into() },
            Error::ProfileLocked { path: "/tmp/profile".into() },
            Error::CookiesCorrupt { path: "/tmp/cookies.sqlite".into(), detail: "not SQLite".into() },
            Error::HardCaptcha { kind: "visual".into() },
            Error::ScoreFailed { detail: "no token after 8000ms".into() },
            Error::EngineCrash { log_path: "/tmp/lurien.log".into() },
            Error::DownloadDirUnusable {
                path: "/mnt/ro/dl".into(),
                detail: "read-only file system".into(),
            },
            Error::DownloadFailed {
                file: "invoice.pdf".into(),
                detail: "nothing finished within 15000ms".into(),
            },
            Error::UnknownVerb { name: "teleport".into() },
            Error::BadArgs {
                verb: "click".into(),
                detail: "unknown argument \"selecter\"; accepts [\"selector\", \"timeout_ms\"]".into(),
            },
            Error::Unresolved {
                selector: "role:button=Ghost".into(),
                detail: "1 element(s) matched but none is visible".into(),
                waited_ms: 4_000,
                action: "on screen now: button \"Log in\"".into(),
            },
            Error::BatchFailed {
                step: 2,
                verb: "click".into(),
                detail: "no element matched".into(),
                ran: "1 goto".into(),
                skipped: 1,
            },
            Error::UnknownMcpTool { name: "challenge".into() },
            Error::LoginsSkipped { detail: "logins.json missing".into() },
            Error::Other("driver closed the connection".into()),
        ];
        for err in &all {
            // Exhaustive on purpose. A new variant must be added above.
            match err {
                Error::EngineMissing
                | Error::EngineNotExecutable { .. }
                | Error::NotFirefox { .. }
                | Error::DisplayUnset
                | Error::BidiTimeout { .. }
                | Error::SessionTimeout { .. }
                | Error::PersonaIncoherent { .. }
                | Error::NonFirefoxPersona { .. }
                | Error::CrossOsPersona { .. }
                | Error::ProxyUnreachable { .. }
                | Error::ProfileLocked { .. }
                | Error::CookiesCorrupt { .. }
                | Error::HardCaptcha { .. }
                | Error::ScoreFailed { .. }
                | Error::EngineCrash { .. }
                | Error::DownloadDirUnusable { .. }
                | Error::DownloadFailed { .. }
                | Error::UnknownVerb { .. }
                | Error::BadArgs { .. }
                | Error::Unresolved { .. }
                | Error::BatchFailed { .. }
                | Error::UnknownMcpTool { .. }
                | Error::LoginsSkipped { .. }
                | Error::Other(_) => {}
            }
        }
        all
    }

    /// An error a caller cannot act on costs a support round trip. Every class
    /// says what to do next, in words a person can follow without reading this
    /// source.
    ///
    /// `Other` carries a driver message verbatim and is exempt: inventing an
    /// action for an unknown driver failure would be a guess.
    #[test]
    fn every_error_class_names_a_corrective_action() {
        const ACTIONS: &[&str] = &[
            "run ", "set ", "use ", "check", "pick", "raise", "retry", "read", "close",
            "fix", "start", "export", "import", "point", "call", "narrow", "take ",
            "accepts", "on screen now", "not run", "chmod",
        ];
        for err in every_class() {
            if matches!(err, Error::Other(_)) {
                continue;
            }
            let text = err.to_string();
            let lower = text.to_lowercase();
            assert!(
                ACTIONS.iter().any(|action| lower.contains(action)),
                "{err:?} tells the caller what broke but not what to do: {text}"
            );
            assert!(
                text.len() > 25,
                "{err:?} is too terse to be actionable: {text}"
            );
        }
    }

    /// Fields exist to be read. A variant that captures a path or a kind and
    /// then hides it makes the caller guess which element, file, or kind failed.
    #[test]
    fn every_error_shows_what_it_captured() {
        for (err, expected) in [
            (Error::EngineNotExecutable { path: "/opt/x".into() }, "/opt/x"),
            (Error::NotFirefox { path: "/bin/ls".into(), hint: String::new() }, "/bin/ls"),
            (Error::HardCaptcha { kind: "visual".into() }, "visual"),
            (Error::ProfileLocked { path: "/tmp/p".into() }, "/tmp/p"),
            (Error::EngineCrash { log_path: "/tmp/l.log".into() }, "/tmp/l.log"),
            (Error::UnknownVerb { name: "teleport".into() }, "teleport"),
            (
                Error::Unresolved {
                    selector: "role:button=Ghost".into(),
                    detail: "hidden".into(),
                    waited_ms: 10,
                    action: "take a snapshot".into(),
                },
                "role:button=Ghost",
            ),
            (
                Error::BatchFailed {
                    step: 3,
                    verb: "fill".into(),
                    detail: "no element".into(),
                    ran: "1 goto, 2 click".into(),
                    skipped: 0,
                },
                "2 click",
            ),
        ] {
            let text = err.to_string();
            assert!(text.contains(expected), "{expected:?} is missing from {text}");
        }
    }
}
