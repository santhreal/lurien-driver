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
  scorecard against the claimed set. Claimed: `none`, `score`, `checkbox`.

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
