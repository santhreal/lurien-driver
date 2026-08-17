# Changelog

## 0.1.0 - 2026-08-16

- One verb registry behind three transports. `Session::call(verb, args)` is the only
  entry point; the CLI, `lurien-mcp`, and `lurien serve` generate their surfaces from
  the same `VerbSpec`, so none can offer a verb or argument the others lack.
- 64 verbs across thirteen domains: `page`, `dom`, `frame`, `input`, `state`, `storage`,
  `net`, `intercept`, `dialog`, `observe`, `context`, `session`, `profile`. One verb per
  file; a new verb is that file plus one line in its domain's `SPECS`.
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
- One selector language for every verb that touches an element: CSS, or the
  semantic forms `role:`, `text:`, `label:`, `placeholder:`, `testid:`. A
  semantic form must fit exactly one visible, enabled element and is otherwise
  refused with the candidates named; CSS keeps its first-match contract.
  Resolution answers with a CSS path and mutates nothing in the page.
- Acts wait for their element instead of failing on a page that has not finished
  laying out. The deadline is 10 s, `LURIEN_TIMEOUT_MS` per session, `timeout_ms`
  per call; an invalid selector, an unknown form and an ambiguous description
  fail at once, since waiting cannot change them. `count` never waits.
- An unresolved selector reports what was asked, what the page had, how long it
  was given, and what to do next, listing up to eight elements on screen by role
  and accessible name.
- An element parked off the canvas (`left: -9999px`) is not visible, so it is
  refused rather than clicked at a coordinate outside the viewport. An element
  below the fold stays visible.
- `snapshot` reports the page as an addressable node list by default: role, name,
  state and one handle per node, in document order, capped at 200 nodes with the
  remainder counted rather than hidden. `format=text` and `format=source` keep the
  old representations. Page source cost an agent tokens on markup it could not
  act on and changed on every redesign.
- A snapshot handle is a selector: `ref:e7` acts on the node that line described.
  The handle table lives in the driver, so the page is never tagged, and a handle
  is checked against the role and name it was captured with before it is used. A
  page that re-rendered under a handle earns a refusal naming what changed rather
  than a click on whatever moved into place.
- An HTTP client's `ref` argument resolves. It used to become
  `[data-lurien-ref="7"]`, an attribute nothing in the product wrote, so every
  ref-based call silently matched nothing; `element:7` now means `ref:e7`.
- `batch` runs several verbs in one call and stops at the first failure. Steps
  are parsed and type-checked against their verbs' specs before the first one
  runs, so a typo in step five does not leave the page half filled in, and the
  failure names the step, the verb, what already ran and how many steps were
  skipped. Identical on the CLI, MCP and `lurien serve`.
- HTTP arguments accept a JSON array, not only a string, so `steps` and `files`
  arrive as lists. A string still works and still means what it meant.
- The HTTP face no longer names the verb twice in an error that already opens
  with it.
- Every error class names a corrective action, not only what broke: a chmod for a
  non-executable engine, the persona to use instead of a cross-OS one, the log to
  read after a crash, the scorecard to check for an unclaimed kind. A test builds
  one instance of every variant and fails on a message that only diagnoses, and
  adding a variant stops that test compiling until it is listed.
- `hard captcha` names the kind it refused, which the message previously captured
  and dropped.
- A tool description says when to reach for the verb, what comes back, and whether
  its selector waits. Every sentence is composed from the spec, so a verb is
  described the moment it is registered, and a selector argument that is CSS only
  is not advertised as accepting a description. `lurien help <verb>` shows the
  same text an MCP client reads.
- `lurien serve` sessions have a lifecycle. Every named session carries an age and
  an idle clock, `sessions` (also `list_contexts`) reports both plus whether an
  engine is actually running behind the name, and a session untouched for
  `LURIEN_SESSION_IDLE_MS` (default 900000, `0` disables) is closed. A client that
  dies mid-session used to leak its browser, profile directory and display for the
  life of the server.
- Downloads. `downloads`, `download-wait` and `download-save` work against a
  directory per session, pointed at by prefs written before the first navigation
  so no file goes to the real Downloads folder and no save prompt can hang an
  unattended run. A download counts as finished when its bytes are on disk, not
  when the browser announces it, and a file that never arrives is refused with
  what the page did start. `--download-dir` and a `download_dir` argument name the
  directory when a caller wants a fixed one.
- `choose-files` drives a page that opens the file chooser itself. The chooser is
  armed, the trigger is pressed, and the click that would open the native picker
  has its default action cancelled, so the files reach the input the page meant
  and no dialog is left for nobody to answer. The page's own listeners still run.
- `screenshot` captures the viewport, the whole scrollable document, a rectangle,
  or one element, and takes a `frame` so a cross-origin widget can be pictured
  without the parent page around it. Every area is one browser-side render:
  nothing scrolls and nothing is stitched, so an element below the fold costs no
  page movement and a sticky header appears once. An element is described in the
  same selector language every act verb takes. Every face now reports a capture's
  pixel size next to its byte count.
- Position and permissions. The engine applies a session's position inside the
  process that owns the tab, which is where `navigator.geolocation` reads one, so
  a page that has already asked follows a move without a reload. Every platform
  provider (GeoClue, CoreLocation, gpsd) is off by name and the network provider
  points nowhere, so the host can never answer. The starting position is the
  region the persona's timezone names, so what a page reads cannot contradict the
  clock it reads. `geolocation-set` moves the live session, `geolocation-clear`
  puts the persona back, and `geolocation` reports the position, whether it is an
  override, and whether pages may read it. Permissions are written into the
  profile at launch and denied unless `--allow` names them: Gecko reads them at
  startup, so `permissions` reports the policy and refuses a mid-session change
  with the launch argument that works.
- A privileged control channel between the driver and the engine, in
  `LURIEN_CONTROL`: a loopback socket the engine binds on a port chosen before
  launch, one JSON line in and one out, every request carrying a per-session
  token. It exists for state no client outside a browser can reach, starting with
  a device position, and it stays closed for a session that does not ask.
- The wall clock. `clock-set` takes milliseconds or a time like
  `2033-05-18T03:33:20Z`, `clock-tick` moves it by an interval, `clock-restore`
  gives the host clock back, and `clock` reports what pages read. The shift is
  compiled into the page's own compartment before its first script, so a page
  that reads the date while parsing reads the session's date, and a frame reads
  the same one as its parent whatever process it landed in. `Date.prototype`,
  `Date.name` and the source of `Date.now` are the native ones. Monotonic time,
  pending timers and workers stay on the host clock, deliberately: a shifted
  clock is a reader's view, not a fake event loop.
- Real network interception. `route-fulfil` answers a URL glob from the browser,
  `route-abort` cancels the request, `route-continue` edits its headers, `route`
  reports the table with a count per route, and `route-clear` gives the network
  back. Routes are applied on the channel in the engine's parent process, so a
  fulfilled request never leaves the machine, an abort is a real network error to
  the page, and a header edit reaches the server rather than a page global. The
  table is set whole and the most recently added route is tried first, so a
  caller narrows behaviour by adding a route. This replaces the seven-verb
  `intercept` domain, which wrote to `navigator.__ahuraIntercepts` and
  `navigator.__ahuraHeaders` and changed no request; the old wire names
  (`dom_intercept_request`, `dom_intercept_response`, `dom_set_header`,
  `dom_delete_header`, `dom_set_extra_headers`, `dom_get_headers`,
  `dom_clear_intercepts`) now land on routes.
- `eval` awaits a promise. An expression like `fetch(url)` returns its resolved
  value instead of an opaque handle, so an async probe is one call.
- `har` exports captured traffic as a HAR 1.2 log, to a file or inline. Redaction
  is the same code `net` uses, so an export cannot show what a row hides:
  credential headers, sensitive query values, and every cookie value are gone,
  while the names, the timings, the sizes and the statuses stay. A request body is
  carried only when it is form or json, with credential fields redacted by key at
  any depth; a body this driver cannot read is reported by size and type and not
  exported. Response bodies are not captured, and the export says so instead of
  reporting an empty one.
- A header whose value is a URL now goes through the same query redaction as a
  request URL. `Location` after an OAuth hop carried a one-time code past the
  redaction in `net`.
- Frames have stable handles. `frames` reports `f1`, `f2` and so on next to each
  context, with its url, parent and depth, and every verb that takes a frame
  (`eval`, `click-in`, `type-in`, `screenshot`, `dialog`) accepts one. A handle is
  minted once per context and never reused, so it still names the same frame after
  that frame navigates, where an index shifts and a url substring silently matches
  a different document. A handle whose frame is gone is refused, naming the url it
  had and the verb to run, rather than resolving to whatever is in that slot now.
  The table is refreshed from the browser's own tree, not from a cached context
  list, so a frame the page removed reads as gone.
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
