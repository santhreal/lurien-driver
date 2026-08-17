//! Cross-origin image-GRID click-delivery contract (live Firefox).
//!
//! reCAPTCHA v2's image grid and hCaptcha render their tile table inside a
//! **cross-origin** iframe. A solver must (a) find those tiles at all, parent
//! JS cannot see into a cross-origin child, and (b) click a chosen tile with a
//! TRUSTED event. This test pins both down deterministically with NO real
//! vendor:
//!
//!   - [`find_tiles_in_frames`] returns every tile's bounding box in
//!     main-viewport coordinates (iframe offset summed in), in document order
//!     with stable indices.
//!   - A top-context [`Page::click_at`] at a returned tile centre delivers an
//!     `isTrusted === true` click to the CORRECT tile inside the cross-origin
//!     frame.
//!   - NEGATIVE twin: a synthetic in-frame `.click()` reaches the tile but
//!     arrives `isTrusted === false`: the exact reason the old
//!     `dispatchEvent(new MouseEvent(...))` grid path could never solve a real
//!     image challenge.
//!
//! Live test: requires `firefox` on PATH. Skips (does not fail) when absent so
//! non-browser CI stays green; Firefox-equipped runners enforce it.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use runtime_foxdriver::frame::find_tiles_in_frames;
use runtime_foxdriver::{launch_firefox, FoxBrowserConfig, FrameId, Page};

const IFRAME_LEFT: f64 = 60.0;
const IFRAME_TOP: f64 = 80.0;
const TILE: f64 = 50.0;
const GRID: usize = 3;

/// Parent page: one cross-origin iframe holding the grid, plus a collector for
/// the `{t:'tileclick', id, trusted}` messages each tile posts to the top
/// window when clicked.
fn parent_html(grid_port: u16) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"></head>
<body style="margin:0;padding:0">
<iframe id="grid" src="http://127.0.0.1:{grid_port}/grid"
  style="position:absolute;left:{IFRAME_LEFT}px;top:{IFRAME_TOP}px;width:300px;height:300px;border:0"></iframe>
<script>
window.__clicks = [];
window.addEventListener('message', function(e){{
  if (e.data && e.data.t === 'tileclick') {{
    window.__clicks.push({{ id: e.data.id, trusted: !!e.data.trusted }});
  }}
}});
</script></body></html>"#
    )
}

/// Cross-origin grid document: a 3×3 grid of absolutely-positioned
/// `.rc-imageselect-tile` cells (50px, no gaps) so getBoundingClientRect is
/// deterministic. Each tile reports its id + isTrusted to the top window.
fn grid_html() -> String {
    let mut tiles = String::new();
    for row in 0..GRID {
        for col in 0..GRID {
            let idx = row * GRID + col;
            let left = col as f64 * TILE;
            let top = row as f64 * TILE;
            tiles.push_str(&format!(
                r#"<div class="rc-imageselect-tile" id="tile-{idx}" style="position:absolute;left:{left}px;top:{top}px;width:{TILE}px;height:{TILE}px;background:#39a"></div>"#
            ));
        }
    }
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"></head>
<body style="margin:0;padding:0">
{tiles}
<script>
document.querySelectorAll('.rc-imageselect-tile').forEach(function(el){{
  el.addEventListener('click', function(e){{
    window.top.postMessage({{ t:'tileclick', id: el.id, trusted: e.isTrusted }}, '*');
  }});
}});
</script></body></html>"#
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
        let (status, body) = match route(path) {
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

/// Read the parent's collected `{id, trusted}` click records.
async fn clicks(page: &Page) -> Vec<(String, bool)> {
    let raw = page
        .evaluate("JSON.stringify(window.__clicks || [])")
        .await
        .ok()
        .and_then(|e| e.into_value::<String>().ok())
        .unwrap_or_default();
    serde_json::from_str::<Vec<serde_json::Value>>(&raw)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| {
            Some((
                v["id"].as_str()?.to_string(),
                v["trusted"].as_bool().unwrap_or(false),
            ))
        })
        .collect()
}

/// Resolve the cross-origin grid frame context (the non-main one).
async fn grid_ctx(page: &Page) -> Option<FrameId> {
    let frames = page.frames().await.ok()?;
    let main = page.mainframe().await.ok().flatten();
    frames.into_iter().find(|c| Some(c) != main.as_ref())
}

async fn run() {
    let l_parent = TcpListener::bind("127.0.0.1:0").unwrap();
    let l_grid = TcpListener::bind("127.0.0.1:0").unwrap();
    let p_parent = l_parent.local_addr().unwrap().port();
    let p_grid = l_grid.local_addr().unwrap().port();

    let phtml = parent_html(p_grid);
    std::thread::spawn(move || serve(l_parent, move |p| (p == "/").then(|| phtml.clone())));
    std::thread::spawn(move || serve(l_grid, |p| p.starts_with("/grid").then(grid_html)));

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
        .expect("goto parent");

    // Wait for the cross-origin grid frame to attach.
    for _ in 0..50 {
        if page.frames().await.unwrap().len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // --- CONTRACT 1: find_tiles_in_frames sees all 9 cross-origin tiles, in
    // order, with correct main-viewport centres. ---
    let mut tiles = find_tiles_in_frames(&page, ".rc-imageselect-tile")
        .await
        .expect("tile search");
    assert_eq!(
        tiles.len(),
        GRID * GRID,
        "must find all {} cross-origin grid tiles, got {}",
        GRID * GRID,
        tiles.len()
    );
    tiles.sort_by_key(|t| t.index);
    for (i, t) in tiles.iter().enumerate() {
        assert_eq!(t.index, i, "tiles returned in document order");
        let col = (i % GRID) as f64;
        let row = (i / GRID) as f64;
        let (cx, cy) = t.centre();
        let exp_x = IFRAME_LEFT + col * TILE + TILE / 2.0;
        let exp_y = IFRAME_TOP + row * TILE + TILE / 2.0;
        assert!(
            (cx - exp_x).abs() <= 2.0 && (cy - exp_y).abs() <= 2.0,
            "tile {i} centre ({cx},{cy}) must be viewport-relative ~({exp_x},{exp_y}) \
             (iframe offset summed in)"
        );
    }

    // --- CONTRACT 2: a top-context click_at at a tile centre delivers a
    // TRUSTED click to the CORRECT cross-origin tile. ---
    let by_index = |i: usize| tiles.iter().find(|t| t.index == i).unwrap().centre();
    for &i in &[0usize, 4, 8] {
        let (x, y) = by_index(i);
        page.click_at(x, y).await.expect("trusted tile click");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let recorded = clicks(&page).await;
    for &i in &[0usize, 4, 8] {
        let want = format!("tile-{i}");
        let hit = recorded.iter().find(|(id, _)| id == &want);
        assert!(
            hit.is_some(),
            "click_at on tile {i} centre must reach tile-{i} in the cross-origin frame; \
             recorded={recorded:?}"
        );
        assert!(
            hit.unwrap().1,
            "click_at on tile {i} must be isTrusted === true (the only click a real \
             image CAPTCHA accepts); recorded={recorded:?}"
        );
    }
    // The trusted clicks must not bleed into tiles we did not target.
    assert!(
        !recorded
            .iter()
            .any(|(id, _)| id == "tile-1" || id == "tile-5"),
        "only targeted tiles should be clicked; recorded={recorded:?}"
    );

    // --- CONTRACT 3 (NEGATIVE): a synthetic in-frame .click() reaches the tile
    // but arrives UNtrusted, the failure mode the trusted path replaces. ---
    let ctx = grid_ctx(&page).await.expect("grid frame context");
    page.evaluate_in_context("document.getElementById('tile-2').click()", &ctx)
        .await
        .expect("synthetic in-frame click");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let recorded = clicks(&page).await;
    let synth = recorded
        .iter()
        .find(|(id, _)| id == "tile-2")
        .expect("synthetic click should reach the element");
    assert!(
        !synth.1,
        "a synthetic JS .click() must arrive isTrusted === false; recorded={recorded:?}"
    );

    let _ = page.close().await;
}

#[test]
fn cross_origin_grid_trusted_click_delivery() {
    if !firefox_present() {
        eprintln!("SKIP cross_origin_grid_trusted_click_delivery: firefox not on PATH");
        return;
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(run());
}
