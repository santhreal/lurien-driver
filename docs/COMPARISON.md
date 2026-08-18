# Axes

What this driver does differently, and the file that proves each one. An axis with
no proof does not belong in this document, and a claim here is a claim a test or a
fixture can be pointed at.

## Where a challenge is cleared

Classification and clearing run inside the browser, in the browsing context that
owns the widget, as chrome-privileged code. A widget served from a second host has
its own process and its own storage, and only code inside it can see whether the
vendor wrote a token. Outside drivers see the top document.

The engine dispatches through the real input path, so `event.isTrusted` is true and
the widget's own listeners run. A synthesized DOM event is not used anywhere in a
solve.

Proof: `lurien/tests/e2e_challenge.sh`, `e2e_slider.sh`, `e2e_pow.sh`,
`e2e_score.sh`, `e2e_token_channels.sh`. Each fixture is cross-origin, refuses an
untrusted event, and records that the parent's `contentDocument` access threw.

## What counts as solved

The vendor writing its own token, in the channel the catalog named: a form field, a
cookie, a storage key in the widget's origin, or a dotted path into a posted
message. A named channel the vendor left empty is a refusal, never a pass.

Proof: `lurien/tests/e2e_token_channels.sh` drives all four channels;
`captcha/kinds/fixtures/challenge_adversarial_child.html` derives its token from a
closure nonce and latches when anything else writes the field.

## What a claim is worth

A kind is claimed only where `docs/bench-results/challenge-scorecard.md` carries a
dated row naming the build that produced it and a script in `lurien/tests/` that
runs it again. An unclaimed kind is refused with a typed error.

Proof: `lurien/tests/kinds_registry.rs` refuses a claim whose row names an older
browser or driver minor version, a row that names no runnable script, and a script
that is not in the tree. `lurien/tests/verb_fail_closed.rs` holds the refusal.

## Whether anything was watching

The browser appends one `started` row before it has seen a page. A driver that
shipped a catalog and finds no such row refuses, because a browser with no observer
in it answers "no challenge here" for every guarded page there is.

Proof: `lurien/tests/e2e_bootstrap.sh`; `lurien/tests/engine_package.rs` holds the
patched hook that starts the subsystem and the row that reports it.

## How an interaction is shaped

Pointer paths, drag profiles, page-reading visits and keystroke timing are sampled
per interaction from a persona model, shipped as decks with a seed, and dealt one
entry per act. The dealt index is recorded in the evidence row, so the claim is
checkable after the fact. No constant delay and no fixed path exists in the solve
path.

Typing carries a gap per pair class and a hold per key class. A page can measure
both, and the fixtures do.

Proof: `lurien/tests/e2e_dynamics.sh`, `e2e_prelude.sh`, `e2e_keys.sh`. The
fixtures refuse an evenly spaced drag, a press with no approach, a visit with no
motion, and a cadence that holds one rate.

## One registry behind every face

The CLI, the MCP server and the HTTP server generate their surfaces from one
`VerbSpec` table, and `Session::call` is the only entry point. A face cannot offer
a verb or an argument the others lack, and `docs/VERBS.md` is generated from the
same table.

Proof: `lurien/tests/verb_registry.rs` (unique tokens, documented lower_snake
arguments, required before optional, no face importing a verb module directly,
`additionalProperties: false` on every schema, and a stale `docs/VERBS.md` fails).
`lurien/tests/serve_protocol.rs` holds the HTTP surface.

## What a failure tells you

Every error class names the corrective action, in words that do not require reading
the source, and shows what it captured: the path, the selector, the kind, the
budget it was given.

Proof: `lurien/src/error.rs` tests `every_error_class_names_a_corrective_action`
and `every_error_shows_what_it_captured`, over a corpus that a new variant must be
added to before the crate compiles.

## What a number means in two repositories

The driver and the browser ship as separate builds. The evidence schema version,
the helper protocol version, the kind severity order, and the share of a page
budget spent reading the page are each one number held equal across both trees by a
test, rather than two numbers that agree today.

Proof: `lurien/tests/engine_package.rs`.

## What the network view shows

Requests, responses, timings and cookies are captured passively for every session,
and one redaction rule covers every view of them: the log, the HAR export, and the
credential report. A route override is applied on the channel in the parent, so a
request that never reaches the network is still reported.

Proof: `lurien/tests/e2e_route.sh`, `e2e_har.sh`.

## What a handle refers to

A frame handle is minted once per context and stays valid across navigations of its
parent. A snapshot handle whose node changed is refused rather than acted on.

Proof: `lurien/tests/e2e_frames.sh`, `e2e_snapshot.sh`.

## What the environment says

Geolocation, the wall clock, permissions and the sensor grid are set where the page
reads them: a clock is a property of a JS compartment, so every frame that gets a
new document gets the same clock, and a position is served by the browser rather
than by a page script.

Proof: `lurien/tests/e2e_geo.sh`, `e2e_clock.sh`.

## Limits

- `audio` is claimed against a fixture that speaks a code and needs a local speech
  model. The shipped `hcaptcha_audio` binding carries no `[audio]` table, because
  what the audio task renders is minted per session, so a live widget is recognized
  and then refused by name. A live row needs a binding that names the control, the
  source, the answer field and the alphabet a real vendor uses.
- `visual` is claimed against a fixture grid and needs a local object detector. The
  shipped vendor bindings for it carry no `[grid]` table, so a live hCaptcha,
  reCAPTCHA or Arkose grid is recognized and then refused by name. The measured
  reason is in the scorecard: the detector answers a live reCAPTCHA crop exactly, but
  a live binding still needs to press the anchor that opens the grid in another
  browsing context and to answer the rounds that follow, and hCaptcha no longer
  renders a tile grid at all.
- Live-vendor rows exist for `none` and `score`. The interactive kinds are proven
  by fixtures, which prove the mechanism and not the arms race.
- Matched-host Linux Firefox only. A cross-OS persona is refused, because fonts,
  WebGL and WebGPU would contradict it.
- One engine family. There is no Chromium path, and there is no fallback to a stock
  browser: a missing engine is an error.
