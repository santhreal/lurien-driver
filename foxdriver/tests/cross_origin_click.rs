//! Cross-origin click-delivery contract (live Firefox).
//!
//! Captcha checkboxes (Turnstile / hCaptcha / reCAPTCHA) live in a
//! *cross-origin* iframe: Turnstile in a doubly-nested one. This test pins
//! down, deterministically and with NO real vendor, exactly what click
//! delivery foxdriver guarantees, so a regression in the input path (or a
//! Firefox change) is caught here instead of silently zeroing a solver's
//! live success rate.
//!
//! Two-pair contract:
//!   POSITIVE, a real BiDi pointer click (top-context viewport OR
//!     iframe-context-local via `click_at_in`) lands a `isTrusted === true`
//!     event inside the cross-origin frame, single AND nested.
//!   NEGATIVE, a synthetic JS `.click()` reaches the element but arrives
//!     `isTrusted === false`: the exact reason a JS-dispatched click never
//!     solves a real captcha.
//!
//! Live test: requires `firefox` on PATH. Skips (does not fail) when absent
//! so non-browser CI stays green; Firefox-equipped runners enforce it.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use runtime_foxdriver::{launch_firefox, FoxBrowserConfig, FrameId, Page};

const IFRAME_LEFT: f64 = 60.0;
const IFRAME_TOP: f64 = 80.0;
const CB_LOCAL_X: f64 = 30.0;
const CB_LOCAL_Y: f64 = 34.0;
const INNER_LEFT_IN_MID: f64 = 20.0;
const INNER_TOP_IN_MID: f64 = 20.0;
const NESTED_TOP_X: f64 = IFRAME_LEFT + INNER_LEFT_IN_MID + CB_LOCAL_X;
const NESTED_TOP_Y: f64 = IFRAME_TOP + INNER_TOP_IN_MID + CB_LOCAL_Y;

fn parent_html(box_port: u16, mid_port: u16) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"></head>
<body style="margin:0;padding:0">
<iframe id="cap" src="http://127.0.0.1:{box_port}/box"
  style="position:absolute;left:{IFRAME_LEFT}px;top:{IFRAME_TOP}px;width:300px;height:65px;border:0"></iframe>
<iframe id="mid" src="http://127.0.0.1:{mid_port}/mid"
  style="position:absolute;left:{IFRAME_LEFT}px;top:{IFRAME_TOP}px;width:300px;height:200px;border:0;display:none"></iframe>
<script>
window.__delivered=false; window.__trusted=null; window.__moveTrusted=null;
window.addEventListener('mousemove', function(e){{ window.__moveTrusted=e.isTrusted; }}, true);
window.addEventListener('message', function(e){{
  if(e.data&&e.data.t==='clicked'){{ window.__delivered=true; window.__trusted=!!e.data.trusted; }}
  if(e.data&&e.data.t==='show_nested'){{ document.getElementById('cap').style.display='none'; document.getElementById('mid').style.display='block'; }}
}});
</script></body></html>"#
    )
}

const CHECKBOX_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"></head>
<body style="margin:0;padding:0">
<div id="cb" style="position:absolute;left:18px;top:22px;width:24px;height:24px;background:#3a7"></div>
<script>document.getElementById('cb').addEventListener('click',function(e){window.top.postMessage({t:'clicked',trusted:e.isTrusted},'*');});</script>
</body></html>"#;

fn mid_html(inner_port: u16) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"></head>
<body style="margin:0;padding:0">
<iframe id="inner" src="http://127.0.0.1:{inner_port}/inner"
  style="position:absolute;left:{INNER_LEFT_IN_MID}px;top:{INNER_TOP_IN_MID}px;width:250px;height:120px;border:0"></iframe>
</body></html>"#
    )
}

fn serve(listener: TcpListener, route: impl Fn(&str) -> Option<String> + Send + 'static) {
    for stream in listener.incoming() {
        let Ok(mut s) = stream else { continue };
        let mut buf = [0u8; 2048];
        let n = s.read(&mut buf).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        let path = req
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("/");
        let body = route(path);
        let (status, body) = match body {
            Some(b) => ("200 OK", b),
            None => ("404 Not Found", "no".to_string()),
        };
        let resp = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = s.write_all(resp.as_bytes());
        let _ = s.flush();
    }
}

fn firefox_present() -> bool {
    std::process::Command::new("firefox")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn delivered(page: &Page) -> (bool, Option<bool>) {
    let v = page
        .evaluate("JSON.stringify([window.__delivered===true, window.__trusted])")
        .await
        .ok()
        .and_then(|e| e.into_value::<String>().ok())
        .unwrap_or_default();
    let d = v.starts_with("[true");
    let t = if v.contains("true,true") {
        Some(true)
    } else if v.contains("true,false") {
        Some(false)
    } else {
        None
    };
    (d, t)
}

async fn reset(page: &Page) {
    let _ = page
        .evaluate("window.__delivered=false;window.__trusted=null;void 0")
        .await;
}

async fn nonmain_ctx(page: &Page) -> Option<FrameId> {
    let frames = page.frames().await.ok()?;
    let main = page.mainframe().await.ok().flatten();
    frames.into_iter().find(|c| Some(c) != main.as_ref())
}

async fn ctx_with_path(page: &Page, needle: &str) -> Option<FrameId> {
    for c in page.frames().await.ok()? {
        if let Ok(r) = page.evaluate_in_context("location.pathname", &c).await {
            if r.into_value::<String>()
                .map(|p| p.contains(needle))
                .unwrap_or(false)
            {
                return Some(c);
            }
        }
    }
    None
}

async fn run() {
    let l_parent = TcpListener::bind("127.0.0.1:0").unwrap();
    let l_box = TcpListener::bind("127.0.0.1:0").unwrap();
    let l_mid = TcpListener::bind("127.0.0.1:0").unwrap();
    let l_inner = TcpListener::bind("127.0.0.1:0").unwrap();
    let p_parent = l_parent.local_addr().unwrap().port();
    let p_box = l_box.local_addr().unwrap().port();
    let p_mid = l_mid.local_addr().unwrap().port();
    let p_inner = l_inner.local_addr().unwrap().port();

    let phtml = parent_html(p_box, p_mid);
    std::thread::spawn(move || serve(l_parent, move |p| (p == "/").then(|| phtml.clone())));
    std::thread::spawn(move || {
        serve(l_box, |p| {
            p.starts_with("/box").then(|| CHECKBOX_HTML.to_string())
        })
    });
    let midhtml = mid_html(p_inner);
    std::thread::spawn(move || {
        serve(l_mid, move |p| {
            p.starts_with("/mid").then(|| midhtml.clone())
        })
    });
    std::thread::spawn(move || {
        serve(l_inner, |p| {
            p.starts_with("/inner").then(|| CHECKBOX_HTML.to_string())
        })
    });

    let page = launch_firefox(FoxBrowserConfig {
        headless: true,
        viewport_width: 1280,
        viewport_height: 800,
        ..Default::default()
    })
    .await
    .expect("launch firefox");
    page.goto(&format!("http://127.0.0.1:{p_parent}/"))
        .await
        .expect("goto");

    for _ in 0..50 {
        if page.frames().await.unwrap().len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let box_ctx = nonmain_ctx(&page)
        .await
        .expect("cross-origin iframe context tracked");
    let top_x = IFRAME_LEFT + CB_LOCAL_X;
    let top_y = IFRAME_TOP + CB_LOCAL_Y;

    // POSITIVE 1: top-context viewport click → trusted, single cross-origin.
    reset(&page).await;
    page.click_at(top_x, top_y)
        .await
        .expect("top-context click");
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        delivered(&page).await,
        (true, Some(true)),
        "top-context click must deliver a TRUSTED event into the cross-origin iframe"
    );

    // POSITIVE 2: iframe-context local click → trusted, single cross-origin.
    reset(&page).await;
    page.click_at_in(&box_ctx, CB_LOCAL_X, CB_LOCAL_Y)
        .await
        .expect("iframe-context click");
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        delivered(&page).await,
        (true, Some(true)),
        "click_at_in must deliver a TRUSTED event in the iframe's own context"
    );

    // NEGATIVE: synthetic JS click → reaches the element but UNtrusted.
    reset(&page).await;
    page.evaluate_in_context("document.getElementById('cb').click()", &box_ctx)
        .await
        .expect("synthetic click");
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(delivered(&page).await, (true, Some(false)), "synthetic JS click must arrive UNtrusted (the reason JS clicks never solve a real captcha)");

    // NESTED (Turnstile-shaped): swap to the parent→mid→inner widget.
    let _ = page
        .evaluate("window.postMessage({t:'show_nested'},'*')")
        .await;
    for _ in 0..60 {
        if page.frames().await.unwrap().len() >= 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // POSITIVE 3: top-context click routes two frames deep → trusted.
    reset(&page).await;
    page.click_at(NESTED_TOP_X, NESTED_TOP_Y)
        .await
        .expect("nested top-context click");
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        delivered(&page).await,
        (true, Some(true)),
        "top-context click must route into a doubly-nested cross-origin frame, trusted"
    );

    // POSITIVE 4: innermost-context local click → trusted.
    let inner = ctx_with_path(&page, "inner")
        .await
        .expect("inner context resolved");
    reset(&page).await;
    page.click_at_in(&inner, CB_LOCAL_X, CB_LOCAL_Y)
        .await
        .expect("inner-context click");
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        delivered(&page).await,
        (true, Some(true)),
        "click_at_in must deliver a TRUSTED event into the innermost nested frame"
    );

    // FRAME-TREE STRUCTURE: the same doubly-nested cross-origin topology
    // (main → mid → inner) must come back from `frame_tree`/`FrameGraph` with
    // REAL parent linkage and depth, not the flat `parent: root, depth: 1`
    // collapse the old `page.frames()`-based snapshot produced. This is the
    // structural source of truth a solver reasons over for the reCAPTCHA
    // bframe-inside-anchor dance.
    let tree = page
        .frame_tree()
        .await
        .expect("frame_tree must walk getTree");
    // The /inner frame is two levels below its top-level document, so its
    // parent must itself have a parent (i.e. it is NOT a direct child of a
    // top-level context). Prove the chain length, not just presence.
    let inner_node = tree
        .iter()
        .find(|n| n.url.contains("/inner"))
        .expect("inner frame present in the BiDi tree");
    let inner_parent = inner_node
        .parent
        .as_ref()
        .expect("inner frame has a parent");
    let mid_node = tree
        .iter()
        .find(|n| &n.id == inner_parent)
        .expect("inner's parent (mid) present in the tree");
    assert!(
        mid_node.parent.is_some(),
        "mid frame must itself have a parent, inner is two levels deep, \
         a flat tree would have made inner a direct child of the top context"
    );
    assert!(
        inner_node.depth >= 2,
        "inner frame depth must be ≥2 (main→mid→inner); got {}. \
         the flat-snapshot bug would report 0/1",
        inner_node.depth
    );

    // And the assembled FrameGraph must expose the same chain with a synthetic
    // root, so `deepest_captcha`/`ancestors_inclusive` reasoning is live.
    let graph = runtime_foxdriver::FrameGraph::snapshot(&page)
        .await
        .expect("FrameGraph::snapshot");
    let inner_idx = graph
        .nodes
        .iter()
        .position(|n| n.url.contains("/inner"))
        .expect("inner frame present in the graph");
    assert!(
        graph.nodes[inner_idx].depth >= 3,
        "in the assembled graph (root=0, top=1, mid=2, inner=3) the inner frame \
         depth must be ≥3; got {}",
        graph.nodes[inner_idx].depth
    );
    // Walking inner's ancestors must reach the synthetic root through the real
    // chain (length ≥4: inner → mid → main → root).
    let chain = graph.ancestors_inclusive(inner_idx);
    assert!(
        chain.len() >= 4 && *chain.last().unwrap() == 0,
        "ancestor chain from inner must climb the real nesting to root; got {chain:?}"
    );

    // POSITIVE 5: move_mouse_to dispatches a TRUSTED mousemove (isTrusted=true).
    // This is the primitive every human-trajectory generator now routes each
    // sampled point through; a synthetic JS mousemove would be isTrusted=false.
    // (500,400) is clear of the widgets, in the top page.
    let _ = page.evaluate("window.__moveTrusted=null; void 0").await;
    page.move_mouse_to(500.0, 400.0)
        .await
        .expect("trusted move");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let move_trusted = page
        .evaluate("JSON.stringify(window.__moveTrusted)")
        .await
        .ok()
        .and_then(|e| e.into_value::<String>().ok())
        .unwrap_or_default();
    assert_eq!(move_trusted, "true", "move_mouse_to must dispatch a TRUSTED mousemove (isTrusted=true), not a synthetic JS event");
}

/// Each browser test in this file launches its OWN Firefox; cargo runs the test
/// fns on parallel threads, and two simultaneous launches race the BiDi WebSocket
/// connect (rustenium `BidiSession::new`), flaking one. Serialize every browser
/// test through this lock so the file stays green no matter how many are added.
/// Poison is ignored (a panicking test still releases the browser slot).
static BROWSER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn cross_origin_trusted_click_delivery() {
    if !firefox_present() {
        eprintln!("SKIP cross_origin_trusted_click_delivery: firefox not on PATH");
        return;
    }
    let _serial = BROWSER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(run());
}

// ── find_element_centre_in_frames: the cross-origin coordinate-DISCOVERY path ──
//
// The click-delivery test above feeds `click_at` HARD-CODED coordinates. A real
// solver does not know where the checkbox is: it calls
// `find_element_centre_in_frames(selector)` to DISCOVER the element's
// viewport coordinate by summing the cross-origin iframe's own bounding box with
// the element's rect *inside* the (opaque) iframe, the one path that turns a CSS
// selector into a clickable point across an OOPIF boundary. This pins that
// summing end-to-end AND proves the discovered point is itself trusted-clickable,
// so a regression in the offset math silently zeroing a solver's hit rate is
// caught here, deterministically, with no real vendor.

// A single cross-origin iframe at a known offset; the checkbox carries a UNIQUE
// id so the frame walk can't accidentally match a same-markup nested copy.
const DISCOVER_IFRAME_LEFT: f64 = 60.0;
const DISCOVER_IFRAME_TOP: f64 = 80.0;
const DISCOVER_CB_LOCAL_LEFT: f64 = 18.0;
const DISCOVER_CB_LOCAL_TOP: f64 = 22.0;
const DISCOVER_CB_SIZE: f64 = 24.0;
// Expected discovered viewport centre = iframe offset + in-frame element centre.
const DISCOVER_EXPECT_X: f64 =
    DISCOVER_IFRAME_LEFT + DISCOVER_CB_LOCAL_LEFT + DISCOVER_CB_SIZE / 2.0;
const DISCOVER_EXPECT_Y: f64 = DISCOVER_IFRAME_TOP + DISCOVER_CB_LOCAL_TOP + DISCOVER_CB_SIZE / 2.0;

fn discover_parent_html(box_port: u16) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"></head>
<body style="margin:0;padding:0">
<iframe id="cap" src="http://127.0.0.1:{box_port}/box"
  style="position:absolute;left:{DISCOVER_IFRAME_LEFT}px;top:{DISCOVER_IFRAME_TOP}px;width:300px;height:65px;border:0"></iframe>
<script>
window.__delivered=false; window.__trusted=null;
window.addEventListener('message', function(e){{
  if(e.data&&e.data.t==='clicked'){{ window.__delivered=true; window.__trusted=!!e.data.trusted; }}
}});
</script></body></html>"#
    )
}

const DISCOVER_CHECKBOX_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"></head>
<body style="margin:0;padding:0">
<div id="cf-checkbox" style="position:absolute;left:18px;top:22px;width:24px;height:24px;background:#3a7"></div>
<script>document.getElementById('cf-checkbox').addEventListener('click',function(e){window.top.postMessage({t:'clicked',trusted:e.isTrusted},'*');});</script>
</body></html>"#;

async fn run_find_centre() {
    let l_parent = TcpListener::bind("127.0.0.1:0").unwrap();
    let l_box = TcpListener::bind("127.0.0.1:0").unwrap();
    let p_parent = l_parent.local_addr().unwrap().port();
    let p_box = l_box.local_addr().unwrap().port();

    let phtml = discover_parent_html(p_box);
    std::thread::spawn(move || serve(l_parent, move |p| (p == "/").then(|| phtml.clone())));
    std::thread::spawn(move || {
        serve(l_box, |p| {
            p.starts_with("/box")
                .then(|| DISCOVER_CHECKBOX_HTML.to_string())
        })
    });

    let page = launch_firefox(FoxBrowserConfig {
        headless: true,
        viewport_width: 1280,
        viewport_height: 800,
        ..Default::default()
    })
    .await
    .expect("launch firefox");
    page.goto(&format!("http://127.0.0.1:{p_parent}/"))
        .await
        .expect("goto");

    // Wait for the cross-origin iframe to attach (the exact race a real solver
    // hits: the widget iframe is injected a few hundred ms after navigation).
    for _ in 0..50 {
        if page.frames().await.unwrap().len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // DISCOVER: sum the opaque cross-origin iframe's offset with the in-frame
    // checkbox rect to get a viewport coordinate.
    let centre = runtime_foxdriver::frame::find_element_centre_in_frames(&page, "#cf-checkbox")
        .await
        .expect("find_element_centre_in_frames must not error")
        .expect("checkbox inside the cross-origin iframe must be located");

    // CONTRACT 1: the discovered coordinate equals iframe-offset + in-frame
    // centre, NOT the raw in-frame rect (which would land ~ (60,80) too high-left,
    // outside the checkbox) and NOT (0,0) (an unresolved offset). Tolerance covers
    // sub-pixel layout rounding only.
    assert!(
        (centre.0 - DISCOVER_EXPECT_X).abs() < 2.0 && (centre.1 - DISCOVER_EXPECT_Y).abs() < 2.0,
        "discovered cross-origin centre {centre:?} must equal iframe offset + in-frame centre \
         (~{DISCOVER_EXPECT_X},{DISCOVER_EXPECT_Y}); a wrong offset would click outside the checkbox"
    );

    // CONTRACT 2: clicking the DISCOVERED coordinate lands a TRUSTED event on the
    // checkbox inside the cross-origin iframe (the full solver path, end to end).
    reset(&page).await;
    page.click_at(centre.0, centre.1)
        .await
        .expect("click at discovered centre");
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        delivered(&page).await,
        (true, Some(true)),
        "a click at the DISCOVERED cross-origin centre must deliver a TRUSTED event onto the checkbox"
    );
}

#[test]
fn cross_origin_find_element_centre_then_trusted_click() {
    if !firefox_present() {
        eprintln!("SKIP cross_origin_find_element_centre_then_trusted_click: firefox not on PATH");
        return;
    }
    let _serial = BROWSER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(run_find_centre());
}

// ── find_element_centre_in_frames at TWO nesting levels, the depth-2 offset sum ──
//
// The C019 discovery test above proves the offset math for a SINGLE cross-origin
// iframe (element one level below main). But `find_element_centre_in_frames` used
// to sum only the MAIN frame's direct-iframe offsets, so an element TWO frames
// deep (Turnstile's checkbox inside a nested `challenges.cloudflare.com` iframe)
// got at most one level of offset and the discovered coordinate landed at the
// wrong viewport point. This pins the multi-level sum end-to-end: parent → mid →
// inner, each at a known offset, with a uniquely-id'd checkbox at the bottom.

const DEEP_MID_LEFT: f64 = 50.0;
const DEEP_MID_TOP: f64 = 70.0;
const DEEP_INNER_LEFT_IN_MID: f64 = 30.0;
const DEEP_INNER_TOP_IN_MID: f64 = 40.0;
const DEEP_CB_LOCAL_LEFT: f64 = 25.0;
const DEEP_CB_LOCAL_TOP: f64 = 35.0;
const DEEP_CB_SIZE: f64 = 20.0;
// Expected discovered viewport centre = mid offset + inner-in-mid offset +
// in-inner checkbox centre. A one-level-only sum would miss the inner offset and
// land ~ (30,40) too high-left, outside the checkbox.
const DEEP_EXPECT_X: f64 =
    DEEP_MID_LEFT + DEEP_INNER_LEFT_IN_MID + DEEP_CB_LOCAL_LEFT + DEEP_CB_SIZE / 2.0;
const DEEP_EXPECT_Y: f64 =
    DEEP_MID_TOP + DEEP_INNER_TOP_IN_MID + DEEP_CB_LOCAL_TOP + DEEP_CB_SIZE / 2.0;

fn deep_parent_html(mid_port: u16) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"></head>
<body style="margin:0;padding:0">
<iframe id="dmid" src="http://127.0.0.1:{mid_port}/dmid"
  style="position:absolute;left:{DEEP_MID_LEFT}px;top:{DEEP_MID_TOP}px;width:340px;height:260px;border:0"></iframe>
<script>
window.__delivered=false; window.__trusted=null;
window.addEventListener('message', function(e){{
  if(e.data&&e.data.t==='clicked'){{ window.__delivered=true; window.__trusted=!!e.data.trusted; }}
}});
</script></body></html>"#
    )
}

fn deep_mid_html(inner_port: u16) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"></head>
<body style="margin:0;padding:0">
<iframe id="dinner" src="http://127.0.0.1:{inner_port}/dinner"
  style="position:absolute;left:{DEEP_INNER_LEFT_IN_MID}px;top:{DEEP_INNER_TOP_IN_MID}px;width:280px;height:180px;border:0"></iframe>
</body></html>"#
    )
}

const DEEP_INNER_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"></head>
<body style="margin:0;padding:0">
<div id="deep-cb" style="position:absolute;left:25px;top:35px;width:20px;height:20px;background:#3a7"></div>
<script>document.getElementById('deep-cb').addEventListener('click',function(e){window.top.postMessage({t:'clicked',trusted:e.isTrusted},'*');});</script>
</body></html>"#;

async fn run_deep_find_centre() {
    let l_parent = TcpListener::bind("127.0.0.1:0").unwrap();
    let l_mid = TcpListener::bind("127.0.0.1:0").unwrap();
    let l_inner = TcpListener::bind("127.0.0.1:0").unwrap();
    let p_parent = l_parent.local_addr().unwrap().port();
    let p_mid = l_mid.local_addr().unwrap().port();
    let p_inner = l_inner.local_addr().unwrap().port();

    let phtml = deep_parent_html(p_mid);
    std::thread::spawn(move || serve(l_parent, move |p| (p == "/").then(|| phtml.clone())));
    let midhtml = deep_mid_html(p_inner);
    std::thread::spawn(move || {
        serve(l_mid, move |p| {
            p.starts_with("/dmid").then(|| midhtml.clone())
        })
    });
    std::thread::spawn(move || {
        serve(l_inner, |p| {
            p.starts_with("/dinner")
                .then(|| DEEP_INNER_HTML.to_string())
        })
    });

    let page = launch_firefox(FoxBrowserConfig {
        headless: true,
        viewport_width: 1280,
        viewport_height: 800,
        ..Default::default()
    })
    .await
    .expect("launch firefox");
    page.goto(&format!("http://127.0.0.1:{p_parent}/"))
        .await
        .expect("goto");

    // Wait for BOTH nested iframes to attach (3 contexts: parent + mid + inner).
    for _ in 0..60 {
        if page.frames().await.unwrap().len() >= 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // DISCOVER: the checkbox lives two cross-origin frames deep. The summed
    // viewport centre must equal mid + inner + in-inner offsets.
    let centre = runtime_foxdriver::frame::find_element_centre_in_frames(&page, "#deep-cb")
        .await
        .expect("find_element_centre_in_frames must not error")
        .expect("checkbox two cross-origin frames deep must be located");

    assert!(
        (centre.0 - DEEP_EXPECT_X).abs() < 2.0 && (centre.1 - DEEP_EXPECT_Y).abs() < 2.0,
        "discovered depth-2 centre {centre:?} must equal the FULL ancestor-chain sum \
         (~{DEEP_EXPECT_X},{DEEP_EXPECT_Y}); a one-level-only sum would land near \
         ({DEEP_INNER_LEFT_IN_MID},{DEEP_INNER_TOP_IN_MID})-ish too high-left, outside the checkbox"
    );

    // And a click at the DISCOVERED point lands a TRUSTED event two frames deep.
    reset(&page).await;
    page.click_at(centre.0, centre.1)
        .await
        .expect("click at discovered depth-2 centre");
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        delivered(&page).await,
        (true, Some(true)),
        "a click at the DISCOVERED depth-2 centre must deliver a TRUSTED event onto the nested checkbox"
    );
}

#[test]
fn cross_origin_find_element_centre_two_levels_deep() {
    if !firefox_present() {
        eprintln!("SKIP cross_origin_find_element_centre_two_levels_deep: firefox not on PATH");
        return;
    }
    let _serial = BROWSER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(run_deep_find_centre());
}
