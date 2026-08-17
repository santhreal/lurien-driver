//! Launch contract: resolve → gates → spawn → Page.

use crate::error::Error;
use crate::geo::{Geolocation, Position};
use crate::permission::PermissionPolicy;
use crate::resolve::resolve_engine_checked;
use guise::StealthProfile;
use runtime_foxdriver::{FoxBrowserConfig, Page, ProxyConfig};
use std::sync::Arc;

/// Launch options. Default is headful, FirefoxLinux, no proxy.
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    /// Persona. Default [`StealthProfile::FirefoxLinux`].
    pub profile: StealthProfile,
    /// Headless is weaker. Default `false`.
    pub headless: bool,
    /// Persistent Firefox profile directory.
    pub profile_dir: Option<String>,
    /// Optional proxy. Unreachable proxy does not fall back to direct.
    pub proxy: Option<ProxyConfig>,
    /// Directory downloads land in. Resolved per session, so two sessions never
    /// overwrite each other's file of the same name.
    pub download_dir: Option<String>,
    /// What a page gets when it asks for a capability. Set here because Gecko
    /// reads it at startup; nothing changes it in a live session.
    pub permissions: PermissionPolicy,
    /// Where the browser thinks it is. `None` means the persona's own region.
    pub geolocation: Option<Position>,
    /// The position state and control channel of the session. Created before
    /// launch by whoever owns the session, because the engine is told about the
    /// channel in its environment and applies the starting position to the first
    /// window it opens; [`launch_with_options`] creates one when the caller did
    /// not.
    pub geo: Option<Arc<Geolocation>>,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            profile: StealthProfile::FirefoxLinux,
            headless: false,
            profile_dir: None,
            proxy: None,
            download_dir: None,
            permissions: PermissionPolicy::default(),
            geolocation: None,
            geo: None,
        }
    }
}

/// A launched browser, and the session-scoped services that must outlive the
/// call that started them.
pub struct Launched {
    /// The BiDi page.
    pub page: Page,
    /// The position this session serves and the channel that moves it.
    pub geo: Arc<Geolocation>,
}

/// Resolve the engine, enforce gates, spawn lurien, return a BiDi [`Page`].
pub async fn launch_with_options(opts: LaunchOptions) -> Result<Launched, Error> {
    let bin = resolve_engine_checked()?;
    if !opts.headless && display_unset() {
        return Err(Error::DisplayUnset);
    }
    firefox_family_gate(opts.profile)?;
    cross_os_gate(opts.profile)?;
    if let Some(dir) = opts.profile_dir.as_deref() {
        let p = std::path::Path::new(dir);
        if crate::error::profile_looks_locked(p) {
            return Err(Error::ProfileLocked {
                path: dir.to_string(),
            });
        }
    }
    if let Some(proxy) = opts.proxy.as_ref() {
        probe_proxy(proxy).await?;
    }
    guise::enforce_persona_launch_coherence(&opts.profile, opts.proxy.is_none()).map_err(|e| {
        Error::PersonaIncoherent {
            reason: e.to_string(),
        }
    })?;

    // Downloads must land somewhere this session can read, with no prompt: a pref
    // set after startup would miss a file the first page starts.
    let downloads = std::path::PathBuf::from(
        opts.download_dir
            .clone()
            .unwrap_or_else(crate::download::session_dir),
    );
    crate::download::ensure_dir(&downloads)?;

    // Where the browser thinks it is. The engine applies the position itself, in
    // the process that owns the tab, so what a launch carries is the channel and
    // the starting fix. A caller that already made one (a session, so a verb can
    // move the position before the first page) keeps it.
    let geo = match opts.geo.clone() {
        Some(state) => state,
        None => Arc::new(Geolocation::new(
            crate::geo::persona_position(opts.profile),
            opts.geolocation,
        )?),
    };
    let mut prefs = crate::download::prefs(&downloads);
    prefs.push_str(&opts.permissions.prefs());
    prefs.push_str(&crate::geo::prefs());

    // The engine solves at the level that can see the widget. It is inert unless
    // this variable is present, so a session that never asked is never observed.
    let challenge = crate::challenge::ChallengeConfig::for_process();
    let config = FoxBrowserConfig {
        headless: opts.headless,
        profile_dir: opts.profile_dir,
        proxy: opts.proxy,
        viewport_width: 1280,
        viewport_height: 720,
        env: vec![challenge.env_entry(), geo.env_entry()],
        user_js_content: Some(prefs),
        ..Default::default()
    };
    let page = guise::browser::launch_with_config(&bin, &opts.profile, config)
        .await
        .map_err(map_launch_err)?;
    Ok(Launched { page, geo })
}

async fn probe_proxy(proxy: &ProxyConfig) -> Result<(), Error> {
    let addr = format!("{}:{}", proxy.host, proxy.port);
    match tokio::time::timeout(
        std::time::Duration::from_secs(3),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(Error::ProxyUnreachable {
            url: addr,
            detail: e.to_string(),
        }),
        Err(_) => Err(Error::ProxyUnreachable {
            url: addr,
            detail: "connect timed out".to_string(),
        }),
    }
}

/// Default launch: FirefoxLinux, headful.
pub async fn launch(profile: StealthProfile) -> Result<Launched, Error> {
    launch_with_options(LaunchOptions {
        profile,
        ..LaunchOptions::default()
    })
    .await
}

fn firefox_family_gate(profile: StealthProfile) -> Result<(), Error> {
    let ua = guise::fingerprint::profile_user_agent(profile);
    let family = guise::fingerprint::user_agent_facts(ua).browser;
    if !matches!(family, guise::fingerprint::UserAgentBrowser::Firefox) {
        return Err(Error::NonFirefoxPersona {
            profile: format!("{profile:?}"),
            family: format!("{family:?}"),
        });
    }
    Ok(())
}

fn cross_os_gate(profile: StealthProfile) -> Result<(), Error> {
    let host = std::env::consts::OS;
    let platform = guise::fingerprint::profile_platform(profile);
    let matched = matches!(
        (platform, host),
        (guise::fingerprint::UserAgentPlatform::Linux, "linux")
            | (guise::fingerprint::UserAgentPlatform::Windows, "windows")
            | (guise::fingerprint::UserAgentPlatform::MacOs, "macos")
    );
    if !matched {
        return Err(Error::CrossOsPersona {
            profile: format!("{profile:?}"),
            host: host.to_string(),
        });
    }
    Ok(())
}

/// True when `DISPLAY` is missing or only whitespace. Empty `DISPLAY=` is
/// not a display; Gecko hangs 30s waiting for one.
fn display_unset() -> bool {
    display_value_unset(std::env::var_os("DISPLAY").as_deref())
}

fn display_value_unset(v: Option<&std::ffi::OsStr>) -> bool {
    match v {
        None => true,
        Some(v) => v.to_string_lossy().trim().is_empty(),
    }
}

fn map_launch_err(err: anyhow::Error) -> Error {
    let msg = err.to_string();
    if msg.contains("lurien engine not installed") {
        return Error::EngineMissing;
    }
    if msg.to_ascii_lowercase().contains("timeout") && msg.contains("session") {
        return Error::SessionTimeout { timeout_ms: 60_000 };
    }
    if looks_like_proxy_failure(&msg) {
        return Error::ProxyUnreachable {
            url: "(configured proxy)".to_string(),
            detail: msg,
        };
    }
    if msg.contains("Connection refused") || msg.contains("never accepted") {
        return Error::BidiTimeout {
            elapsed_ms: 0,
            detail: msg,
        };
    }
    Error::Other(msg)
}

fn looks_like_proxy_failure(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    let names_proxy = lower.contains("proxy");
    let failed = lower.contains("unreachable")
        || lower.contains("refused")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("failed to connect")
        || lower.contains("ns_error_proxy")
        || lower.contains("proxy_connection_refused");
    names_proxy && failed
}

/// Source-walk registry: every launcher must call [`resolve_engine`].
#[must_use]
pub fn launch_call_sites() -> &'static [&'static str] {
    &[
        "lurien::Browser::launch",
        "lurien::Browser::launch_with_options",
        "lurien CLI",
        "lurien-mcp",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firefox_linux_passes_family_gate() {
        firefox_family_gate(StealthProfile::FirefoxLinux).expect("FirefoxLinux");
    }

    #[test]
    fn chrome_persona_is_refused() {
        let err = firefox_family_gate(StealthProfile::ChromeLinux).expect_err("chrome");
        assert!(matches!(err, Error::NonFirefoxPersona { .. }));
    }

    #[test]
    fn windows_persona_is_cross_os_on_linux() {
        if std::env::consts::OS != "linux" {
            return;
        }
        let err = cross_os_gate(StealthProfile::FirefoxWindows).expect_err("win on linux");
        assert!(matches!(err, Error::CrossOsPersona { .. }));
    }

    #[test]
    fn firefox_linux_is_matched_host() {
        if std::env::consts::OS != "linux" {
            return;
        }
        cross_os_gate(StealthProfile::FirefoxLinux).expect("matched host");
    }

    #[test]
    fn proxy_connect_failure_is_not_other() {
        let err = map_launch_err(anyhow::anyhow!(
            "NS_ERROR_PROXY_CONNECTION_REFUSED connecting via http://127.0.0.1:9"
        ));
        match err {
            Error::ProxyUnreachable { detail, .. } => {
                assert!(detail.contains("NS_ERROR_PROXY_CONNECTION_REFUSED"));
            }
            other => panic!("expected ProxyUnreachable, got {other:?}"),
        }
    }

    #[test]
    fn bidi_refused_without_proxy_stays_bidi() {
        let err = map_launch_err(anyhow::anyhow!("Connection refused"));
        assert!(matches!(err, Error::BidiTimeout { .. }));
    }

    #[tokio::test]
    async fn unreachable_proxy_is_named_before_spawn() {
        let proxy = ProxyConfig {
            host: "127.0.0.1".into(),
            port: 9,
            ..Default::default()
        };
        let err = probe_proxy(&proxy).await.expect_err("port 9 refused");
        match err {
            Error::ProxyUnreachable { url, detail } => {
                assert_eq!(url, "127.0.0.1:9");
                assert!(!detail.is_empty());
            }
            other => panic!("expected ProxyUnreachable, got {other:?}"),
        }
    }

    #[test]
    fn blank_display_is_unset() {
        use std::ffi::OsStr;
        assert!(display_value_unset(None));
        assert!(display_value_unset(Some(OsStr::new(""))));
        assert!(display_value_unset(Some(OsStr::new("   "))));
        assert!(!display_value_unset(Some(OsStr::new(":10"))));
        assert!(!display_value_unset(Some(OsStr::new(":0"))));
    }
}
