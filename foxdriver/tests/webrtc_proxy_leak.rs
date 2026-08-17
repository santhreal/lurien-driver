//! WebRTC must not leak the host's real IP past a configured proxy (live Firefox).
//!
//! A proxy covers HTTP, but WebRTC ICE gathers server-reflexive (srflx)
//! candidates by opening a DIRECT UDP socket to a STUN server, bypassing the
//! proxy and exposing the host's real PUBLIC IP. Behind a proxy that real IP
//! contradicts the proxy egress, a hard deanonymization. `proxy_prefs` now
//! emits `media.peerconnection.ice.proxy_only` (+ `no_host` +
//! `default_address_only`) whenever a proxy is configured, confining ICE to the
//! proxy. This pins the behaviour as a DIFFERENTIAL on the real browser:
//!
//!   BASELINE (no proxy)  → an `srflx` candidate IS gathered (the leak vector,
//!                          proving Firefox + this harness really reach STUN).
//!   FIXED (proxy set)    → NO `srflx` candidate is gathered (ICE confined to
//!                          the proxy; the real public IP never escapes).
//!
//! Live test: requires `firefox` on PATH **and** outbound UDP to a public STUN
//! server. Skips (does not fail) when either is missing, so offline/non-browser
//! CI stays green. Never prints the discovered IP, asserts only on candidate
//! TYPE presence/absence.

use std::time::Duration;

use runtime_foxdriver::{launch_firefox, FoxBrowserConfig, Page, ProxyConfig};

const STUN_URL: &str = "stun:stun.l.google.com:19302";

fn firefox_present() -> bool {
    std::process::Command::new("firefox")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// True iff this host can reach the public STUN server over UDP (so the
/// BASELINE arm can legitimately expect an srflx candidate). A raw STUN binding
/// request (same path Firefox's ICE agent would take).
fn stun_reachable() -> bool {
    use std::net::UdpSocket;
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else {
        return false;
    };
    let _ = sock.set_read_timeout(Some(Duration::from_secs(3)));
    // STUN Binding Request: type 0x0001, len 0, magic cookie, 12-byte txid.
    let mut msg = vec![0x00, 0x01, 0x00, 0x00, 0x21, 0x12, 0xA4, 0x42];
    msg.extend_from_slice(&[0xAB; 12]);
    let Ok(addrs) = std::net::ToSocketAddrs::to_socket_addrs(&("stun.l.google.com", 19302)) else {
        return false;
    };
    let Some(addr) = addrs.into_iter().next() else {
        return false;
    };
    if sock.send_to(&msg, addr).is_err() {
        return false;
    }
    let mut buf = [0u8; 256];
    matches!(sock.recv_from(&mut buf), Ok((n, _)) if n >= 20)
}

/// Inject an RTCPeerConnection that gathers ICE against STUN, then poll the
/// accumulated candidate strings. Returns the full candidate list (each a SDP
/// `candidate:...` line (the `typ srflx`/`typ host` token is what we assert on)).
async fn gather_ice_candidates(page: &Page) -> Vec<String> {
    let setup = format!(
        r#"(() => {{
            window.__cands = [];
            window.__gatherDone = false;
            let pc;
            try {{
                pc = new RTCPeerConnection({{ iceServers: [{{ urls: '{STUN_URL}' }}] }});
            }} catch (e) {{ window.__gatherErr = String(e); return; }}
            pc.onicecandidate = (e) => {{
                if (e.candidate) window.__cands.push(e.candidate.candidate);
                else window.__gatherDone = true;
            }};
            pc.createDataChannel('probe');
            pc.createOffer().then((o) => pc.setLocalDescription(o)).catch((e) => {{ window.__gatherErr = String(e); }});
        }})()"#
    );
    let _ = page.evaluate(&setup).await;

    // Poll until gathering completes or a wall-clock budget elapses.
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(150)).await;
        let done = page
            .evaluate("window.__gatherDone === true")
            .await
            .ok()
            .and_then(|v| v.into_value::<bool>().ok())
            .unwrap_or(false);
        if done {
            break;
        }
    }

    page.evaluate("JSON.stringify(window.__cands || [])")
        .await
        .ok()
        .and_then(|v| v.into_value::<String>().ok())
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
}

async fn launch(config: FoxBrowserConfig) -> Page {
    let page = launch_firefox(config).await.expect("launch firefox");
    // about:blank loads with no network, so the FIXED arm's proxy (pointed at a
    // dead port) cannot block page load (only the WebRTC ICE path is exercised).
    page.goto("about:blank").await.expect("goto about:blank");
    page
}

async fn run_baseline_leaks() {
    let page = launch(FoxBrowserConfig {
        headless: true,
        viewport_width: 800,
        viewport_height: 600,
        ..Default::default()
    })
    .await;
    let cands = gather_ice_candidates(&page).await;
    let _ = page.close().await;

    let has_srflx = cands.iter().any(|c| c.contains("typ srflx"));
    assert!(
        has_srflx,
        "BASELINE (no proxy): expected an srflx candidate proving the real public \
         IP escapes via WebRTC, got {} candidate(s) with none srflx. (If this \
         flakes, STUN egress from Firefox differs from the precheck.)",
        cands.len()
    );
}

async fn run_proxy_closes_leak() {
    // A dead SOCKS port: enough to make `proxy_only` confine ICE to a proxy that
    // cannot relay STUN, so no srflx can be gathered. about:blank still loads.
    let page = launch(FoxBrowserConfig {
        headless: true,
        viewport_width: 800,
        viewport_height: 600,
        profile_dir: Some(
            std::env::temp_dir()
                .join(format!("fox-webrtc-leak-{}", std::process::id()))
                .to_string_lossy()
                .into_owned(),
        ),
        proxy: Some(ProxyConfig::from_url("socks5://127.0.0.1:1").unwrap()),
        ..Default::default()
    })
    .await;
    let cands = gather_ice_candidates(&page).await;
    let _ = page.close().await;

    let leaked: Vec<&String> = cands.iter().filter(|c| c.contains("typ srflx")).collect();
    assert!(
        leaked.is_empty(),
        "FIXED (proxy + ice.proxy_only): WebRTC must gather NO srflx candidate. \
         the real public IP must not escape the proxy. Leaked: {leaked:?}"
    );
}

static BROWSER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn webrtc_srflx_leak_present_without_proxy() {
    if !firefox_present() {
        eprintln!("SKIP webrtc_srflx_leak_present_without_proxy: firefox not on PATH");
        return;
    }
    if !stun_reachable() {
        eprintln!("SKIP webrtc_srflx_leak_present_without_proxy: STUN egress blocked");
        return;
    }
    let _serial = BROWSER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(run_baseline_leaks());
}

#[test]
fn webrtc_proxy_only_closes_the_srflx_leak() {
    if !firefox_present() {
        eprintln!("SKIP webrtc_proxy_only_closes_the_srflx_leak: firefox not on PATH");
        return;
    }
    if !stun_reachable() {
        eprintln!("SKIP webrtc_proxy_only_closes_the_srflx_leak: STUN egress blocked");
        return;
    }
    let _serial = BROWSER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(run_proxy_closes_leak());
}
