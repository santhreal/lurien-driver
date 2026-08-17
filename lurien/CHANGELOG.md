# Changelog

## 0.1.0 - 2026-08-16

- One verb registry behind three transports. `Session::call(verb, args)` is the only
  entry point; the CLI, `lurien-mcp`, and `lurien serve` generate their surfaces from
  the same `VerbSpec`, so none can offer a verb or argument the others lack.
- 38 verbs across ten domains: `page`, `dom`, `input`, `frame`, `storage`, `state`,
  `net`, `dialog`, `observe`, `profile`. One verb per file; a new verb is that file
  plus one line in its domain's `SPECS`.
- `lurien serve` replaces the separate `guise-bridge` daemon and speaks the same wire
  protocol (`GET /v1/health`, `POST /v1/browser/command`, schema version 1). Legacy
  command names map onto verbs instead of being reimplemented, so an HTTP client and a
  CLI user get identical behavior. Sessions are named by `browser_context_id` and run
  concurrently.
- Network capture, dialog capture, and the sensor grid are armed at launch, so a verb
  that reads them never reports an empty log as no traffic. `LURIEN_SENSORS=0` opts out
  of the preload script.
- `net` redacts credential headers and sensitive query values before any face sees a
  row. `net-tokens` reports where a credential appears, never its value.
- `state` snapshots cookies plus local and session storage under a version; `state-set`
  refuses a snapshot from another version instead of half-applying it.
- `docs/VERBS.md` is generated from the registry; a stale copy fails the test suite.
- Argument decoding is one path: unknown argument, missing required argument, and wrong
  type all fail closed before the engine is touched.
- Vendor bindings for turnstile, arkose, geetest, datadome, akamai, hcaptcha audio, and
  proof of work. Every interactive kind now has a binding, and a vendor name reaching
  the engine additions is a test failure.
- Ahura reads `LURIEN_BRIDGE_URL`; `AHURA_GUISE_BRIDGE_URL` is honored for one release.
- Challenges are classified and cleared inside the engine. `engine/additions/challenge/`
  is a chrome-privileged actor pair started by the remote agent: it attaches to every
  browsing context including out-of-process widget frames, walks closed shadow roots,
  dispatches trusted pointer and key events through the widget's own event path, and
  observes the vendor token appearing in a field or a cookie. Nothing is reported as
  solved on the strength of a click.
- `LURIEN_CHALLENGE` carries the catalog, the evidence path, the budget, the claimed
  kinds, and the approach path (sampled by `guise`) to the engine. `lurien::challenge`
  owns that contract; the catalog is the same `captcha/kinds/*.toml` table serialized as
  JSON, so the product has exactly one TOML parser.
- `goto` returns an `engine` outcome next to the kind. When the engine reports, it wins
  over the page probe, so a cleared widget is not re-reported as pending.
- A kind is claimed only when the scorecard carries a dated row for it. An unclaimed kind
  is refused with a typed error rather than reported as a pass, and two tests enforce the
  scorecard against the claimed set. Claimed: `none`, `score`, `checkbox`, `pow`, `slider`.
- The `pow` kind is solved in the browser with no helper process. The binding's
  `[work]` table says where the challenge and difficulty live and where the answer
  goes; the engine searches for a nonce in `ChromeWorker` lanes and hands it back by
  typing it through the keyboard path, calling a page callback, or navigating. The
  lane count follows the core count the page itself reads, and a difficulty above
  `pow_max_difficulty` is refused rather than paid for.
- The `slider` kind is measured from the rendered image. `lurien-vision` is a loopback
  helper of a few hundred lines with no model: it finds the puzzle and the cut-out as
  two equal-width pairs of vertical edges and returns one number, in CSS pixels. The
  drag is a profile sampled per solve from the same corpus as the approach path, with
  an overshoot and two corrections, dispatched as individual trusted moves. A binding
  names the puzzle it measures and the handle it drags, both resolved structurally.
- Every act is preceded by a visit. The driver samples a `prelude` plan (settle,
  pointer path across the viewport, wheel session, dwell) from the same persona as
  the fingerprint, and `Prelude.sys.mjs` dispatches it in the top document as
  trusted events before any kind is acted on. Reading is a property of the page,
  not of the cross-origin frame, and a page nobody read scores as a machine however
  trusted the click is. The visit is bounded to a third of the page budget and its
  counts are recorded in the evidence row's `visit` field.
- `guise::human::scroll::HumanScroller::plan` returns the wheel session as data
  (`ScrollStep`), so the browser dispatches a cadence guise owns instead of a
  second scroll signature written next to it.
- A caller-supplied `LURIEN_CHALLENGE` is given a freshly sampled `trajectory`,
  `drag_profile` and `prelude` when it names none. Without this the engine fell
  back to a built-in constant, so every session moved identically.
- Evidence carries a `taken` row when a page's pipeline starts. A cross-origin widget
  is invisible to the page probe, so `goto` used to see a clean page and end the
  session mid-solve; it now waits for the verdict. A diagnostic row is never read as
  one.

### Launch contract

- First public face: `lurien::Browser::launch`, `lurien` CLI, `lurien-mcp`.
- Engine required (`LURIEN_BIN` or `~/.local/share/lurien/lurien`). No Firefox fallback.
- Profile import copies `cookies.sqlite`, `logins.json`+`key4.db`, and localStorage.
- MCP verbs: goto, snapshot, click, type, fill, screenshot, cookies, url, scroll, wait, frames, as.
- No `challenge` tool. Captcha is a property of `goto`. v1 claims `score` only.
- `goto` waits up to 8s for a Turnstile token. An iframe without a token is `score-pending`, not checkbox.
- Unreachable proxy is a TCP probe before spawn. No direct fallback.
- `none` is held 2s so a late Turnstile widget can appear before classify claims none.
- `check_engine` requires `--version` to name Firefox/Camoufox/lurien. `/usr/bin/true` is refused.
- Launch wrapper exports `LURIEN_CONFIG`, `REYNARD_CONFIG`, and `CAMOU_CONFIG` so the June engine applies persona geometry.
- Headful launch treats empty or whitespace `DISPLAY` as unset. `DISPLAY=` no longer hangs 30s.
