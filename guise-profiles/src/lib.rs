//! Pure browser fingerprint data (the canonical source for browser identity).
//!
//! Holds the `StealthProfile` selector, per-profile `ProfileFacts` (User-Agent,
//! navigation headers, hardware, client hints), and the canonical `HeaderProfile`
//! header catalog. This crate intentionally has no runtime, HTTP, TLS, browser, or
//! async dependencies. It sits below `scanclient` and `stealth` so both crates can
//! derive browser identity from one source without creating a dependency cycle.
//!
//! Naming: `StealthProfile` is the canonical selector; `ProfileFacts` is its data;
//! a `<Domain>Profile` (e.g. `HeaderProfile`) is a pure derived projection for one
//! domain. One type per projection. `HeaderProfile` is the single browser-header
//! catalog type, re-exported unchanged by `scanclient` and `stealth`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::todo,
        clippy::unimplemented,
        clippy::panic
    )
)]
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::too_many_lines,
    clippy::match_same_arms,
    clippy::doc_markdown
)]

mod os_network;
pub use os_network::{
    infer_initial_ttl, os_network_coherence, os_network_options_match, os_network_stack,
    profile_os_network_stack, Ja4tError, NetworkOsCoherence, OsNetworkStack, TcpWindow,
    KNOWN_INITIAL_TTLS,
};

/// Named browser fingerprint variants. Each one identifies a coherent
/// (browser, OS, GPU class) tuple used by higher-level stealth crates.
///
/// `#[non_exhaustive]` - adding a new variant is a minor-version change,
/// removing one is major. UA strings inside each variant are also additive
/// maintenance data, but the browser/OS family shape stays stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StealthProfile {
    /// Chrome stable on Windows 10/11, Intel iGPU.
    ChromeWindowsStable,
    /// Chrome 96 on Windows 10 for legacy differential and compatibility probes.
    ChromeWindowsLegacy96,
    /// Chrome stable on macOS, Apple Silicon GPU.
    ChromeMacStable,
    /// Microsoft Edge on Windows, same Chromium core but distinct brand.
    EdgeWindowsStable,
    /// Internet Explorer 11 on Windows 8.1 for legacy resolver fallback paths.
    Ie11Windows,
    /// Firefox on Linux desktop.
    FirefoxLinux,
    /// Firefox on Windows desktop.
    FirefoxWindows,
    /// Firefox on macOS desktop.
    FirefoxMacStable,
    /// Chrome on Android.
    ChromeAndroid,
    /// Safari on iPhone.
    SafariIphone,
    /// Safari on iPad.
    SafariIpad,
    /// Safari on macOS desktop.
    SafariMacStable,
    /// Chrome on Linux desktop.
    ChromeLinux,
    /// Brave browser on Windows.
    BraveWindows,
    /// Opera on Windows.
    OperaWindows,
    /// Samsung Internet on Galaxy phone.
    SamsungInternetAndroid,
}

/// Canonical default stealth identity for fleet-owned browser and HTTP launches.
pub const DEFAULT_STEALTH_PROFILE: StealthProfile = StealthProfile::ChromeWindowsStable;

/// Every canonical browser identity profile in the catalog.
pub const ALL_PROFILES: &[StealthProfile] = &[
    DEFAULT_STEALTH_PROFILE,
    StealthProfile::ChromeWindowsLegacy96,
    StealthProfile::ChromeMacStable,
    StealthProfile::EdgeWindowsStable,
    StealthProfile::Ie11Windows,
    StealthProfile::FirefoxLinux,
    StealthProfile::FirefoxWindows,
    StealthProfile::FirefoxMacStable,
    StealthProfile::ChromeAndroid,
    StealthProfile::SafariIphone,
    StealthProfile::SafariIpad,
    StealthProfile::SafariMacStable,
    StealthProfile::ChromeLinux,
    StealthProfile::BraveWindows,
    StealthProfile::OperaWindows,
    StealthProfile::SamsungInternetAndroid,
];

/// Profiles intended for deterministic fleet rotation.
///
/// Legacy compatibility personas such as IE11 and Chrome 96 are intentionally
/// excluded from normal rotation; callers can still request them explicitly by
/// variant or by [`named_profile`].
pub const ROTATION_PROFILES: &[StealthProfile] = &[
    DEFAULT_STEALTH_PROFILE,
    StealthProfile::ChromeMacStable,
    StealthProfile::EdgeWindowsStable,
    StealthProfile::FirefoxLinux,
    StealthProfile::FirefoxWindows,
    StealthProfile::FirefoxMacStable,
    StealthProfile::ChromeAndroid,
    StealthProfile::SafariIphone,
    StealthProfile::SafariIpad,
    StealthProfile::SafariMacStable,
    StealthProfile::ChromeLinux,
    StealthProfile::BraveWindows,
    StealthProfile::OperaWindows,
    StealthProfile::SamsungInternetAndroid,
];

/// Stable lowercase profile name for caller-facing config.
#[must_use]
pub const fn profile_name(profile: StealthProfile) -> &'static str {
    match profile {
        StealthProfile::ChromeWindowsStable => "chrome",
        StealthProfile::ChromeWindowsLegacy96 => "chrome-windows-legacy-96",
        StealthProfile::ChromeMacStable => "chrome-macos",
        StealthProfile::EdgeWindowsStable => "edge",
        StealthProfile::Ie11Windows => "ie11-windows",
        StealthProfile::FirefoxLinux => "firefox",
        StealthProfile::FirefoxWindows => "firefox-windows",
        StealthProfile::FirefoxMacStable => "firefox-macos",
        StealthProfile::ChromeAndroid => "chrome-android",
        StealthProfile::SafariIphone => "safari-iphone",
        StealthProfile::SafariIpad => "safari-ipad",
        StealthProfile::SafariMacStable => "safari",
        StealthProfile::ChromeLinux => "chrome-linux",
        StealthProfile::BraveWindows => "brave",
        StealthProfile::OperaWindows => "opera",
        StealthProfile::SamsungInternetAndroid => "samsung-internet",
    }
}

/// Stable enum-style profile name for human-readable listings.
#[must_use]
pub const fn profile_display_name(profile: StealthProfile) -> &'static str {
    match profile {
        StealthProfile::ChromeWindowsStable => "ChromeWindowsStable",
        StealthProfile::ChromeWindowsLegacy96 => "ChromeWindowsLegacy96",
        StealthProfile::ChromeMacStable => "ChromeMacStable",
        StealthProfile::EdgeWindowsStable => "EdgeWindowsStable",
        StealthProfile::Ie11Windows => "Ie11Windows",
        StealthProfile::FirefoxLinux => "FirefoxLinux",
        StealthProfile::FirefoxWindows => "FirefoxWindows",
        StealthProfile::FirefoxMacStable => "FirefoxMacStable",
        StealthProfile::ChromeAndroid => "ChromeAndroid",
        StealthProfile::SafariIphone => "SafariIphone",
        StealthProfile::SafariIpad => "SafariIpad",
        StealthProfile::SafariMacStable => "SafariMacStable",
        StealthProfile::ChromeLinux => "ChromeLinux",
        StealthProfile::BraveWindows => "BraveWindows",
        StealthProfile::OperaWindows => "OperaWindows",
        StealthProfile::SamsungInternetAndroid => "SamsungInternetAndroid",
    }
}

/// Resolve a config profile name or common alias to a canonical profile.
#[must_use]
pub fn named_profile(name: &str) -> Option<StealthProfile> {
    let normalized = name.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "chrome"
        | "chrome-windows"
        | "chrome-win"
        | "chrome_131_windows"
        | "chrome_windows"
        | "chromewindowsstable" => Some(StealthProfile::ChromeWindowsStable),
        "chrome-windows-legacy-96"
        | "chrome_96_windows"
        | "chrome_windows_legacy_96"
        | "chromewindowslegacy96" => Some(StealthProfile::ChromeWindowsLegacy96),
        "chrome-macos" | "chrome-mac" | "chrome-osx" | "chrome_131_macos" | "chrome_mac"
        | "chromemacstable" => Some(StealthProfile::ChromeMacStable),
        "edge" | "edge-windows" | "edge_131" | "edge_windows" | "edgewindowsstable" => {
            Some(StealthProfile::EdgeWindowsStable)
        }
        "ie11" | "ie" | "internet-explorer" | "ie11-windows" | "ie11_windows" | "ie11windows" => {
            Some(StealthProfile::Ie11Windows)
        }
        "firefox" | "firefox-linux" | "firefox_133" | "firefox_linux" | "firefoxlinux" => {
            Some(StealthProfile::FirefoxLinux)
        }
        "firefox-windows" | "firefox_windows" | "firefox_133_windows" | "firefoxwindows" => {
            Some(StealthProfile::FirefoxWindows)
        }
        "firefox-macos" | "firefox-mac" | "firefox_osx" | "firefox_mac" | "firefoxmacstable" => {
            Some(StealthProfile::FirefoxMacStable)
        }
        "chrome-android" | "chrome_android" | "android" | "chromeandroid" => {
            Some(StealthProfile::ChromeAndroid)
        }
        "safari-iphone" | "safari_iphone" | "iphone" | "safariiphone" => {
            Some(StealthProfile::SafariIphone)
        }
        "safari-ipad" | "safari_ipad" | "ipad" | "safariipad" => Some(StealthProfile::SafariIpad),
        "safari" | "safari-mac" | "safari_17_5" | "safari_mac" | "safarimacstable" => {
            Some(StealthProfile::SafariMacStable)
        }
        "chrome-linux" | "chrome_linux" | "chromelinux" => Some(StealthProfile::ChromeLinux),
        "brave" | "brave-windows" | "brave_windows" | "bravewindows" => {
            Some(StealthProfile::BraveWindows)
        }
        "opera" | "opera-windows" | "opera_windows" | "operawindows" => {
            Some(StealthProfile::OperaWindows)
        }
        "samsung-internet" | "samsung_internet" | "samsung" | "samsunginternetandroid" => {
            Some(StealthProfile::SamsungInternetAndroid)
        }
        _ => None,
    }
}

/// Stable browser identity facts shared by HTTP clients and browser
/// fingerprint layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileFacts {
    /// Canonical User-Agent string.
    pub user_agent: &'static str,
    /// `navigator.platform` value coherent with the User-Agent OS family.
    pub platform: &'static str,
    /// `navigator.languages` / Accept-Language base language list.
    pub languages: &'static [&'static str],
    /// Browser-shaped Accept header for top-level document navigation.
    pub accept: &'static str,
    /// Browser-shaped Accept-Language header for top-level document navigation.
    pub accept_language: &'static str,
    /// Browser-shaped Accept-Encoding header for top-level document navigation.
    pub accept_encoding: &'static str,
    /// `userAgentData.mobile` browser-family flag.
    pub mobile: bool,
    /// Default screen width for this persona.
    pub screen_width: u32,
    /// Default screen height for this persona.
    pub screen_height: u32,
}

/// Browser family parsed from a User-Agent string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserAgentBrowser {
    /// Chromium or Google Chrome.
    Chrome,
    /// Microsoft Edge.
    Edge,
    /// Mozilla Firefox.
    Firefox,
    /// Apple Safari.
    Safari,
    /// Internet Explorer.
    InternetExplorer,
    /// Opera.
    Opera,
    /// Samsung Internet.
    SamsungInternet,
    /// No supported browser token was present.
    Unknown,
}

/// Operating-system family parsed from a User-Agent string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserAgentPlatform {
    /// Android.
    Android,
    /// iPhone or iPad iOS/iPadOS.
    Ios,
    /// macOS.
    MacOs,
    /// Windows.
    Windows,
    /// Linux desktop.
    Linux,
    /// No supported platform token was present.
    Unknown,
}

impl UserAgentPlatform {
    /// Low-entropy Client Hint platform value, including browser-required quotes.
    #[must_use]
    pub const fn client_hint_value(self) -> Option<&'static str> {
        match self {
            Self::Android => Some("\"Android\""),
            Self::Ios => Some("\"iOS\""),
            Self::MacOs => Some("\"macOS\""),
            Self::Windows => Some("\"Windows\""),
            Self::Linux => Some("\"Linux\""),
            Self::Unknown => None,
        }
    }

    /// Platform label used by the Chrome TLS diagnostic catalogue.
    #[must_use]
    pub const fn chrome_tls_label(self) -> Option<&'static str> {
        match self {
            Self::Android => Some("Android"),
            Self::MacOs => Some("macOS"),
            Self::Windows => Some("Windows"),
            Self::Linux => Some("Linux"),
            Self::Ios | Self::Unknown => None,
        }
    }

    /// Whether this platform is a mobile browser surface.
    #[must_use]
    pub const fn is_mobile(self) -> bool {
        matches!(self, Self::Android | Self::Ios)
    }
}

/// Parsed browser identity facts from a User-Agent string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserAgentFacts {
    /// Parsed browser family.
    pub browser: UserAgentBrowser,
    /// Parsed platform family.
    pub platform: UserAgentPlatform,
    /// Major version for the parsed browser family.
    pub browser_major_version: Option<u32>,
    /// Major Chromium engine version when the UA carries a Chromium token.
    pub chromium_major_version: Option<u32>,
    /// Whether the UA leaks a headless Chromium token.
    pub headless: bool,
    /// Best-effort mapping to a canonical stealth profile.
    pub inferred_profile: Option<StealthProfile>,
    /// Whether the UA represents a mobile browser surface.
    pub mobile: bool,
}

impl UserAgentFacts {
    /// Client Hint platform value derived from the parsed platform.
    #[must_use]
    pub const fn client_hint_platform_value(self) -> Option<&'static str> {
        self.platform.client_hint_value()
    }

    /// Client Hint mobile value derived from the parsed platform and mobile token.
    #[must_use]
    pub const fn client_hint_mobile_value(self) -> &'static str {
        if self.mobile {
            "?1"
        } else {
            "?0"
        }
    }
}

/// Parse browser, platform, version, and stealth-profile facts from a User-Agent string.
#[must_use]
pub fn user_agent_facts(user_agent: &str) -> UserAgentFacts {
    let browser = user_agent_browser(user_agent);
    let platform = user_agent_platform(user_agent);
    let headless =
        user_agent.contains("HeadlessChrome/") || user_agent.contains("HeadlessChromium/");
    let mobile = platform.is_mobile() || user_agent.contains("Mobile");
    let chromium_major_version = first_major_after(
        user_agent,
        &[
            "HeadlessChrome/",
            "HeadlessChromium/",
            "Chrome/",
            "Chromium/",
            "CriOS/",
        ],
    );
    let browser_major_version = match browser {
        UserAgentBrowser::Chrome => chromium_major_version,
        UserAgentBrowser::Edge => first_major_after(user_agent, &["Edg/", "EdgA/", "EdgiOS/"]),
        UserAgentBrowser::Firefox => first_major_after(user_agent, &["Firefox/", "FxiOS/"]),
        UserAgentBrowser::Safari => major_after(user_agent, "Version/"),
        UserAgentBrowser::InternetExplorer => first_major_after(user_agent, &["MSIE ", "rv:"]),
        UserAgentBrowser::Opera => first_major_after(user_agent, &["OPR/", "OPiOS/", "OPT/"]),
        UserAgentBrowser::SamsungInternet => major_after(user_agent, "SamsungBrowser/"),
        UserAgentBrowser::Unknown => None,
    };

    UserAgentFacts {
        browser,
        platform,
        browser_major_version,
        chromium_major_version,
        headless,
        inferred_profile: profile_from_user_agent_facts(
            user_agent,
            browser,
            platform,
            browser_major_version,
        ),
        mobile,
    }
}

/// Infer the closest canonical stealth profile from a User-Agent string.
#[must_use]
pub fn infer_profile_from_user_agent(user_agent: &str) -> Option<StealthProfile> {
    user_agent_facts(user_agent).inferred_profile
}

fn user_agent_browser(user_agent: &str) -> UserAgentBrowser {
    if user_agent.contains("Trident/") || user_agent.contains("MSIE ") {
        UserAgentBrowser::InternetExplorer
    } else if user_agent.contains("Edg/")
        || user_agent.contains("EdgA/")
        || user_agent.contains("EdgiOS/")
    {
        UserAgentBrowser::Edge
    } else if user_agent.contains("SamsungBrowser/") {
        UserAgentBrowser::SamsungInternet
    } else if user_agent.contains("OPR/")
        || user_agent.contains("OPiOS/")
        || user_agent.contains("OPT/")
    {
        UserAgentBrowser::Opera
    } else if user_agent.contains("Firefox/") || user_agent.contains("FxiOS/") {
        UserAgentBrowser::Firefox
    } else if user_agent.contains("HeadlessChrome/")
        || user_agent.contains("HeadlessChromium/")
        || user_agent.contains("Chrome/")
        || user_agent.contains("Chromium/")
        || user_agent.contains("CriOS/")
    {
        UserAgentBrowser::Chrome
    } else if user_agent.contains("Safari/") && user_agent.contains("Version/") {
        UserAgentBrowser::Safari
    } else {
        UserAgentBrowser::Unknown
    }
}

fn user_agent_platform(user_agent: &str) -> UserAgentPlatform {
    if user_agent.contains("Android") {
        UserAgentPlatform::Android
    } else if user_agent.contains("iPhone")
        || user_agent.contains("iPad")
        || user_agent.contains("iPod")
        || user_agent.contains("iPhone OS")
        || user_agent.contains("CPU OS")
    {
        UserAgentPlatform::Ios
    } else if user_agent.contains("Macintosh") || user_agent.contains("Mac OS X") {
        UserAgentPlatform::MacOs
    } else if user_agent.contains("Windows") {
        UserAgentPlatform::Windows
    } else if user_agent.contains("Linux") || user_agent.contains("X11") {
        UserAgentPlatform::Linux
    } else {
        UserAgentPlatform::Unknown
    }
}

fn profile_from_user_agent_facts(
    user_agent: &str,
    browser: UserAgentBrowser,
    platform: UserAgentPlatform,
    browser_major_version: Option<u32>,
) -> Option<StealthProfile> {
    match browser {
        UserAgentBrowser::InternetExplorer => match platform {
            UserAgentPlatform::Windows => Some(StealthProfile::Ie11Windows),
            _ => None,
        },
        UserAgentBrowser::Edge => match platform {
            UserAgentPlatform::Windows => Some(StealthProfile::EdgeWindowsStable),
            _ => None,
        },
        UserAgentBrowser::Firefox => match platform {
            UserAgentPlatform::Windows => Some(StealthProfile::FirefoxWindows),
            UserAgentPlatform::MacOs => Some(StealthProfile::FirefoxMacStable),
            UserAgentPlatform::Linux => Some(StealthProfile::FirefoxLinux),
            _ => None,
        },
        UserAgentBrowser::Safari => {
            if user_agent.contains("iPhone") || user_agent.contains("iPod") {
                Some(StealthProfile::SafariIphone)
            } else if user_agent.contains("iPad") {
                Some(StealthProfile::SafariIpad)
            } else if platform == UserAgentPlatform::MacOs {
                Some(StealthProfile::SafariMacStable)
            } else {
                None
            }
        }
        UserAgentBrowser::SamsungInternet => match platform {
            UserAgentPlatform::Android => Some(StealthProfile::SamsungInternetAndroid),
            _ => None,
        },
        UserAgentBrowser::Opera => match platform {
            UserAgentPlatform::Windows => Some(StealthProfile::OperaWindows),
            _ => None,
        },
        UserAgentBrowser::Chrome => match platform {
            UserAgentPlatform::Android => Some(StealthProfile::ChromeAndroid),
            UserAgentPlatform::MacOs => Some(StealthProfile::ChromeMacStable),
            UserAgentPlatform::Linux => Some(StealthProfile::ChromeLinux),
            UserAgentPlatform::Windows => {
                if browser_major_version.is_some_and(|major| major <= 96) {
                    Some(StealthProfile::ChromeWindowsLegacy96)
                } else {
                    Some(StealthProfile::ChromeWindowsStable)
                }
            }
            _ => None,
        },
        UserAgentBrowser::Unknown => None,
    }
}

fn first_major_after(user_agent: &str, tokens: &[&str]) -> Option<u32> {
    tokens
        .iter()
        .find_map(|token| major_after(user_agent, token))
}
fn major_after(user_agent: &str, token: &str) -> Option<u32> {
    let rest = user_agent.split_once(token)?.1;
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// HTTP header name for the profile User-Agent identity header.
pub const USER_AGENT_HEADER: &str = "user-agent";

/// HTTP header name for the profile top-level navigation Accept header.
pub const ACCEPT_HEADER: &str = "accept";

/// HTTP header name for the profile Accept-Language identity header.
pub const ACCEPT_LANGUAGE_HEADER: &str = "accept-language";

/// HTTP header name for the profile Accept-Encoding negotiation header.
pub const ACCEPT_ENCODING_HEADER: &str = "accept-encoding";

/// HTTP header name for `Upgrade-Insecure-Requests`.
pub const UPGRADE_INSECURE_REQUESTS_HEADER: &str = "upgrade-insecure-requests";

/// HTTP header name for `Sec-Fetch-Dest`.
pub const SEC_FETCH_DEST_HEADER: &str = "sec-fetch-dest";

/// HTTP header name for `Sec-Fetch-Mode`.
pub const SEC_FETCH_MODE_HEADER: &str = "sec-fetch-mode";

/// HTTP header name for `Sec-Fetch-Site`.
pub const SEC_FETCH_SITE_HEADER: &str = "sec-fetch-site";

/// HTTP header name for `Sec-Fetch-User`.
pub const SEC_FETCH_USER_HEADER: &str = "sec-fetch-user";

/// Browser request surface to materialize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BrowserRequestKind {
    /// Top-level document navigation.
    Navigation,
    /// Top-level document navigation from the same origin.
    SameOriginNavigation,
    /// Top-level document navigation from another site.
    CrossSiteNavigation,
    /// Same-origin `fetch`/XHR request to an application endpoint.
    SameOriginFetch,
    /// Same-origin `fetch`/XHR request with explicit `same-origin` mode.
    SameOriginModeFetch,
    /// Cross-site `fetch`/XHR request to an application endpoint.
    CrossSiteFetch,
    /// Image element fetch for an image resource.
    ImageSubresource,
    /// Media element fetch for an audio resource.
    AudioSubresource,
}

/// Browser-compatible display casing for a canonical navigation header name.
///
/// Profile catalog header names stay lower-case so `http::HeaderName::from_static`
/// can consume them directly. String-map transports that preserve caller-facing
/// header names use this helper to emit the same browser-shaped casing everywhere.
#[must_use]
pub fn canonical_navigation_header_name(name: &str) -> &str {
    match name {
        USER_AGENT_HEADER => "User-Agent",
        ACCEPT_HEADER => "Accept",
        ACCEPT_LANGUAGE_HEADER => "Accept-Language",
        ACCEPT_ENCODING_HEADER => "Accept-Encoding",
        UPGRADE_INSECURE_REQUESTS_HEADER => "Upgrade-Insecure-Requests",
        SEC_FETCH_DEST_HEADER => "Sec-Fetch-Dest",
        SEC_FETCH_MODE_HEADER => "Sec-Fetch-Mode",
        SEC_FETCH_SITE_HEADER => "Sec-Fetch-Site",
        SEC_FETCH_USER_HEADER => "Sec-Fetch-User",
        _ => name,
    }
}

/// Header/value pair for browser identity defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavigationHeader {
    /// Lower-case HTTP header name suitable for `http::HeaderName::from_static`.
    pub name: &'static str,
    /// Header value derived from the selected browser profile.
    pub value: &'static str,
}

const EMPTY_HEADER: NavigationHeader = NavigationHeader {
    name: "",
    value: "",
};

/// Fixed browser request header set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserRequestHeaders {
    entries: [NavigationHeader; 9],
    len: usize,
}

impl BrowserRequestHeaders {
    /// Borrow only the populated header entries.
    #[must_use]
    pub fn as_slice(&self) -> &[NavigationHeader] {
        &self.entries[..self.len]
    }

    /// Number of populated header entries.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// True if no entries are populated.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

const WILDCARD_ACCEPT: &str = "*/*";
const UPGRADE_INSECURE_REQUESTS_VALUE: &str = "1";
const DOCUMENT_DEST_VALUE: &str = "document";
const EMPTY_DEST_VALUE: &str = "empty";
const IMAGE_DEST_VALUE: &str = "image";
const AUDIO_DEST_VALUE: &str = "audio";
const NAVIGATE_MODE_VALUE: &str = "navigate";
const CORS_MODE_VALUE: &str = "cors";
const SAME_ORIGIN_MODE_VALUE: &str = "same-origin";
const NO_CORS_MODE_VALUE: &str = "no-cors";
const NONE_SITE_VALUE: &str = "none";
const SAME_ORIGIN_SITE_VALUE: &str = "same-origin";
const CROSS_SITE_VALUE: &str = "cross-site";
const FETCH_USER_ACTIVATED_VALUE: &str = "?1";

const EN_US_EN: &[&str] = &["en-US", "en"];

/// Chromium-class top-level navigation Accept header.
pub const CHROMIUM_NAVIGATION_ACCEPT: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7";

/// Firefox top-level navigation Accept header.
///
/// Measured from stock Firefox 151.0.3 (Linux, headless) on a top-level document
/// navigation: image/avif and image/webp are NOT advertised here; they are
/// emitted on image-element fetches via [`FIREFOX_IMAGE_ACCEPT`]. Using the
/// image-heavy Accept for navigation requests would be a catalogue drift tell.
pub const FIREFOX_NAVIGATION_ACCEPT: &str =
    "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";

/// Internet Explorer 11 top-level navigation Accept header.
pub const IE11_NAVIGATION_ACCEPT: &str = "text/html, application/xhtml+xml, */*";

/// Safari top-level navigation Accept header.
pub const SAFARI_NAVIGATION_ACCEPT: &str =
    "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";

/// Default Chromium/Safari Accept-Language weighting.
pub const DEFAULT_ACCEPT_LANGUAGE: &str = "en-US,en;q=0.9";

/// Default Firefox Accept-Language weighting.
///
/// Measured from stock Firefox 151.0.3: the secondary tag carries q=0.9, not the
/// older 0.5 weight used by legacy Firefox.
pub const FIREFOX_ACCEPT_LANGUAGE: &str = "en-US,en;q=0.9";

/// Chromium `<img>`-element Accept header (`kImageAcceptHeader`).
///
/// Real Chrome/Edge/Brave/etc. request an image element with this exact
/// resource-specific Accept, never a bare `*/*`, which would itself be a
/// fetch-metadata tell. Stable across modern Chromium.
pub const CHROMIUM_IMAGE_ACCEPT: &str =
    "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8";

/// Firefox `<img>`-element Accept header (`image.http.accept`).
///
/// Modern Firefox (≥92, incl. 133) requests an image element with this exact
/// Accept. Distinct from Chromium's, so it must track the persona's family.
pub const FIREFOX_IMAGE_ACCEPT: &str = "image/avif,image/webp,*/*";

/// Default browser Accept-Encoding set for shared Santh scanner transports.
pub const DEFAULT_ACCEPT_ENCODING: &str = "gzip, deflate, br";

/// Legacy browser Accept-Encoding set for pre-Brotli navigation stacks.
pub const LEGACY_ACCEPT_ENCODING: &str = "gzip, deflate";

/// Canonical browser HTTP **header projection** of a [`StealthProfile`].
///
/// One of the `<Domain>Profile` facets (see the crate-level vocabulary): a pure,
/// `const`-friendly view of the request headers a given browser identity sends.
/// `scanclient`/`karyx` re-export this directly (header data without the heavy
/// `stealth` stack); `stealth::http`/`stealth::fingerprint` re-export it as the
/// single header-catalog type, it replaces the former duplicate `BrowserProfile`
/// structs in `stealth::fingerprint::browser_catalog` and this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderProfile {
    /// Stable profile name used by config and CLI flags.
    pub name: &'static str,
    /// User-Agent header value.
    pub user_agent: &'static str,
    /// Accept header value.
    pub accept: &'static str,
    /// Accept-Language header value.
    pub accept_language: &'static str,
    /// Accept-Encoding header value.
    pub accept_encoding: &'static str,
    /// `Sec-Fetch-Site` navigation value.
    pub sec_fetch_site: &'static str,
    /// `Sec-Fetch-Mode` navigation value.
    pub sec_fetch_mode: &'static str,
    /// `Sec-Fetch-Dest` navigation value.
    pub sec_fetch_dest: &'static str,
}

impl HeaderProfile {
    /// Browser-shaped HTTP headers represented by this compatibility profile.
    ///
    /// Unlike [`profile_navigation_headers`], this legacy compatibility view
    /// includes `Accept-Encoding` because `scanclient::tls_profiles` has
    /// exposed that field since before the canonical profile catalog existed.
    #[must_use]
    pub const fn headers(self) -> [NavigationHeader; 4] {
        [
            NavigationHeader {
                name: USER_AGENT_HEADER,
                value: self.user_agent,
            },
            NavigationHeader {
                name: ACCEPT_HEADER,
                value: self.accept,
            },
            NavigationHeader {
                name: ACCEPT_LANGUAGE_HEADER,
                value: self.accept_language,
            },
            NavigationHeader {
                name: ACCEPT_ENCODING_HEADER,
                value: self.accept_encoding,
            },
        ]
    }
}

/// Common browser HTTP profiles used by scanner transports.
const ALL_HEADER_PROFILES: &[HeaderProfile] = &[
    browser_profile("chrome", StealthProfile::ChromeWindowsStable),
    browser_profile(
        "chrome-windows-legacy-96",
        StealthProfile::ChromeWindowsLegacy96,
    ),
    browser_profile("chrome-macos", StealthProfile::ChromeMacStable),
    browser_profile("edge", StealthProfile::EdgeWindowsStable),
    browser_profile("ie11-windows", StealthProfile::Ie11Windows),
    browser_profile("firefox", StealthProfile::FirefoxLinux),
    browser_profile("firefox-windows", StealthProfile::FirefoxWindows),
    browser_profile("firefox-macos", StealthProfile::FirefoxMacStable),
    browser_profile("chrome-android", StealthProfile::ChromeAndroid),
    browser_profile("safari-iphone", StealthProfile::SafariIphone),
    browser_profile("safari-ipad", StealthProfile::SafariIpad),
    browser_profile("safari", StealthProfile::SafariMacStable),
    browser_profile("chrome-linux", StealthProfile::ChromeLinux),
    browser_profile("brave", StealthProfile::BraveWindows),
    browser_profile("opera", StealthProfile::OperaWindows),
    browser_profile("samsung-internet", StealthProfile::SamsungInternetAndroid),
];

/// Common browser HTTP profiles used by scanner transports.
pub static PROFILES: &[HeaderProfile] = ALL_HEADER_PROFILES;

const DEFAULT_PROFILE: HeaderProfile = browser_profile("default", DEFAULT_STEALTH_PROFILE);

const fn browser_profile(name: &'static str, profile: StealthProfile) -> HeaderProfile {
    let facts = profile_facts(profile);
    HeaderProfile {
        name,
        user_agent: facts.user_agent,
        accept: facts.accept,
        accept_language: facts.accept_language,
        accept_encoding: facts.accept_encoding,
        sec_fetch_site: "none",
        sec_fetch_mode: "navigate",
        sec_fetch_dest: "document",
    }
}

/// Resolve a common browser HTTP profile by its stable config name.
#[must_use]
pub fn get_profile(name: &str) -> Option<&'static HeaderProfile> {
    let profile = named_profile(name)?;
    let index = ALL_PROFILES.iter().position(|&p| p == profile)?;
    Some(&ALL_HEADER_PROFILES[index])
}

/// Deterministically rotate through common browser HTTP profiles.
#[must_use]
pub fn rotate(index: usize) -> &'static HeaderProfile {
    if PROFILES.is_empty() {
        return &DEFAULT_PROFILE;
    }
    &PROFILES[index % PROFILES.len()]
}

/// A coherent hardware/display tuple for a browser fingerprint profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileHardware {
    /// `screen.width`.
    pub screen_width: u32,
    /// `screen.height`.
    pub screen_height: u32,
    /// `screen.colorDepth` / `screen.pixelDepth`.
    pub color_depth: u8,
    /// `navigator.deviceMemory` GB.
    pub device_memory: u8,
    /// `navigator.hardwareConcurrency`.
    pub hardware_concurrency: u8,
    /// WebGL `UNMASKED_VENDOR_WEBGL`.
    pub webgl_vendor: &'static str,
    /// WebGL `UNMASKED_RENDERER_WEBGL`.
    pub webgl_renderer: &'static str,
}

const CHROME_WINDOWS_HARDWARE: &[ProfileHardware] = &[
    ProfileHardware {
        screen_width: 1920,
        screen_height: 1080,
        color_depth: 24,
        device_memory: 8,
        hardware_concurrency: 8,
        webgl_vendor: "Google Inc. (Intel)",
        webgl_renderer:
            "ANGLE (Intel, Intel(R) Iris(R) Xe Graphics Direct3D11 vs_5_0 ps_5_0, D3D11)",
    },
    ProfileHardware {
        screen_width: 1920,
        screen_height: 1080,
        color_depth: 24,
        device_memory: 16,
        hardware_concurrency: 12,
        webgl_vendor: "Google Inc. (NVIDIA)",
        webgl_renderer: "ANGLE (NVIDIA, NVIDIA GeForce RTX 3060 Direct3D11 vs_5_0 ps_5_0, D3D11)",
    },
    ProfileHardware {
        screen_width: 2560,
        screen_height: 1440,
        color_depth: 24,
        device_memory: 16,
        hardware_concurrency: 16,
        webgl_vendor: "Google Inc. (AMD)",
        webgl_renderer: "ANGLE (AMD, AMD Radeon RX 6700 XT Direct3D11 vs_5_0 ps_5_0, D3D11)",
    },
    ProfileHardware {
        screen_width: 1366,
        screen_height: 768,
        color_depth: 24,
        device_memory: 8,
        hardware_concurrency: 8,
        webgl_vendor: "Google Inc. (Intel)",
        webgl_renderer: "ANGLE (Intel, Intel(R) UHD Graphics 630 Direct3D11 vs_5_0 ps_5_0, D3D11)",
    },
    ProfileHardware {
        screen_width: 1920,
        screen_height: 1080,
        color_depth: 24,
        device_memory: 32,
        hardware_concurrency: 16,
        webgl_vendor: "Google Inc. (NVIDIA)",
        webgl_renderer: "ANGLE (NVIDIA, NVIDIA GeForce RTX 4070 Direct3D11 vs_5_0 ps_5_0, D3D11)",
    },
];

const CHROME_WINDOWS_LEGACY_96_HARDWARE: &[ProfileHardware] = &[ProfileHardware {
    screen_width: 1366,
    screen_height: 768,
    color_depth: 24,
    device_memory: 8,
    hardware_concurrency: 8,
    webgl_vendor: "Google Inc. (Intel)",
    webgl_renderer: "ANGLE (Intel, Intel(R) UHD Graphics 630 Direct3D11 vs_5_0 ps_5_0, D3D11)",
}];

const IE11_WINDOWS_HARDWARE: &[ProfileHardware] = &[ProfileHardware {
    screen_width: 1366,
    screen_height: 768,
    color_depth: 24,
    device_memory: 4,
    hardware_concurrency: 4,
    webgl_vendor: "Microsoft",
    webgl_renderer: "Internet Explorer 11",
}];

const CHROME_MAC_HARDWARE: &[ProfileHardware] = &[ProfileHardware {
    screen_width: 1728,
    screen_height: 1117,
    color_depth: 30,
    device_memory: 16,
    hardware_concurrency: 10,
    webgl_vendor: "Google Inc. (Apple)",
    webgl_renderer: "ANGLE (Apple, ANGLE Metal Renderer: Apple M1 Pro, Unspecified Version)",
}];

const EDGE_WINDOWS_HARDWARE: &[ProfileHardware] = &[ProfileHardware {
    screen_width: 1920,
    screen_height: 1080,
    color_depth: 24,
    device_memory: 8,
    hardware_concurrency: 8,
    webgl_vendor: "Google Inc. (Intel)",
    webgl_renderer: "ANGLE (Intel, Intel(R) UHD Graphics Direct3D11 vs_5_0 ps_5_0, D3D11)",
}];

const FIREFOX_LINUX_HARDWARE: &[ProfileHardware] = &[ProfileHardware {
    screen_width: 1920,
    screen_height: 1080,
    color_depth: 24,
    device_memory: 8,
    hardware_concurrency: 8,
    // Native passthrough: empty = expose the host's real, Gecko-sanitized WebGL
    // adapter rather than a constant. FirefoxLinux is a matched-host persona
    // (Firefox on Linux, run on Firefox/Linux), so the real adapter
    // ("NVIDIA Corporation" / "NVIDIA GeForce <card>, or similar") is already
    // low-entropy AND its rendered pixels match, strictly more coherent than
    // the former "Mesa Intel Iris Xe" string, which both claimed an iGPU the
    // host may not have and contradicted the actual pixels. A genuinely
    // cross-OS Firefox persona must carry a coherent renderer for its claimed
    // OS instead (see `profile_js`, which only pins WebGL when this is set).
    webgl_vendor: "",
    webgl_renderer: "",
}];

const FIREFOX_WINDOWS_HARDWARE: &[ProfileHardware] = &[ProfileHardware {
    screen_width: 1920,
    screen_height: 1080,
    color_depth: 24,
    device_memory: 8,
    hardware_concurrency: 8,
    webgl_vendor: "Google Inc. (Intel)",
    webgl_renderer: "ANGLE (Intel, Intel(R) UHD Graphics Direct3D11 vs_5_0 ps_5_0, D3D11)",
}];

const FIREFOX_MAC_HARDWARE: &[ProfileHardware] = &[ProfileHardware {
    screen_width: 1728,
    screen_height: 1117,
    color_depth: 30,
    device_memory: 16,
    hardware_concurrency: 10,
    webgl_vendor: "Apple Inc.",
    webgl_renderer: "Apple GPU",
}];

const CHROME_ANDROID_HARDWARE: &[ProfileHardware] = &[ProfileHardware {
    screen_width: 412,
    screen_height: 915,
    color_depth: 24,
    device_memory: 6,
    hardware_concurrency: 8,
    webgl_vendor: "Qualcomm",
    webgl_renderer: "Adreno (TM) 740",
}];

const SAFARI_IPHONE_HARDWARE: &[ProfileHardware] = &[ProfileHardware {
    screen_width: 390,
    screen_height: 844,
    color_depth: 24,
    device_memory: 4,
    hardware_concurrency: 6,
    webgl_vendor: "Apple Inc.",
    webgl_renderer: "Apple GPU",
}];

const SAFARI_IPAD_HARDWARE: &[ProfileHardware] = &[ProfileHardware {
    screen_width: 1024,
    screen_height: 1366,
    color_depth: 24,
    device_memory: 8,
    hardware_concurrency: 8,
    webgl_vendor: "Apple Inc.",
    webgl_renderer: "Apple GPU",
}];

const SAFARI_MAC_HARDWARE: &[ProfileHardware] = &[ProfileHardware {
    screen_width: 1728,
    screen_height: 1117,
    color_depth: 30,
    device_memory: 16,
    hardware_concurrency: 10,
    webgl_vendor: "Apple Inc.",
    webgl_renderer: "Apple M2",
}];

const CHROME_LINUX_HARDWARE: &[ProfileHardware] = &[
    ProfileHardware {
        screen_width: 1920,
        screen_height: 1080,
        color_depth: 24,
        device_memory: 8,
        hardware_concurrency: 8,
        webgl_vendor: "Mesa",
        webgl_renderer: "Mesa Intel(R) UHD Graphics 770 (ADL-S GT1)",
    },
    ProfileHardware {
        screen_width: 1920,
        screen_height: 1080,
        color_depth: 24,
        device_memory: 32,
        hardware_concurrency: 8,
        webgl_vendor: "Google Inc. (NVIDIA)",
        webgl_renderer: "ANGLE (NVIDIA, NVIDIA GeForce GTX 1660 SUPER/PCIe/SSE2, OpenGL 4.5)",
    },
];

const BRAVE_WINDOWS_HARDWARE: &[ProfileHardware] = &[ProfileHardware {
    screen_width: 1920,
    screen_height: 1080,
    color_depth: 24,
    device_memory: 8,
    hardware_concurrency: 8,
    webgl_vendor: "Brave",
    webgl_renderer: "Brave",
}];

const OPERA_WINDOWS_HARDWARE: &[ProfileHardware] = &[ProfileHardware {
    screen_width: 1920,
    screen_height: 1080,
    color_depth: 24,
    device_memory: 8,
    hardware_concurrency: 8,
    webgl_vendor: "Google Inc. (Intel)",
    webgl_renderer: "ANGLE (Intel, Intel(R) UHD Graphics 770 Direct3D11 vs_5_0 ps_5_0, D3D11)",
}];

const SAMSUNG_INTERNET_HARDWARE: &[ProfileHardware] = &[ProfileHardware {
    screen_width: 412,
    screen_height: 915,
    color_depth: 24,
    device_memory: 8,
    hardware_concurrency: 8,
    webgl_vendor: "Qualcomm",
    webgl_renderer: "Adreno (TM) 750",
}];

/// Hardware/display tuples coherent with a browser fingerprint profile.
///
/// Every table is compile-time guaranteed non-empty (the assertions below),
/// so [`profile_hardware`] and [`profile_hardware_at`] can never panic on an
/// empty table: an empty table is a compile error, not a latent runtime
/// panic. This matches the crate's fail-closed-at-compile-time discipline
/// (`profile_os_network_stack` panics in const for an unknown platform).
#[must_use]
pub const fn profile_hardware_variants(profile: StealthProfile) -> &'static [ProfileHardware] {
    match profile {
        StealthProfile::ChromeWindowsStable => CHROME_WINDOWS_HARDWARE,
        StealthProfile::ChromeWindowsLegacy96 => CHROME_WINDOWS_LEGACY_96_HARDWARE,
        StealthProfile::ChromeMacStable => CHROME_MAC_HARDWARE,
        StealthProfile::EdgeWindowsStable => EDGE_WINDOWS_HARDWARE,
        StealthProfile::Ie11Windows => IE11_WINDOWS_HARDWARE,
        StealthProfile::FirefoxLinux => FIREFOX_LINUX_HARDWARE,
        StealthProfile::FirefoxWindows => FIREFOX_WINDOWS_HARDWARE,
        StealthProfile::FirefoxMacStable => FIREFOX_MAC_HARDWARE,
        StealthProfile::ChromeAndroid => CHROME_ANDROID_HARDWARE,
        StealthProfile::SafariIphone => SAFARI_IPHONE_HARDWARE,
        StealthProfile::SafariIpad => SAFARI_IPAD_HARDWARE,
        StealthProfile::SafariMacStable => SAFARI_MAC_HARDWARE,
        StealthProfile::ChromeLinux => CHROME_LINUX_HARDWARE,
        StealthProfile::BraveWindows => BRAVE_WINDOWS_HARDWARE,
        StealthProfile::OperaWindows => OPERA_WINDOWS_HARDWARE,
        StealthProfile::SamsungInternetAndroid => SAMSUNG_INTERNET_HARDWARE,
    }
}

/// Default hardware/display tuple for a browser fingerprint profile.
#[must_use]
pub const fn profile_hardware(profile: StealthProfile) -> ProfileHardware {
    profile_hardware_variants(profile)[0]
}

/// Deterministically select a profile-coherent hardware/display tuple.
#[must_use]
pub const fn profile_hardware_at(profile: StealthProfile, index: usize) -> ProfileHardware {
    let variants = profile_hardware_variants(profile);
    variants[index % variants.len()]
}

// Compile-time non-emptiness guarantee for every per-profile hardware table
// (BACKLOG robustness/panic row): `profile_hardware` indexes `[0]` and
// `profile_hardware_at` reduces modulo `len`, so an empty table would be a
// latent index-out-of-bounds or divide-by-zero panic with no compile-time
// signal. Any new `StealthProfile` variant must add its table here.
const _: () = {
    let mut i = 0;
    while i < ALL_PROFILES.len() {
        assert!(!profile_hardware_variants(ALL_PROFILES[i]).is_empty());
        i += 1;
    }
};

/// One low-entropy User-Agent Client Hint brand entry for a browser profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileClientHintBrand {
    /// Browser brand token.
    pub brand: &'static str,
    /// Major version string paired with the brand.
    pub version: &'static str,
}

const NO_CLIENT_HINT_BRANDS: &[ProfileClientHintBrand] = &[];

const CHROMIUM_131_BRANDS: &[ProfileClientHintBrand] = &[
    ProfileClientHintBrand {
        brand: "Chromium",
        version: "131",
    },
    ProfileClientHintBrand {
        brand: "Google Chrome",
        version: "131",
    },
    ProfileClientHintBrand {
        brand: "Not?A_Brand",
        version: "99",
    },
];

const CHROMIUM_96_BRANDS: &[ProfileClientHintBrand] = &[
    ProfileClientHintBrand {
        brand: "Chromium",
        version: "96",
    },
    ProfileClientHintBrand {
        brand: "Google Chrome",
        version: "96",
    },
    ProfileClientHintBrand {
        brand: "Not?A_Brand",
        version: "99",
    },
];

const EDGE_131_BRANDS: &[ProfileClientHintBrand] = &[
    ProfileClientHintBrand {
        brand: "Chromium",
        version: "131",
    },
    ProfileClientHintBrand {
        brand: "Microsoft Edge",
        version: "131",
    },
    ProfileClientHintBrand {
        brand: "Not?A_Brand",
        version: "99",
    },
];

const BRAVE_131_BRANDS: &[ProfileClientHintBrand] = &[
    ProfileClientHintBrand {
        brand: "Brave",
        version: "131",
    },
    ProfileClientHintBrand {
        brand: "Chromium",
        version: "131",
    },
    ProfileClientHintBrand {
        brand: "Not?A_Brand",
        version: "99",
    },
];

const OPERA_116_BRANDS: &[ProfileClientHintBrand] = &[
    ProfileClientHintBrand {
        brand: "Chromium",
        version: "131",
    },
    ProfileClientHintBrand {
        brand: "Opera",
        version: "116",
    },
    ProfileClientHintBrand {
        brand: "Not?A_Brand",
        version: "99",
    },
];

const SAMSUNG_INTERNET_26_BRANDS: &[ProfileClientHintBrand] = &[
    ProfileClientHintBrand {
        brand: "Samsung Internet",
        version: "26",
    },
    ProfileClientHintBrand {
        brand: "Chromium",
        version: "126",
    },
    ProfileClientHintBrand {
        brand: "Not?A_Brand",
        version: "99",
    },
];

/// `navigator.vendor` value coherent with a browser fingerprint profile.
#[must_use]
pub const fn profile_navigator_vendor(profile: StealthProfile) -> &'static str {
    match profile {
        StealthProfile::ChromeWindowsStable
        | StealthProfile::ChromeWindowsLegacy96
        | StealthProfile::ChromeMacStable
        | StealthProfile::EdgeWindowsStable
        | StealthProfile::ChromeAndroid
        | StealthProfile::ChromeLinux
        | StealthProfile::BraveWindows
        | StealthProfile::OperaWindows
        | StealthProfile::SamsungInternetAndroid => "Google Inc.",
        StealthProfile::SafariIphone
        | StealthProfile::SafariIpad
        | StealthProfile::SafariMacStable => "Apple Computer, Inc.",
        StealthProfile::Ie11Windows
        | StealthProfile::FirefoxLinux
        | StealthProfile::FirefoxWindows
        | StealthProfile::FirefoxMacStable => "",
    }
}

/// Low-entropy User-Agent Client Hint brands coherent with a browser profile.
#[must_use]
pub const fn profile_client_hint_brands(
    profile: StealthProfile,
) -> &'static [ProfileClientHintBrand] {
    match profile {
        StealthProfile::ChromeWindowsStable
        | StealthProfile::ChromeMacStable
        | StealthProfile::ChromeAndroid
        | StealthProfile::ChromeLinux => CHROMIUM_131_BRANDS,
        StealthProfile::ChromeWindowsLegacy96 => CHROMIUM_96_BRANDS,
        StealthProfile::EdgeWindowsStable => EDGE_131_BRANDS,
        StealthProfile::BraveWindows => BRAVE_131_BRANDS,
        StealthProfile::OperaWindows => OPERA_116_BRANDS,
        StealthProfile::SamsungInternetAndroid => SAMSUNG_INTERNET_26_BRANDS,
        StealthProfile::Ie11Windows
        | StealthProfile::FirefoxLinux
        | StealthProfile::FirefoxWindows
        | StealthProfile::FirefoxMacStable
        | StealthProfile::SafariIphone
        | StealthProfile::SafariIpad
        | StealthProfile::SafariMacStable => NO_CLIENT_HINT_BRANDS,
    }
}

/// Low-entropy `Sec-CH-UA-Platform` value coherent with a browser profile.
#[must_use]
pub const fn profile_client_hint_platform(profile: StealthProfile) -> Option<&'static str> {
    match profile {
        StealthProfile::ChromeWindowsStable
        | StealthProfile::ChromeWindowsLegacy96
        | StealthProfile::EdgeWindowsStable
        | StealthProfile::BraveWindows
        | StealthProfile::OperaWindows => Some("Windows"),
        StealthProfile::ChromeMacStable => Some("macOS"),
        StealthProfile::ChromeAndroid | StealthProfile::SamsungInternetAndroid => Some("Android"),
        StealthProfile::ChromeLinux => Some("Linux"),
        StealthProfile::Ie11Windows
        | StealthProfile::FirefoxLinux
        | StealthProfile::FirefoxWindows
        | StealthProfile::FirefoxMacStable
        | StealthProfile::SafariIphone
        | StealthProfile::SafariIpad
        | StealthProfile::SafariMacStable => None,
    }
}

/// Operating-system family a stealth profile claims, derived directly from the
/// persona without User-Agent string parsing.
///
/// This is the pure, total inverse of the OS encoded in each persona's
/// User-Agent: it never returns [`UserAgentPlatform::Unknown`] (every persona
/// has a concrete OS) and is `const`, so the transport layer can resolve a
/// persona's OS family without round-tripping through the UA parser. It agrees
/// with `user_agent_facts(profile_user_agent(p)).platform` for every persona.
#[must_use]
pub const fn profile_platform(profile: StealthProfile) -> UserAgentPlatform {
    match profile {
        StealthProfile::ChromeWindowsStable
        | StealthProfile::ChromeWindowsLegacy96
        | StealthProfile::EdgeWindowsStable
        | StealthProfile::Ie11Windows
        | StealthProfile::FirefoxWindows
        | StealthProfile::BraveWindows
        | StealthProfile::OperaWindows => UserAgentPlatform::Windows,
        StealthProfile::ChromeMacStable
        | StealthProfile::SafariMacStable
        | StealthProfile::FirefoxMacStable => UserAgentPlatform::MacOs,
        StealthProfile::FirefoxLinux | StealthProfile::ChromeLinux => UserAgentPlatform::Linux,
        StealthProfile::ChromeAndroid | StealthProfile::SamsungInternetAndroid => {
            UserAgentPlatform::Android
        }
        StealthProfile::SafariIphone | StealthProfile::SafariIpad => UserAgentPlatform::Ios,
    }
}

/// Canonical User-Agent for [`StealthProfile::ChromeWindowsStable`].
pub const CHROME_WINDOWS_STABLE_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                         (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// Canonical User-Agent for [`StealthProfile::ChromeWindowsLegacy96`].
pub const CHROME_WINDOWS_LEGACY_96_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
     AppleWebKit/537.36 (KHTML, like Gecko) Chrome/96.0.4664.110 Safari/537.36";

/// Canonical User-Agent for [`StealthProfile::FirefoxWindows`].
pub const FIREFOX_WINDOWS_STABLE_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:150.0) Gecko/20100101 Firefox/150.0";

/// Canonical User-Agent for [`StealthProfile::Ie11Windows`].
pub const IE11_WINDOWS_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 6.3; Trident/7.0; rv:11.0) like Gecko";

/// Canonical identity facts for a stealth profile.
#[must_use]
pub const fn profile_facts(profile: StealthProfile) -> ProfileFacts {
    match profile {
        StealthProfile::ChromeWindowsStable => ProfileFacts {
            user_agent: CHROME_WINDOWS_STABLE_USER_AGENT,
            platform: "Win32",
            languages: EN_US_EN,
            accept: CHROMIUM_NAVIGATION_ACCEPT,
            accept_language: DEFAULT_ACCEPT_LANGUAGE,
            accept_encoding: DEFAULT_ACCEPT_ENCODING,
            mobile: false,
            screen_width: 1920,
            screen_height: 1080,
        },
        StealthProfile::ChromeWindowsLegacy96 => ProfileFacts {
            user_agent: CHROME_WINDOWS_LEGACY_96_USER_AGENT,
            platform: "Win32",
            languages: EN_US_EN,
            accept: CHROMIUM_NAVIGATION_ACCEPT,
            accept_language: DEFAULT_ACCEPT_LANGUAGE,
            accept_encoding: DEFAULT_ACCEPT_ENCODING,
            mobile: false,
            screen_width: 1366,
            screen_height: 768,
        },
        StealthProfile::ChromeMacStable => ProfileFacts {
            user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
                         (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
            platform: "MacIntel",
            languages: EN_US_EN,
            accept: CHROMIUM_NAVIGATION_ACCEPT,
            accept_language: DEFAULT_ACCEPT_LANGUAGE,
            accept_encoding: DEFAULT_ACCEPT_ENCODING,
            mobile: false,
            screen_width: 1728,
            screen_height: 1117,
        },
        StealthProfile::EdgeWindowsStable => ProfileFacts {
            user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                         (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0",
            platform: "Win32",
            languages: EN_US_EN,
            accept: CHROMIUM_NAVIGATION_ACCEPT,
            accept_language: DEFAULT_ACCEPT_LANGUAGE,
            accept_encoding: DEFAULT_ACCEPT_ENCODING,
            mobile: false,
            screen_width: 1920,
            screen_height: 1080,
        },
        StealthProfile::Ie11Windows => ProfileFacts {
            user_agent: IE11_WINDOWS_USER_AGENT,
            platform: "Win32",
            languages: EN_US_EN,
            accept: IE11_NAVIGATION_ACCEPT,
            accept_language: DEFAULT_ACCEPT_LANGUAGE,
            accept_encoding: LEGACY_ACCEPT_ENCODING,
            mobile: false,
            screen_width: 1366,
            screen_height: 768,
        },
        StealthProfile::FirefoxLinux => ProfileFacts {
            user_agent: "Mozilla/5.0 (X11; Linux x86_64; rv:150.0) Gecko/20100101 Firefox/150.0",
            platform: "Linux x86_64",
            languages: EN_US_EN,
            accept: FIREFOX_NAVIGATION_ACCEPT,
            accept_language: FIREFOX_ACCEPT_LANGUAGE,
            accept_encoding: DEFAULT_ACCEPT_ENCODING,
            mobile: false,
            screen_width: 1920,
            screen_height: 1080,
        },
        StealthProfile::FirefoxWindows => ProfileFacts {
            user_agent: FIREFOX_WINDOWS_STABLE_USER_AGENT,
            platform: "Win32",
            languages: EN_US_EN,
            accept: FIREFOX_NAVIGATION_ACCEPT,
            accept_language: FIREFOX_ACCEPT_LANGUAGE,
            accept_encoding: DEFAULT_ACCEPT_ENCODING,
            mobile: false,
            screen_width: 1920,
            screen_height: 1080,
        },
        StealthProfile::FirefoxMacStable => ProfileFacts {
            user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:150.0) Gecko/20100101 Firefox/150.0",
            platform: "MacIntel",
            languages: EN_US_EN,
            accept: FIREFOX_NAVIGATION_ACCEPT,
            accept_language: FIREFOX_ACCEPT_LANGUAGE,
            accept_encoding: DEFAULT_ACCEPT_ENCODING,
            mobile: false,
            screen_width: 1728,
            screen_height: 1117,
        },
        StealthProfile::ChromeAndroid => ProfileFacts {
            user_agent: "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 \
                         (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36",
            platform: "Linux armv8l",
            languages: EN_US_EN,
            accept: CHROMIUM_NAVIGATION_ACCEPT,
            accept_language: DEFAULT_ACCEPT_LANGUAGE,
            accept_encoding: DEFAULT_ACCEPT_ENCODING,
            mobile: true,
            screen_width: 412,
            screen_height: 915,
        },
        StealthProfile::SafariIphone => ProfileFacts {
            user_agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) \
                         AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 \
                         Mobile/15E148 Safari/604.1",
            platform: "iPhone",
            languages: EN_US_EN,
            accept: SAFARI_NAVIGATION_ACCEPT,
            accept_language: DEFAULT_ACCEPT_LANGUAGE,
            accept_encoding: DEFAULT_ACCEPT_ENCODING,
            mobile: true,
            screen_width: 390,
            screen_height: 844,
        },
        StealthProfile::SafariIpad => ProfileFacts {
            user_agent: "Mozilla/5.0 (iPad; CPU OS 17_5 like Mac OS X) AppleWebKit/605.1.15 \
                         (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1",
            platform: "iPad",
            languages: EN_US_EN,
            accept: SAFARI_NAVIGATION_ACCEPT,
            accept_language: DEFAULT_ACCEPT_LANGUAGE,
            accept_encoding: DEFAULT_ACCEPT_ENCODING,
            mobile: true,
            screen_width: 1024,
            screen_height: 1366,
        },
        StealthProfile::SafariMacStable => ProfileFacts {
            user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
                         AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15",
            platform: "MacIntel",
            languages: EN_US_EN,
            accept: SAFARI_NAVIGATION_ACCEPT,
            accept_language: DEFAULT_ACCEPT_LANGUAGE,
            accept_encoding: DEFAULT_ACCEPT_ENCODING,
            mobile: false,
            screen_width: 1728,
            screen_height: 1117,
        },
        StealthProfile::ChromeLinux => ProfileFacts {
            user_agent: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
                         (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
            platform: "Linux x86_64",
            languages: EN_US_EN,
            accept: CHROMIUM_NAVIGATION_ACCEPT,
            accept_language: DEFAULT_ACCEPT_LANGUAGE,
            accept_encoding: DEFAULT_ACCEPT_ENCODING,
            mobile: false,
            screen_width: 1920,
            screen_height: 1080,
        },
        StealthProfile::BraveWindows => ProfileFacts {
            user_agent: CHROME_WINDOWS_STABLE_USER_AGENT,
            platform: "Win32",
            languages: EN_US_EN,
            accept: CHROMIUM_NAVIGATION_ACCEPT,
            accept_language: DEFAULT_ACCEPT_LANGUAGE,
            accept_encoding: DEFAULT_ACCEPT_ENCODING,
            mobile: false,
            screen_width: 1920,
            screen_height: 1080,
        },
        StealthProfile::OperaWindows => ProfileFacts {
            user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                         (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 OPR/116.0.0.0",
            platform: "Win32",
            languages: EN_US_EN,
            accept: CHROMIUM_NAVIGATION_ACCEPT,
            accept_language: DEFAULT_ACCEPT_LANGUAGE,
            accept_encoding: DEFAULT_ACCEPT_ENCODING,
            mobile: false,
            screen_width: 1920,
            screen_height: 1080,
        },
        StealthProfile::SamsungInternetAndroid => ProfileFacts {
            user_agent: "Mozilla/5.0 (Linux; Android 14; SM-S928B) AppleWebKit/537.36 \
                         (KHTML, like Gecko) SamsungBrowser/26.0 Chrome/126.0.0.0 \
                         Mobile Safari/537.36",
            platform: "Linux armv8l",
            languages: EN_US_EN,
            accept: CHROMIUM_NAVIGATION_ACCEPT,
            accept_language: DEFAULT_ACCEPT_LANGUAGE,
            accept_encoding: DEFAULT_ACCEPT_ENCODING,
            mobile: true,
            screen_width: 412,
            screen_height: 915,
        },
    }
}

/// Canonical identity facts for [`DEFAULT_STEALTH_PROFILE`].
#[must_use]
pub const fn default_profile_facts() -> ProfileFacts {
    profile_facts(DEFAULT_STEALTH_PROFILE)
}

/// Canonical User-Agent for a stealth profile.
#[must_use]
pub const fn profile_user_agent(profile: StealthProfile) -> &'static str {
    profile_facts(profile).user_agent
}

/// Canonical User-Agent for [`DEFAULT_STEALTH_PROFILE`].
#[must_use]
pub const fn default_profile_user_agent() -> &'static str {
    default_profile_facts().user_agent
}

/// Canonical profile-backed headers for top-level browser-like HTTP navigation.
///
/// The returned names are lower-case so crates using the `http`/`reqwest`
/// header types can install them with `HeaderName::from_static` without
/// allocation or fallible parsing. `Accept-Encoding` is intentionally left to
/// the transport so compression negotiation and automatic decompression remain
/// controlled by the HTTP stack.
#[must_use]
pub const fn profile_navigation_headers(profile: StealthProfile) -> [NavigationHeader; 3] {
    let facts = profile_facts(profile);
    [
        NavigationHeader {
            name: USER_AGENT_HEADER,
            value: facts.user_agent,
        },
        NavigationHeader {
            name: ACCEPT_HEADER,
            value: facts.accept,
        },
        NavigationHeader {
            name: ACCEPT_LANGUAGE_HEADER,
            value: facts.accept_language,
        },
    ]
}

/// Canonical profile-backed navigation headers for [`DEFAULT_STEALTH_PROFILE`].
#[must_use]
pub const fn default_profile_navigation_headers() -> [NavigationHeader; 3] {
    profile_navigation_headers(DEFAULT_STEALTH_PROFILE)
}

/// Canonical browser HTTP headers including compression negotiation.
///
/// Scanner transports that own response decompression can use this complete
/// browser-shaped set. Transports that need to avoid advertising compressed
/// bodies should use [`profile_navigation_headers`] instead.
#[must_use]
pub const fn profile_browser_headers(profile: StealthProfile) -> [NavigationHeader; 4] {
    let facts = profile_facts(profile);
    [
        NavigationHeader {
            name: USER_AGENT_HEADER,
            value: facts.user_agent,
        },
        NavigationHeader {
            name: ACCEPT_HEADER,
            value: facts.accept,
        },
        NavigationHeader {
            name: ACCEPT_LANGUAGE_HEADER,
            value: facts.accept_language,
        },
        NavigationHeader {
            name: ACCEPT_ENCODING_HEADER,
            value: facts.accept_encoding,
        },
    ]
}

/// Canonical browser HTTP headers for [`DEFAULT_STEALTH_PROFILE`].
#[must_use]
pub const fn default_profile_browser_headers() -> [NavigationHeader; 4] {
    profile_browser_headers(DEFAULT_STEALTH_PROFILE)
}

/// Canonical browser request headers for a request surface, including
/// compression negotiation.
///
/// # Header order is convenience-shaped, not wire-authentic (G028/G045)
///
/// The returned sequence is a single **Firefox-shaped** canonical order
/// (`User-Agent, Accept, Accept-Language, [Accept-Encoding,]
/// Upgrade-Insecure-Requests, Sec-Fetch-Dest, -Mode, -Site, -User`) for *every*
/// profile, only the header *values* vary by `profile`, not the order. Real
/// Chrome orders these differently (e.g. `Upgrade-Insecure-Requests` before
/// `User-Agent`, Sec-Fetch `Site, Mode, User, Dest`, `Accept-Encoding` /
/// `-Language` last), so do **not** treat this as a per-browser wire
/// fingerprint.
///
/// This is safe because nothing emits this slice as the literal on-wire order:
/// the `reqwest` integration converts it to an [`http::HeaderMap`], whose
/// iteration order hyper controls (insertion order is lost), and the
/// browser-authentic per-engine header order is owned by the
/// `scanclient::tls_impersonate` (`ImpersonateProfile`) lane for non-browser
/// traffic and by reynard's NSS-native stack for real browser traffic. Treat
/// this builder as the value-coherent header *set*; get wire-authentic *order*
/// from those lanes.
#[must_use]
pub const fn profile_request_headers(
    profile: StealthProfile,
    kind: BrowserRequestKind,
) -> BrowserRequestHeaders {
    profile_request_headers_inner(profile, kind, true)
}

/// Canonical request-surface headers for [`DEFAULT_STEALTH_PROFILE`].
#[must_use]
pub const fn default_profile_request_headers(kind: BrowserRequestKind) -> BrowserRequestHeaders {
    profile_request_headers(DEFAULT_STEALTH_PROFILE, kind)
}

/// Canonical browser request headers for a request surface without
/// `Accept-Encoding`.
///
/// Use this for transports that do not own transparent response
/// decompression but still need the request's browser fetch metadata.
#[must_use]
pub const fn profile_request_headers_without_compression(
    profile: StealthProfile,
    kind: BrowserRequestKind,
) -> BrowserRequestHeaders {
    profile_request_headers_inner(profile, kind, false)
}

/// Canonical request-surface headers without `Accept-Encoding` for [`DEFAULT_STEALTH_PROFILE`].
#[must_use]
pub const fn default_profile_request_headers_without_compression(
    kind: BrowserRequestKind,
) -> BrowserRequestHeaders {
    profile_request_headers_without_compression(DEFAULT_STEALTH_PROFILE, kind)
}

const fn profile_request_headers_inner(
    profile: StealthProfile,
    kind: BrowserRequestKind,
    include_compression: bool,
) -> BrowserRequestHeaders {
    let facts = profile_facts(profile);
    let surface = request_surface_facts(profile, kind, facts.accept);

    if surface.upgrade_insecure_requests {
        if include_compression {
            BrowserRequestHeaders {
                entries: [
                    NavigationHeader {
                        name: USER_AGENT_HEADER,
                        value: facts.user_agent,
                    },
                    NavigationHeader {
                        name: ACCEPT_HEADER,
                        value: surface.accept,
                    },
                    NavigationHeader {
                        name: ACCEPT_LANGUAGE_HEADER,
                        value: facts.accept_language,
                    },
                    NavigationHeader {
                        name: ACCEPT_ENCODING_HEADER,
                        value: facts.accept_encoding,
                    },
                    NavigationHeader {
                        name: UPGRADE_INSECURE_REQUESTS_HEADER,
                        value: UPGRADE_INSECURE_REQUESTS_VALUE,
                    },
                    NavigationHeader {
                        name: SEC_FETCH_DEST_HEADER,
                        value: surface.dest,
                    },
                    NavigationHeader {
                        name: SEC_FETCH_MODE_HEADER,
                        value: surface.mode,
                    },
                    NavigationHeader {
                        name: SEC_FETCH_SITE_HEADER,
                        value: surface.site,
                    },
                    NavigationHeader {
                        name: SEC_FETCH_USER_HEADER,
                        value: surface.fetch_user,
                    },
                ],
                len: 9,
            }
        } else {
            BrowserRequestHeaders {
                entries: [
                    NavigationHeader {
                        name: USER_AGENT_HEADER,
                        value: facts.user_agent,
                    },
                    NavigationHeader {
                        name: ACCEPT_HEADER,
                        value: surface.accept,
                    },
                    NavigationHeader {
                        name: ACCEPT_LANGUAGE_HEADER,
                        value: facts.accept_language,
                    },
                    NavigationHeader {
                        name: UPGRADE_INSECURE_REQUESTS_HEADER,
                        value: UPGRADE_INSECURE_REQUESTS_VALUE,
                    },
                    NavigationHeader {
                        name: SEC_FETCH_DEST_HEADER,
                        value: surface.dest,
                    },
                    NavigationHeader {
                        name: SEC_FETCH_MODE_HEADER,
                        value: surface.mode,
                    },
                    NavigationHeader {
                        name: SEC_FETCH_SITE_HEADER,
                        value: surface.site,
                    },
                    NavigationHeader {
                        name: SEC_FETCH_USER_HEADER,
                        value: surface.fetch_user,
                    },
                    EMPTY_HEADER,
                ],
                len: 8,
            }
        }
    } else if include_compression {
        BrowserRequestHeaders {
            entries: [
                NavigationHeader {
                    name: USER_AGENT_HEADER,
                    value: facts.user_agent,
                },
                NavigationHeader {
                    name: ACCEPT_HEADER,
                    value: surface.accept,
                },
                NavigationHeader {
                    name: ACCEPT_LANGUAGE_HEADER,
                    value: facts.accept_language,
                },
                NavigationHeader {
                    name: ACCEPT_ENCODING_HEADER,
                    value: facts.accept_encoding,
                },
                NavigationHeader {
                    name: SEC_FETCH_DEST_HEADER,
                    value: surface.dest,
                },
                NavigationHeader {
                    name: SEC_FETCH_MODE_HEADER,
                    value: surface.mode,
                },
                NavigationHeader {
                    name: SEC_FETCH_SITE_HEADER,
                    value: surface.site,
                },
                EMPTY_HEADER,
                EMPTY_HEADER,
            ],
            len: 7,
        }
    } else {
        BrowserRequestHeaders {
            entries: [
                NavigationHeader {
                    name: USER_AGENT_HEADER,
                    value: facts.user_agent,
                },
                NavigationHeader {
                    name: ACCEPT_HEADER,
                    value: surface.accept,
                },
                NavigationHeader {
                    name: ACCEPT_LANGUAGE_HEADER,
                    value: facts.accept_language,
                },
                NavigationHeader {
                    name: SEC_FETCH_DEST_HEADER,
                    value: surface.dest,
                },
                NavigationHeader {
                    name: SEC_FETCH_MODE_HEADER,
                    value: surface.mode,
                },
                NavigationHeader {
                    name: SEC_FETCH_SITE_HEADER,
                    value: surface.site,
                },
                EMPTY_HEADER,
                EMPTY_HEADER,
                EMPTY_HEADER,
            ],
            len: 6,
        }
    }
}

struct RequestSurfaceFacts {
    accept: &'static str,
    dest: &'static str,
    mode: &'static str,
    site: &'static str,
    upgrade_insecure_requests: bool,
    fetch_user: &'static str,
}

/// The persona-family `<img>`-element Accept header, keyed by browser family.
///
/// A real browser requests an image element with a resource-specific Accept, NOT
/// the bare `*/*` a generic client sends, so an image subresource that carries
/// `*/*` is a fetch-metadata tell. Chromium and Firefox values are wire-verified
/// constants; Safari/IE fall back to `*/*` (a documented residual gap, their
/// exact modern image Accept is not pinned here, and fabricating one would risk a
/// *unique* tell, worse than the generic one).
const fn profile_image_accept(profile: StealthProfile) -> &'static str {
    match profile {
        StealthProfile::FirefoxLinux
        | StealthProfile::FirefoxWindows
        | StealthProfile::FirefoxMacStable => FIREFOX_IMAGE_ACCEPT,
        StealthProfile::ChromeWindowsStable
        | StealthProfile::ChromeWindowsLegacy96
        | StealthProfile::ChromeMacStable
        | StealthProfile::ChromeAndroid
        | StealthProfile::ChromeLinux
        | StealthProfile::EdgeWindowsStable
        | StealthProfile::BraveWindows
        | StealthProfile::OperaWindows
        | StealthProfile::SamsungInternetAndroid => CHROMIUM_IMAGE_ACCEPT,
        StealthProfile::Ie11Windows
        | StealthProfile::SafariIphone
        | StealthProfile::SafariIpad
        | StealthProfile::SafariMacStable => WILDCARD_ACCEPT,
    }
}

const fn request_surface_facts(
    profile: StealthProfile,
    kind: BrowserRequestKind,
    navigation_accept: &'static str,
) -> RequestSurfaceFacts {
    match kind {
        BrowserRequestKind::Navigation => RequestSurfaceFacts {
            accept: navigation_accept,
            dest: DOCUMENT_DEST_VALUE,
            mode: NAVIGATE_MODE_VALUE,
            site: NONE_SITE_VALUE,
            upgrade_insecure_requests: true,
            fetch_user: FETCH_USER_ACTIVATED_VALUE,
        },
        BrowserRequestKind::SameOriginNavigation => RequestSurfaceFacts {
            accept: navigation_accept,
            dest: DOCUMENT_DEST_VALUE,
            mode: NAVIGATE_MODE_VALUE,
            site: SAME_ORIGIN_SITE_VALUE,
            upgrade_insecure_requests: true,
            fetch_user: FETCH_USER_ACTIVATED_VALUE,
        },
        BrowserRequestKind::CrossSiteNavigation => RequestSurfaceFacts {
            accept: navigation_accept,
            dest: DOCUMENT_DEST_VALUE,
            mode: NAVIGATE_MODE_VALUE,
            site: CROSS_SITE_VALUE,
            upgrade_insecure_requests: true,
            fetch_user: FETCH_USER_ACTIVATED_VALUE,
        },
        BrowserRequestKind::SameOriginFetch => RequestSurfaceFacts {
            accept: WILDCARD_ACCEPT,
            dest: EMPTY_DEST_VALUE,
            mode: CORS_MODE_VALUE,
            site: SAME_ORIGIN_SITE_VALUE,
            upgrade_insecure_requests: false,
            fetch_user: "",
        },
        BrowserRequestKind::SameOriginModeFetch => RequestSurfaceFacts {
            accept: WILDCARD_ACCEPT,
            dest: EMPTY_DEST_VALUE,
            mode: SAME_ORIGIN_MODE_VALUE,
            site: SAME_ORIGIN_SITE_VALUE,
            upgrade_insecure_requests: false,
            fetch_user: "",
        },
        BrowserRequestKind::CrossSiteFetch => RequestSurfaceFacts {
            accept: WILDCARD_ACCEPT,
            dest: EMPTY_DEST_VALUE,
            mode: CORS_MODE_VALUE,
            site: CROSS_SITE_VALUE,
            upgrade_insecure_requests: false,
            fetch_user: "",
        },
        BrowserRequestKind::ImageSubresource => RequestSurfaceFacts {
            // Real `<img>` loads carry a family-specific image Accept, not `*/*`.
            accept: profile_image_accept(profile),
            dest: IMAGE_DEST_VALUE,
            mode: NO_CORS_MODE_VALUE,
            site: CROSS_SITE_VALUE,
            upgrade_insecure_requests: false,
            fetch_user: "",
        },
        BrowserRequestKind::AudioSubresource => RequestSurfaceFacts {
            accept: WILDCARD_ACCEPT,
            dest: AUDIO_DEST_VALUE,
            mode: NO_CORS_MODE_VALUE,
            site: CROSS_SITE_VALUE,
            upgrade_insecure_requests: false,
            fetch_user: "",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_windows_const_matches_profile() {
        assert_eq!(
            profile_user_agent(StealthProfile::ChromeWindowsStable),
            CHROME_WINDOWS_STABLE_USER_AGENT
        );
    }

    #[test]
    fn firefox_windows_const_matches_profile() {
        assert_eq!(
            profile_user_agent(StealthProfile::FirefoxWindows),
            FIREFOX_WINDOWS_STABLE_USER_AGENT
        );
    }

    #[test]
    fn firefox_windows_stays_windows_and_firefox() {
        let ua = profile_user_agent(StealthProfile::FirefoxWindows);
        assert!(ua.contains("Windows NT"));
        assert!(ua.contains("Firefox/150.0"));
        assert!(!ua.contains("Chrome/"));
    }

    #[test]
    fn firefox_mac_stays_macos_and_firefox() {
        let ua = profile_user_agent(StealthProfile::FirefoxMacStable);
        assert!(ua.contains("Macintosh; Intel Mac OS X"));
        assert!(ua.contains("Firefox/150.0"));
        assert!(!ua.contains("Chrome/"));
    }

    #[test]
    fn profile_facts_match_user_agent_api() {
        for profile in ALL_PROFILES {
            assert_eq!(
                profile_facts(*profile).user_agent,
                profile_user_agent(*profile)
            );
            assert_eq!(profile_facts(*profile).languages[0], "en-US");
            assert!(!profile_facts(*profile).accept.is_empty());
            assert!(!profile_facts(*profile).accept_language.is_empty());
            let expected_encoding = match profile {
                &StealthProfile::Ie11Windows => LEGACY_ACCEPT_ENCODING,
                _ => DEFAULT_ACCEPT_ENCODING,
            };
            assert_eq!(profile_facts(*profile).accept_encoding, expected_encoding);
        }
    }

    /// Proving test for the BACKLOG robustness/panic row: every
    /// `StealthProfile` variant has a non-empty hardware table, so
    /// `profile_hardware` (`[0]`) and `profile_hardware_at` (`% len`) succeed
    /// for the whole catalogue. The compile-time `const _` assertions beside
    /// the tables make an empty table a build error; this test proves the
    /// runtime accessors agree over every variant.
    #[test]
    fn every_profile_has_usable_hardware_accessors() {
        for profile in ALL_PROFILES {
            assert!(
                !profile_hardware_variants(*profile).is_empty(),
                "{profile:?} has an empty hardware table"
            );
            let default = profile_hardware(*profile);
            let first = profile_hardware_at(*profile, 0);
            assert_eq!(default, first, "{profile:?} index 0 must equal default");
            // One full turn of the modulo wheel returns to the default.
            let len = profile_hardware_variants(*profile).len();
            let wrapped = profile_hardware_at(*profile, len);
            assert_eq!(
                wrapped, default,
                "{profile:?} modulo wrap must equal default"
            );
        }
    }

    #[test]
    fn every_persona_ua_os_matches_its_tcp_network_os() {
        // X007, the cross-layer OS-coherence gate. A persona's UA-advertised OS
        // (Windows / Linux / macOS / iOS / Android) MUST equal the OS its TCP/IP
        // stack emulates (initial TTL + window + option layout), or a server that
        // correlates the User-Agent against a passive p0f SYN signature sees a
        // "Windows" browser speaking a Linux kernel's stack (an instant tell).
        // Pinned across ALL_PROFILES so a newly-added persona can never ship a
        // UA-OS-vs-TCP-OS split; fails loud, naming the persona and both OSes.
        for profile in ALL_PROFILES {
            let ua = profile_user_agent(*profile);
            let ua_os = user_agent_facts(ua).platform;
            let stack = profile_os_network_stack(*profile);
            assert_eq!(
                ua_os, stack.os,
                "{profile:?}: UA-OS {ua_os:?} (from {ua:?}) != TCP-stack OS {:?} (initial_ttl {})",
                stack.os, stack.initial_ttl
            );
            // Round-trip: the persona's own canonical initial TTL must read back
            // Coherent against itself (the stack is internally self-consistent).
            assert!(
                os_network_coherence(*profile, stack.initial_ttl).is_coherent(),
                "{profile:?}: own initial_ttl {} is not self-coherent",
                stack.initial_ttl
            );
        }
    }

    #[test]
    fn every_persona_accept_language_is_coherent_with_its_language_list() {
        // X020 (locale arm), a persona's `Accept-Language` HEADER and its
        // `navigator.languages` array are stored as SEPARATE fields; they MUST
        // describe one locale, or a server sees `Accept-Language: en-US` from a
        // browser whose JS reports `fr-FR` (a header-vs-JS locale split that a
        // cross-checking anti-bot flags). Pinned across ALL_PROFILES: the header's
        // primary tag is languages[0], and every declared language appears in it.
        for profile in ALL_PROFILES {
            let f = profile_facts(*profile);
            let primary = f.languages[0];
            assert!(
                f.accept_language.starts_with(primary),
                "{profile:?}: Accept-Language {:?} must start with primary navigator.language {primary:?}",
                f.accept_language
            );
            for lang in f.languages {
                assert!(
                    f.accept_language.contains(*lang),
                    "{profile:?}: navigator.languages has {lang:?} but Accept-Language {:?} omits it",
                    f.accept_language
                );
            }
        }
    }

    #[test]
    fn default_profile_accessors_delegate_to_default_profile() {
        assert_eq!(
            default_profile_facts(),
            profile_facts(DEFAULT_STEALTH_PROFILE)
        );
        assert_eq!(
            default_profile_user_agent(),
            profile_user_agent(DEFAULT_STEALTH_PROFILE)
        );
        assert_eq!(
            default_profile_navigation_headers(),
            profile_navigation_headers(DEFAULT_STEALTH_PROFILE)
        );
        assert_eq!(
            default_profile_browser_headers(),
            profile_browser_headers(DEFAULT_STEALTH_PROFILE)
        );
        assert_eq!(
            default_profile_request_headers(BrowserRequestKind::Navigation),
            profile_request_headers(DEFAULT_STEALTH_PROFILE, BrowserRequestKind::Navigation)
        );
        assert_eq!(
            default_profile_request_headers_without_compression(BrowserRequestKind::Navigation),
            profile_request_headers_without_compression(
                DEFAULT_STEALTH_PROFILE,
                BrowserRequestKind::Navigation
            )
        );
    }

    #[test]
    fn profile_names_and_aliases_are_canonical() {
        for profile in ALL_PROFILES {
            assert_eq!(named_profile(profile_name(*profile)), Some(*profile));
            assert_eq!(
                named_profile(profile_display_name(*profile)),
                Some(*profile)
            );
        }

        assert_eq!(
            named_profile("chrome-win"),
            Some(StealthProfile::ChromeWindowsStable)
        );
        assert_eq!(
            named_profile("chrome-osx"),
            Some(StealthProfile::ChromeMacStable)
        );
        assert_eq!(named_profile("ie11"), Some(StealthProfile::Ie11Windows));
        assert_eq!(
            named_profile("firefox-mac"),
            Some(StealthProfile::FirefoxMacStable)
        );
        assert_eq!(
            named_profile("firefox-macos"),
            Some(StealthProfile::FirefoxMacStable)
        );
        assert_eq!(named_profile("unknown"), None);
    }

    #[test]
    fn user_agent_facts_parse_chrome_windows() {
        let facts = user_agent_facts(profile_user_agent(StealthProfile::ChromeWindowsStable));

        assert_eq!(facts.browser, UserAgentBrowser::Chrome);
        assert_eq!(facts.platform, UserAgentPlatform::Windows);
        assert_eq!(facts.browser_major_version, Some(131));
        assert_eq!(facts.chromium_major_version, Some(131));
        assert_eq!(
            facts.inferred_profile,
            Some(StealthProfile::ChromeWindowsStable)
        );
        assert_eq!(facts.client_hint_platform_value(), Some("\"Windows\""));
        assert_eq!(facts.client_hint_mobile_value(), "?0");
        assert_eq!(facts.platform.chrome_tls_label(), Some("Windows"));
    }

    #[test]
    fn user_agent_facts_parse_mobile_and_safari_profiles() {
        let android = user_agent_facts(profile_user_agent(StealthProfile::ChromeAndroid));
        assert_eq!(android.platform, UserAgentPlatform::Android);
        assert_eq!(
            android.inferred_profile,
            Some(StealthProfile::ChromeAndroid)
        );
        assert_eq!(android.client_hint_mobile_value(), "?1");
        assert_eq!(android.platform.chrome_tls_label(), Some("Android"));

        let iphone = user_agent_facts(profile_user_agent(StealthProfile::SafariIphone));
        assert_eq!(iphone.browser, UserAgentBrowser::Safari);
        assert_eq!(iphone.platform, UserAgentPlatform::Ios);
        assert_eq!(iphone.inferred_profile, Some(StealthProfile::SafariIphone));
        assert_eq!(iphone.platform.chrome_tls_label(), None);
    }

    #[test]
    fn user_agent_facts_parse_chromium_vendor_profiles() {
        let edge = user_agent_facts(profile_user_agent(StealthProfile::EdgeWindowsStable));
        assert_eq!(edge.browser, UserAgentBrowser::Edge);
        assert_eq!(edge.browser_major_version, Some(131));
        assert_eq!(edge.chromium_major_version, Some(131));
        assert_eq!(
            edge.inferred_profile,
            Some(StealthProfile::EdgeWindowsStable)
        );

        let opera = user_agent_facts(profile_user_agent(StealthProfile::OperaWindows));
        assert_eq!(opera.browser, UserAgentBrowser::Opera);
        assert_eq!(opera.browser_major_version, Some(116));
        assert_eq!(opera.chromium_major_version, Some(131));
        assert_eq!(opera.inferred_profile, Some(StealthProfile::OperaWindows));

        let samsung = user_agent_facts(profile_user_agent(StealthProfile::SamsungInternetAndroid));
        assert_eq!(samsung.browser, UserAgentBrowser::SamsungInternet);
        assert_eq!(samsung.browser_major_version, Some(26));
        assert_eq!(samsung.chromium_major_version, Some(126));
        assert_eq!(
            samsung.inferred_profile,
            Some(StealthProfile::SamsungInternetAndroid)
        );
    }

    #[test]
    fn user_agent_facts_parse_firefox_ie_and_legacy_chrome() {
        let firefox = user_agent_facts(profile_user_agent(StealthProfile::FirefoxWindows));
        assert_eq!(firefox.browser, UserAgentBrowser::Firefox);
        assert_eq!(firefox.platform, UserAgentPlatform::Windows);
        assert_eq!(firefox.browser_major_version, Some(150));
        assert_eq!(firefox.chromium_major_version, None);
        assert_eq!(
            firefox.inferred_profile,
            Some(StealthProfile::FirefoxWindows)
        );

        let firefox_mac = user_agent_facts(profile_user_agent(StealthProfile::FirefoxMacStable));
        assert_eq!(firefox_mac.browser, UserAgentBrowser::Firefox);
        assert_eq!(firefox_mac.platform, UserAgentPlatform::MacOs);
        assert_eq!(firefox_mac.browser_major_version, Some(150));
        assert_eq!(firefox_mac.chromium_major_version, None);
        assert_eq!(
            firefox_mac.inferred_profile,
            Some(StealthProfile::FirefoxMacStable)
        );

        let ie = user_agent_facts(profile_user_agent(StealthProfile::Ie11Windows));
        assert_eq!(ie.browser, UserAgentBrowser::InternetExplorer);
        assert_eq!(ie.platform, UserAgentPlatform::Windows);
        assert_eq!(ie.browser_major_version, Some(11));
        assert_eq!(ie.inferred_profile, Some(StealthProfile::Ie11Windows));

        let legacy = user_agent_facts(profile_user_agent(StealthProfile::ChromeWindowsLegacy96));
        assert_eq!(legacy.browser_major_version, Some(96));
        assert_eq!(
            legacy.inferred_profile,
            Some(StealthProfile::ChromeWindowsLegacy96)
        );
    }

    #[test]
    fn user_agent_facts_reject_unknown_and_flags_headless() {
        let unknown = user_agent_facts("curl/8.0");
        assert_eq!(unknown.browser, UserAgentBrowser::Unknown);
        assert_eq!(unknown.platform, UserAgentPlatform::Unknown);
        assert_eq!(unknown.inferred_profile, None);
        assert_eq!(unknown.browser_major_version, None);

        let headless = user_agent_facts(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
             (KHTML, like Gecko) HeadlessChrome/134.0.0.0 Safari/537.36",
        );
        assert!(headless.headless);
        assert_eq!(headless.browser, UserAgentBrowser::Chrome);
        assert_eq!(headless.chromium_major_version, Some(134));
        assert_eq!(headless.inferred_profile, Some(StealthProfile::ChromeLinux));
    }

    #[test]
    fn user_agent_facts_no_silent_fallback_on_unsupported_platforms() {
        // Firefox on Android must NOT silently fall back to FirefoxLinux
        let ff_android = user_agent_facts(
            "Mozilla/5.0 (Android 14; Mobile; rv:126.0) Gecko/126.0 Firefox/126.0",
        );
        assert_eq!(ff_android.browser, UserAgentBrowser::Firefox);
        assert_eq!(ff_android.platform, UserAgentPlatform::Android);
        assert_eq!(ff_android.inferred_profile, None);

        // Firefox on iOS (FxiOS) must NOT silently fall back to FirefoxLinux
        let ff_ios = user_agent_facts(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) FxiOS/126.0 Mobile/15E148 Safari/605.1.15",
        );
        assert_eq!(ff_ios.browser, UserAgentBrowser::Firefox);
        assert_eq!(ff_ios.platform, UserAgentPlatform::Ios);
        assert_eq!(ff_ios.browser_major_version, Some(126));
        assert_eq!(ff_ios.inferred_profile, None);

        // Safari on Windows must NOT silently fall back to SafariMacStable
        let safari_win = user_agent_facts(
            "Mozilla/5.0 (Windows NT 6.1; WOW64) AppleWebKit/534.57.2 (KHTML, like Gecko) Version/5.1.7 Safari/534.57.2",
        );
        assert_eq!(safari_win.browser, UserAgentBrowser::Safari);
        assert_eq!(safari_win.platform, UserAgentPlatform::Windows);
        assert_eq!(safari_win.inferred_profile, None);

        // Edge on Mac must NOT silently fall back to EdgeWindowsStable
        let edge_mac = user_agent_facts(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36 Edg/120.0.0.0",
        );
        assert_eq!(edge_mac.browser, UserAgentBrowser::Edge);
        assert_eq!(edge_mac.platform, UserAgentPlatform::MacOs);
        assert_eq!(edge_mac.inferred_profile, None);

        // Opera on Android must NOT silently fall back to OperaWindows
        let opera_android = user_agent_facts(
            "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36 OPR/80.0.0.0",
        );
        assert_eq!(opera_android.browser, UserAgentBrowser::Opera);
        assert_eq!(opera_android.platform, UserAgentPlatform::Android);
        assert_eq!(opera_android.inferred_profile, None);

        // iPod touch with CPU iPhone OS must be classified as Ios, not MacOs
        let ipod = user_agent_facts(
            "Mozilla/5.0 (iPod touch; CPU iPhone OS 14_4 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0.3 Mobile/15E148 Safari/604.1",
        );
        assert_eq!(ipod.browser, UserAgentBrowser::Safari);
        assert_eq!(ipod.platform, UserAgentPlatform::Ios);
        assert_eq!(ipod.inferred_profile, Some(StealthProfile::SafariIphone));
    }

    #[test]
    fn rotation_profiles_exclude_legacy_personas() {
        assert!(!ROTATION_PROFILES.contains(&StealthProfile::ChromeWindowsLegacy96));
        assert!(!ROTATION_PROFILES.contains(&StealthProfile::Ie11Windows));
        assert!(ROTATION_PROFILES.contains(&StealthProfile::ChromeWindowsStable));
    }

    #[test]
    fn default_stealth_profile_is_catalogued_and_rotatable() {
        assert!(ALL_PROFILES.contains(&DEFAULT_STEALTH_PROFILE));
        assert!(ROTATION_PROFILES.contains(&DEFAULT_STEALTH_PROFILE));
        assert_eq!(
            named_profile(profile_name(DEFAULT_STEALTH_PROFILE)),
            Some(DEFAULT_STEALTH_PROFILE)
        );
    }

    #[test]
    fn profile_hardware_defaults_share_profile_screen_facts() {
        for profile in ALL_PROFILES {
            let facts = profile_facts(*profile);
            let variants = profile_hardware_variants(*profile);
            let hardware = profile_hardware(*profile);

            assert!(
                !variants.is_empty(),
                "{profile:?} must expose at least one hardware tuple"
            );
            assert_eq!(hardware.screen_width, facts.screen_width);
            assert_eq!(hardware.screen_height, facts.screen_height);
            assert!(hardware.color_depth > 0);
            assert!(hardware.device_memory > 0);
            assert!(hardware.hardware_concurrency > 0);
            // WebGL vendor/renderer must be coherent: both empty (native
            // passthrough, expose the host's real Gecko-sanitized adapter) or
            // both set (a cross-OS persona pinning a coherent adapter). A
            // half-state is the exact incoherence the old override shipped.
            assert_eq!(
                hardware.webgl_vendor.is_empty(),
                hardware.webgl_renderer.is_empty(),
                "{profile:?} half-spoofed WebGL adapter (vendor/renderer emptiness disagree)"
            );
        }
    }

    #[test]
    fn browser_surface_metadata_tracks_profile_family() {
        for profile in ALL_PROFILES {
            let vendor = profile_navigator_vendor(*profile);
            let brands = profile_client_hint_brands(*profile);
            let client_hint_platform = profile_client_hint_platform(*profile);

            match profile {
                StealthProfile::FirefoxLinux
                | StealthProfile::FirefoxWindows
                | StealthProfile::FirefoxMacStable
                | StealthProfile::Ie11Windows => {
                    assert_eq!(vendor, "");
                    assert!(brands.is_empty(), "{profile:?} should not expose UA-CH");
                    assert_eq!(client_hint_platform, None);
                }
                StealthProfile::SafariIphone
                | StealthProfile::SafariIpad
                | StealthProfile::SafariMacStable => {
                    assert_eq!(vendor, "Apple Computer, Inc.");
                    assert!(brands.is_empty(), "{profile:?} should not expose UA-CH");
                    assert_eq!(client_hint_platform, None);
                }
                _ => {
                    assert_eq!(vendor, "Google Inc.");
                    assert!(!brands.is_empty(), "{profile:?} missing UA-CH brands");
                    assert!(
                        client_hint_platform.is_some(),
                        "{profile:?} missing Sec-CH-UA-Platform"
                    );
                    assert!(
                        brands
                            .iter()
                            .any(|brand| brand.brand == "Not?A_Brand" && brand.version == "99"),
                        "{profile:?} missing GREASE brand"
                    );
                }
            }
        }
    }

    #[test]
    fn ie11_profile_keeps_legacy_http_shape() {
        let facts = profile_facts(StealthProfile::Ie11Windows);

        assert_eq!(facts.user_agent, IE11_WINDOWS_USER_AGENT);
        assert!(facts.user_agent.contains("Trident/7.0"));
        assert!(!facts.user_agent.contains("Chrome/"));
        assert_eq!(facts.platform, "Win32");
        assert_eq!(facts.accept, IE11_NAVIGATION_ACCEPT);
        assert_eq!(facts.accept_encoding, LEGACY_ACCEPT_ENCODING);
        assert!(!facts.accept_encoding.contains("br"));
    }

    #[test]
    fn chrome_legacy_96_profile_keeps_legacy_chromium_shape() {
        let facts = profile_facts(StealthProfile::ChromeWindowsLegacy96);

        assert_eq!(facts.user_agent, CHROME_WINDOWS_LEGACY_96_USER_AGENT);
        assert!(facts.user_agent.contains("Chrome/96.0.4664.110"));
        assert_eq!(facts.platform, "Win32");
        assert_eq!(facts.accept, CHROMIUM_NAVIGATION_ACCEPT);
        assert_eq!(facts.accept_encoding, DEFAULT_ACCEPT_ENCODING);
        assert_eq!(facts.screen_width, 1366);
        assert_eq!(facts.screen_height, 768);
    }

    #[test]
    fn common_browser_profiles_delegate_to_profile_facts() {
        for (name, profile) in [
            ("chrome", StealthProfile::ChromeWindowsStable),
            ("firefox", StealthProfile::FirefoxLinux),
            ("safari", StealthProfile::SafariMacStable),
            ("edge", StealthProfile::EdgeWindowsStable),
        ] {
            let entry = get_profile(name).expect("profile should exist");
            let facts = profile_facts(profile);

            assert_eq!(entry.user_agent, facts.user_agent, "{name} UA drifted");
            assert_eq!(entry.accept, facts.accept, "{name} Accept drifted");
            assert_eq!(
                entry.accept_language, facts.accept_language,
                "{name} Accept-Language drifted"
            );
            assert_eq!(
                entry.accept_encoding, facts.accept_encoding,
                "{name} Accept-Encoding drifted"
            );
        }
    }

    #[test]
    fn common_browser_profile_rotation_matches_legacy_contract() {
        let first = rotate(0);
        assert_eq!(first.name, "chrome");
        assert_eq!(rotate(PROFILES.len()), first);
        assert_ne!(rotate(1).name, first.name);
        assert!(get_profile("unknown").is_none());
    }

    #[test]
    fn navigation_headers_delegate_to_profile_facts() {
        let facts = profile_facts(StealthProfile::ChromeWindowsStable);
        assert_eq!(
            profile_navigation_headers(StealthProfile::ChromeWindowsStable),
            [
                NavigationHeader {
                    name: USER_AGENT_HEADER,
                    value: facts.user_agent,
                },
                NavigationHeader {
                    name: ACCEPT_HEADER,
                    value: facts.accept,
                },
                NavigationHeader {
                    name: ACCEPT_LANGUAGE_HEADER,
                    value: facts.accept_language,
                },
            ]
        );
    }

    #[test]
    fn browser_profile_headers_include_legacy_accept_encoding() {
        let profile = rotate(0);
        assert_eq!(
            profile.headers(),
            [
                NavigationHeader {
                    name: USER_AGENT_HEADER,
                    value: profile.user_agent,
                },
                NavigationHeader {
                    name: ACCEPT_HEADER,
                    value: profile.accept,
                },
                NavigationHeader {
                    name: ACCEPT_LANGUAGE_HEADER,
                    value: profile.accept_language,
                },
                NavigationHeader {
                    name: ACCEPT_ENCODING_HEADER,
                    value: profile.accept_encoding,
                },
            ]
        );
    }

    #[test]
    fn browser_headers_delegate_to_profile_facts_with_accept_encoding() {
        for profile in ALL_PROFILES {
            let facts = profile_facts(*profile);
            assert_eq!(
                profile_browser_headers(*profile),
                [
                    NavigationHeader {
                        name: USER_AGENT_HEADER,
                        value: facts.user_agent,
                    },
                    NavigationHeader {
                        name: ACCEPT_HEADER,
                        value: facts.accept,
                    },
                    NavigationHeader {
                        name: ACCEPT_LANGUAGE_HEADER,
                        value: facts.accept_language,
                    },
                    NavigationHeader {
                        name: ACCEPT_ENCODING_HEADER,
                        value: facts.accept_encoding,
                    },
                ]
            );
        }
    }

    #[test]
    fn navigation_request_headers_include_fetch_metadata() {
        let facts = profile_facts(StealthProfile::ChromeWindowsStable);
        let headers = profile_request_headers_without_compression(
            StealthProfile::ChromeWindowsStable,
            BrowserRequestKind::Navigation,
        );

        assert_eq!(headers.len(), 8);
        assert_eq!(
            headers.as_slice(),
            &[
                NavigationHeader {
                    name: USER_AGENT_HEADER,
                    value: facts.user_agent,
                },
                NavigationHeader {
                    name: ACCEPT_HEADER,
                    value: facts.accept,
                },
                NavigationHeader {
                    name: ACCEPT_LANGUAGE_HEADER,
                    value: facts.accept_language,
                },
                NavigationHeader {
                    name: UPGRADE_INSECURE_REQUESTS_HEADER,
                    value: "1",
                },
                NavigationHeader {
                    name: SEC_FETCH_DEST_HEADER,
                    value: "document",
                },
                NavigationHeader {
                    name: SEC_FETCH_MODE_HEADER,
                    value: "navigate",
                },
                NavigationHeader {
                    name: SEC_FETCH_SITE_HEADER,
                    value: "none",
                },
                NavigationHeader {
                    name: SEC_FETCH_USER_HEADER,
                    value: "?1",
                },
            ]
        );
    }

    #[test]
    fn request_header_order_is_firefox_shaped_for_all_families_by_design() {
        // The builder emits ONE Firefox-shaped name order for every persona
        // only the values vary by family. This is deliberate (see
        // `profile_request_headers`): the slice is a value-coherent convenience
        // *set*, not a wire-authentic *order* (reqwest/hyper reorders the
        // HeaderMap; the on-wire per-engine order is owned by the wreq
        // ImpersonateProfile lane and reynard). Locking it stops a well-meaning
        // "give Chrome its real header order" edit that would diverge this
        // convenience API without changing a single wire byte.
        let order_of = |p| {
            profile_request_headers_without_compression(p, BrowserRequestKind::Navigation)
                .as_slice()
                .iter()
                .map(|h| h.name)
                .collect::<Vec<_>>()
        };
        let firefox_order = order_of(StealthProfile::FirefoxWindows);
        for p in [
            StealthProfile::ChromeWindowsStable,
            StealthProfile::SafariMacStable,
        ] {
            assert_eq!(
                order_of(p),
                firefox_order,
                "{p:?} request-header order drifted from the canonical Firefox-shaped order"
            );
        }
        assert_eq!(
            firefox_order,
            vec![
                USER_AGENT_HEADER,
                ACCEPT_HEADER,
                ACCEPT_LANGUAGE_HEADER,
                UPGRADE_INSECURE_REQUESTS_HEADER,
                SEC_FETCH_DEST_HEADER,
                SEC_FETCH_MODE_HEADER,
                SEC_FETCH_SITE_HEADER,
                SEC_FETCH_USER_HEADER,
            ],
            "canonical convenience order is no longer Firefox-shaped"
        );
    }

    #[test]
    fn same_origin_fetch_request_headers_are_not_navigation_shaped() {
        let facts = profile_facts(StealthProfile::ChromeWindowsStable);
        let headers = profile_request_headers_without_compression(
            StealthProfile::ChromeWindowsStable,
            BrowserRequestKind::SameOriginFetch,
        );

        assert_eq!(headers.len(), 6);
        assert_eq!(
            headers.as_slice(),
            &[
                NavigationHeader {
                    name: USER_AGENT_HEADER,
                    value: facts.user_agent,
                },
                NavigationHeader {
                    name: ACCEPT_HEADER,
                    value: "*/*",
                },
                NavigationHeader {
                    name: ACCEPT_LANGUAGE_HEADER,
                    value: facts.accept_language,
                },
                NavigationHeader {
                    name: SEC_FETCH_DEST_HEADER,
                    value: "empty",
                },
                NavigationHeader {
                    name: SEC_FETCH_MODE_HEADER,
                    value: "cors",
                },
                NavigationHeader {
                    name: SEC_FETCH_SITE_HEADER,
                    value: "same-origin",
                },
            ]
        );
    }

    #[test]
    fn same_origin_navigation_uses_document_surface_with_same_origin_site() {
        let facts = profile_facts(StealthProfile::ChromeWindowsStable);
        let headers = profile_request_headers_without_compression(
            StealthProfile::ChromeWindowsStable,
            BrowserRequestKind::SameOriginNavigation,
        );
        let by_name = |name: &str| {
            headers
                .as_slice()
                .iter()
                .find(|header| header.name == name)
                .map(|header| header.value)
        };

        assert_eq!(headers.len(), 8);
        assert_eq!(by_name(ACCEPT_HEADER), Some(facts.accept));
        assert_eq!(by_name(UPGRADE_INSECURE_REQUESTS_HEADER), Some("1"));
        assert_eq!(by_name(SEC_FETCH_DEST_HEADER), Some("document"));
        assert_eq!(by_name(SEC_FETCH_MODE_HEADER), Some("navigate"));
        assert_eq!(by_name(SEC_FETCH_SITE_HEADER), Some("same-origin"));
        assert_eq!(by_name(SEC_FETCH_USER_HEADER), Some("?1"));
    }

    #[test]
    fn cross_site_navigation_uses_document_surface_with_cross_site_site() {
        // The last untested BrowserRequestKind, a user-activated navigation that
        // arrives from another site (a clicked external link): document dest,
        // navigate mode, cross-site, still user-activated (?1) and UIR=1.
        let facts = profile_facts(StealthProfile::ChromeWindowsStable);
        let headers = profile_request_headers_without_compression(
            StealthProfile::ChromeWindowsStable,
            BrowserRequestKind::CrossSiteNavigation,
        );
        let by_name = |name: &str| {
            headers
                .as_slice()
                .iter()
                .find(|header| header.name == name)
                .map(|header| header.value)
        };

        assert_eq!(headers.len(), 8);
        assert_eq!(by_name(ACCEPT_HEADER), Some(facts.accept));
        assert_eq!(by_name(UPGRADE_INSECURE_REQUESTS_HEADER), Some("1"));
        assert_eq!(by_name(SEC_FETCH_DEST_HEADER), Some("document"));
        assert_eq!(by_name(SEC_FETCH_MODE_HEADER), Some("navigate"));
        assert_eq!(by_name(SEC_FETCH_SITE_HEADER), Some("cross-site"));
        assert_eq!(by_name(SEC_FETCH_USER_HEADER), Some("?1"));
    }

    #[test]
    fn cross_site_fetch_request_headers_are_cors_cross_site() {
        let headers = profile_request_headers_without_compression(
            StealthProfile::ChromeWindowsStable,
            BrowserRequestKind::CrossSiteFetch,
        );
        let by_name = |name: &str| {
            headers
                .as_slice()
                .iter()
                .find(|header| header.name == name)
                .map(|header| header.value)
        };

        assert_eq!(headers.len(), 6);
        assert_eq!(by_name(ACCEPT_HEADER), Some("*/*"));
        assert_eq!(by_name(SEC_FETCH_DEST_HEADER), Some("empty"));
        assert_eq!(by_name(SEC_FETCH_MODE_HEADER), Some("cors"));
        assert_eq!(by_name(SEC_FETCH_SITE_HEADER), Some("cross-site"));
        assert_eq!(by_name(UPGRADE_INSECURE_REQUESTS_HEADER), None);
        assert_eq!(by_name(SEC_FETCH_USER_HEADER), None);
    }

    #[test]
    fn same_origin_mode_fetch_request_headers_preserve_explicit_fetch_mode() {
        let headers = profile_request_headers_without_compression(
            StealthProfile::ChromeWindowsStable,
            BrowserRequestKind::SameOriginModeFetch,
        );
        let by_name = |name: &str| {
            headers
                .as_slice()
                .iter()
                .find(|header| header.name == name)
                .map(|header| header.value)
        };

        assert_eq!(headers.len(), 6);
        assert_eq!(by_name(ACCEPT_HEADER), Some("*/*"));
        assert_eq!(by_name(SEC_FETCH_DEST_HEADER), Some("empty"));
        assert_eq!(by_name(SEC_FETCH_MODE_HEADER), Some("same-origin"));
        assert_eq!(by_name(SEC_FETCH_SITE_HEADER), Some("same-origin"));
        assert_eq!(by_name(SEC_FETCH_USER_HEADER), None);
    }

    #[test]
    fn image_request_headers_are_image_subresource_shaped() {
        let headers = profile_request_headers_without_compression(
            StealthProfile::ChromeWindowsStable,
            BrowserRequestKind::ImageSubresource,
        );
        let by_name = |name: &str| {
            headers
                .as_slice()
                .iter()
                .find(|header| header.name == name)
                .map(|header| header.value)
        };

        assert_eq!(headers.len(), 6);
        // A real Chrome <img> load carries the resource-specific image Accept,
        // never a bare */* (which is a fetch-metadata tell).
        assert_eq!(by_name(ACCEPT_HEADER), Some(CHROMIUM_IMAGE_ACCEPT));
        assert_eq!(by_name(SEC_FETCH_DEST_HEADER), Some("image"));
        assert_eq!(by_name(SEC_FETCH_MODE_HEADER), Some("no-cors"));
        assert_eq!(by_name(SEC_FETCH_SITE_HEADER), Some("cross-site"));
        assert_eq!(by_name(ACCEPT_ENCODING_HEADER), None);
        assert_eq!(by_name(SEC_FETCH_USER_HEADER), None);
    }

    #[test]
    fn image_subresource_accept_tracks_the_persona_family() {
        let image_accept = |p| {
            profile_request_headers_without_compression(p, BrowserRequestKind::ImageSubresource)
                .as_slice()
                .iter()
                .find(|h| h.name == ACCEPT_HEADER)
                .map(|h| h.value)
        };
        // Chromium and Firefox carry their distinct, wire-verified image Accepts.
        assert_eq!(
            image_accept(StealthProfile::ChromeWindowsStable),
            Some(CHROMIUM_IMAGE_ACCEPT)
        );
        assert_eq!(
            image_accept(StealthProfile::FirefoxWindows),
            Some(FIREFOX_IMAGE_ACCEPT)
        );
        assert_eq!(
            image_accept(StealthProfile::FirefoxMacStable),
            Some(FIREFOX_IMAGE_ACCEPT)
        );
        assert_ne!(
            image_accept(StealthProfile::ChromeWindowsStable),
            image_accept(StealthProfile::FirefoxWindows),
            "image Accept must distinguish Chromium from Firefox"
        );
        // Safari's exact modern image Accept is not pinned here, so it keeps the
        // generic */* rather than a fabricated (and possibly unique) value, a
        // documented residual gap, not a silent default.
        assert_eq!(image_accept(StealthProfile::SafariMacStable), Some("*/*"));
    }

    #[test]
    fn audio_request_headers_are_media_subresource_shaped() {
        let headers = profile_request_headers_without_compression(
            StealthProfile::ChromeWindowsStable,
            BrowserRequestKind::AudioSubresource,
        );
        let by_name = |name: &str| {
            headers
                .as_slice()
                .iter()
                .find(|header| header.name == name)
                .map(|header| header.value)
        };

        assert_eq!(headers.len(), 6);
        assert_eq!(by_name(ACCEPT_HEADER), Some("*/*"));
        assert_eq!(by_name(SEC_FETCH_DEST_HEADER), Some("audio"));
        assert_eq!(by_name(SEC_FETCH_MODE_HEADER), Some("no-cors"));
        assert_eq!(by_name(SEC_FETCH_SITE_HEADER), Some("cross-site"));
        assert_eq!(by_name(ACCEPT_ENCODING_HEADER), None);
        assert_eq!(by_name(SEC_FETCH_USER_HEADER), None);
    }

    #[test]
    fn compressed_request_headers_include_accept_encoding() {
        let facts = profile_facts(StealthProfile::ChromeWindowsStable);
        let headers = profile_request_headers(
            StealthProfile::ChromeWindowsStable,
            BrowserRequestKind::Navigation,
        );
        let by_name = |name: &str| {
            headers
                .as_slice()
                .iter()
                .find(|header| header.name == name)
                .map(|header| header.value)
        };

        assert_eq!(headers.len(), 9);
        assert_eq!(by_name(ACCEPT_ENCODING_HEADER), Some(facts.accept_encoding));
    }

    #[test]
    fn canonical_navigation_header_names_use_browser_casing() {
        assert_eq!(
            canonical_navigation_header_name(USER_AGENT_HEADER),
            "User-Agent"
        );
        assert_eq!(canonical_navigation_header_name(ACCEPT_HEADER), "Accept");
        assert_eq!(
            canonical_navigation_header_name(ACCEPT_LANGUAGE_HEADER),
            "Accept-Language"
        );
        assert_eq!(
            canonical_navigation_header_name(ACCEPT_ENCODING_HEADER),
            "Accept-Encoding"
        );
        assert_eq!(
            canonical_navigation_header_name(SEC_FETCH_MODE_HEADER),
            "Sec-Fetch-Mode"
        );
        assert_eq!(
            canonical_navigation_header_name("x-custom-header"),
            "x-custom-header"
        );
    }
    #[test]
    fn unknown_platform_user_agent_never_infers_a_profile() {
        // Unknown platform must fail profile inference (return None) for ALL browser families.
        let edge_unknown = user_agent_facts("Mozilla/5.0 (CustomOS) Edg/131.0.0.0");
        assert_eq!(edge_unknown.platform, UserAgentPlatform::Unknown);
        assert_eq!(edge_unknown.inferred_profile, None);

        let ie_unknown = user_agent_facts("Trident/7.0; rv:11.0");
        assert_eq!(ie_unknown.platform, UserAgentPlatform::Unknown);
        assert_eq!(ie_unknown.inferred_profile, None);

        let opera_unknown = user_agent_facts("OPR/116.0.0.0");
        assert_eq!(opera_unknown.platform, UserAgentPlatform::Unknown);
        assert_eq!(opera_unknown.inferred_profile, None);

        let samsung_unknown = user_agent_facts("SamsungBrowser/26.0");
        assert_eq!(samsung_unknown.platform, UserAgentPlatform::Unknown);
        assert_eq!(samsung_unknown.inferred_profile, None);
    }

    #[test]
    fn user_agent_facts_parses_mobile_and_variant_tokens_correctly() {
        // Edge on Android (EdgA/)
        let edga =
            user_agent_facts("Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 EdgA/131.0.0.0");
        assert_eq!(edga.browser, UserAgentBrowser::Edge);
        assert_eq!(edga.platform, UserAgentPlatform::Android);
        assert_eq!(edga.browser_major_version, Some(131));

        // Edge on iOS (EdgiOS/)
        let edgios = user_agent_facts(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) EdgiOS/131.0.0.0",
        );
        assert_eq!(edgios.browser, UserAgentBrowser::Edge);
        assert_eq!(edgios.platform, UserAgentPlatform::Ios);
        assert_eq!(edgios.browser_major_version, Some(131));

        // Chrome on iOS (CriOS/)
        let crios = user_agent_facts(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) CriOS/131.0.0.0",
        );
        assert_eq!(crios.browser, UserAgentBrowser::Chrome);
        assert_eq!(crios.platform, UserAgentPlatform::Ios);
        assert_eq!(crios.browser_major_version, Some(131));

        // Opera on iOS (OPiOS/)
        let opios = user_agent_facts(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) OPiOS/116.0.0.0",
        );
        assert_eq!(opios.browser, UserAgentBrowser::Opera);
        assert_eq!(opios.platform, UserAgentPlatform::Ios);
        assert_eq!(opios.browser_major_version, Some(116));
    }

    #[test]
    fn get_profile_resolves_all_catalog_profiles_and_aliases() {
        for profile in ALL_PROFILES {
            let name = profile_name(*profile);
            let hp = get_profile(name).unwrap_or_else(|| panic!("get_profile failed for {name}"));
            assert_eq!(hp.user_agent, profile_user_agent(*profile));
        }

        assert!(get_profile("chrome-windows").is_some());
        assert!(get_profile("chrome-macos").is_some());
        assert!(get_profile("firefox-windows").is_some());
        assert!(get_profile("brave").is_some());
        assert!(get_profile("opera").is_some());
        assert!(get_profile("samsung-internet").is_some());
        assert_eq!(get_profile("unknown_invalid_profile"), None);
    }
}
