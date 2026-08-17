//! The **Omniscient Page**: a passive, always-on instrumentation grid injected
//! into every page's MAIN world before its scripts run.
//!
//! A human pentester *goes looking* for a DOM-XSS sink, a CSP gap, a postMessage
//! handler. This module makes the page report all of them on its own: every
//! write to a known DOM-XSS sink (with the actual value + a JS stack), every
//! console line, every uncaught exception, every CSP violation, and every
//! inbound `postMessage` is recorded into a bounded buffer the agent reads at
//! will. Active hunting becomes passive telemetry, coverage no human can hold
//! across every page, continuously.
//!
//! The script is injected two ways (see [`crate::Page::start_sensors`]): as a
//! preload (so it runs before page scripts on every navigation) AND evaluated
//! once on the current document (so a page already loaded at launch is covered).
//! It is idempotent, defensive (every hook is wrapped in try/catch and calls the
//! ORIGINAL implementation), and bounded (ring buffers capped), so it never
//! breaks or hangs the page it observes.

/// Per-category ring-buffer cap. A hostile page that spams `console.log` or
/// fires sink writes in a loop cannot grow this without bound (Law 7).
pub const SENSOR_BUFFER_CAP: usize = 300;

/// Max captured length of any single value/code snippet.
pub const SENSOR_SNIPPET_LEN: usize = 512;

/// The sensor install script, an idempotent IIFE so the SAME source is valid
/// both as a preload body and as a one-shot `evaluate` expression.
///
/// Records into a non-enumerable `window.__meridian_signals__` with slices:
/// `sinks` (DOM-XSS), `console`, `errors` (uncaught + rejections), `csp`
/// (violations), `postmessage` (inbound). Each entry carries a short value
/// snippet and, where available, a JS stack so the agent can locate the source.
pub const SENSOR_SCRIPT: &str = r#"(function () {
  try {
    if (window.__meridian_signals__ && window.__meridian_signals__.__installed) return;
    var CAP = 300, SNIP = 512;
    var P = Array.prototype.push, slice = Function.prototype.call.bind(Array.prototype.slice);
    var S = { __installed: true, sinks: [], console: [], errors: [], csp: [], postmessage: [] };
    try { Object.defineProperty(window, "__meridian_signals__", { value: S, writable: true, enumerable: false, configurable: true }); }
    catch (e) { window.__meridian_signals__ = S; }
    var now = function () { try { return Date.now(); } catch (e) { return 0; } };
    var snip = function (v) {
      try {
        var s = typeof v === "string" ? v : (function () { try { return JSON.stringify(v); } catch (e) { return String(v); } })();
        if (s == null) return "";
        return s.length > SNIP ? s.slice(0, SNIP) + "…" : s;
      } catch (e) { return ""; }
    };
    var stack = function () { try { return (new Error().stack || "").split("\n").slice(2, 8).join("\n"); } catch (e) { return ""; } };
    var rec = function (bucket, entry) {
      try { entry.ts = now(); P.call(bucket, entry); if (bucket.length > CAP) bucket.splice(0, bucket.length - CAP); } catch (e) {}
    };

    // ---- DOM-XSS sinks -----------------------------------------------------
    var hookSetter = function (proto, prop, sink) {
      try {
        var d = Object.getOwnPropertyDescriptor(proto, prop);
        if (!d || !d.set) return;
        var orig = d.set;
        Object.defineProperty(proto, prop, {
          configurable: true, enumerable: d.enumerable, get: d.get,
          set: function (val) { rec(S.sinks, { sink: sink, tag: (this && this.tagName) || "", value: snip(val), stack: stack() }); return orig.call(this, val); }
        });
      } catch (e) {}
    };
    hookSetter(Element.prototype, "innerHTML", "innerHTML");
    hookSetter(Element.prototype, "outerHTML", "outerHTML");
    try {
      var iah = Element.prototype.insertAdjacentHTML;
      Element.prototype.insertAdjacentHTML = function (pos, html) { rec(S.sinks, { sink: "insertAdjacentHTML", tag: (this && this.tagName) || "", value: snip(html), stack: stack() }); return iah.apply(this, arguments); };
    } catch (e) {}
    try {
      var dw = document.write;
      document.write = function () { rec(S.sinks, { sink: "document.write", value: snip(slice(arguments).join("")), stack: stack() }); return dw.apply(this, arguments); };
    } catch (e) {}
    try {
      var ev = window.eval;
      window.eval = function (code) { rec(S.sinks, { sink: "eval", value: snip(code), stack: stack() }); return ev.apply(this, arguments); };
    } catch (e) {}
    try {
      var setAttr = Element.prototype.setAttribute;
      Element.prototype.setAttribute = function (name, value) {
        try { var n = ("" + name).toLowerCase(); if (n.indexOf("on") === 0 || ((n === "src" || n === "href") && /^\s*javascript:/i.test("" + value))) rec(S.sinks, { sink: "setAttribute:" + n, tag: (this && this.tagName) || "", value: snip(value), stack: stack() }); } catch (e) {}
        return setAttr.apply(this, arguments);
      };
    } catch (e) {}
    try {
      var sd = Object.getOwnPropertyDescriptor(HTMLScriptElement.prototype, "src");
      if (sd && sd.set) { var so = sd.set; Object.defineProperty(HTMLScriptElement.prototype, "src", { configurable: true, get: sd.get, set: function (u) { rec(S.sinks, { sink: "script.src", value: snip(u), stack: stack() }); return so.call(this, u); } }); }
    } catch (e) {}

    // ---- console -----------------------------------------------------------
    try {
      ["log", "info", "warn", "error", "debug"].forEach(function (level) {
        var orig = console[level];
        if (typeof orig !== "function") return;
        console[level] = function () { try { rec(S.console, { level: level, text: snip(slice(arguments).map(function (a) { return typeof a === "string" ? a : snip(a); }).join(" ")) }); } catch (e) {} return orig.apply(this, arguments); };
      });
    } catch (e) {}

    // ---- uncaught errors + rejections -------------------------------------
    try { window.addEventListener("error", function (e) { rec(S.errors, { kind: "error", message: snip(e && e.message), filename: (e && e.filename) || "", line: (e && e.lineno) || 0, col: (e && e.colno) || 0, stack: snip(e && e.error && e.error.stack) }); }, true); } catch (e) {}
    try { window.addEventListener("unhandledrejection", function (e) { rec(S.errors, { kind: "unhandledrejection", message: snip(e && e.reason && (e.reason.message || e.reason)) }); }, true); } catch (e) {}

    // ---- CSP violations ----------------------------------------------------
    try { document.addEventListener("securitypolicyviolation", function (e) { rec(S.csp, { directive: (e && e.violatedDirective) || "", blocked: (e && e.blockedURI) || "", source: (e && e.sourceFile) || "", line: (e && e.lineNumber) || 0, sample: snip(e && e.sample) }); }, true); } catch (e) {}

    // ---- inbound postMessage ----------------------------------------------
    try { window.addEventListener("message", function (e) { rec(S.postmessage, { origin: (e && e.origin) || "", data: snip(e && e.data) }); }, true); } catch (e) {}
  } catch (e) {}
  return true;
})()"#;

/// Build the reader expression. When `clear` is true the buffer slices are
/// emptied after the snapshot is taken (so the agent can read deltas).
pub fn sensor_reader(clear: bool) -> String {
    format!(
        r#"(function () {{
  var S = window.__meridian_signals__;
  if (!S) return {{ installed: false, sinks: [], console: [], errors: [], csp: [], postmessage: [] }};
  var snap = {{ installed: true,
    sinks: S.sinks.slice(), console: S.console.slice(), errors: S.errors.slice(),
    csp: S.csp.slice(), postmessage: S.postmessage.slice(),
    counts: {{ sinks: S.sinks.length, console: S.console.length, errors: S.errors.length, csp: S.csp.length, postmessage: S.postmessage.length }} }};
  if ({clear}) {{ S.sinks.length = 0; S.console.length = 0; S.errors.length = 0; S.csp.length = 0; S.postmessage.length = 0; }}
  return snap;
}})()"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn balanced(s: &str) -> bool {
        let (mut paren, mut brace, mut bracket) = (0i32, 0i32, 0i32);
        for c in s.chars() {
            match c {
                '(' => paren += 1,
                ')' => paren -= 1,
                '{' => brace += 1,
                '}' => brace -= 1,
                '[' => bracket += 1,
                ']' => bracket -= 1,
                _ => {}
            }
        }
        paren == 0 && brace == 0 && bracket == 0
    }

    #[test]
    fn sensor_script_is_idempotent_iife() {
        assert!(SENSOR_SCRIPT.trim_start().starts_with("(function"));
        assert!(SENSOR_SCRIPT.contains("__meridian_signals__"));
        assert!(SENSOR_SCRIPT.contains("__installed"));
        assert!(
            balanced(SENSOR_SCRIPT),
            "sensor script must be bracket-balanced"
        );
    }

    #[test]
    fn sensor_script_covers_every_sensor_category() {
        // The categorical advantage is breadth (assert each sink/sensor is wired).
        for needle in [
            "innerHTML",
            "outerHTML",
            "insertAdjacentHTML",
            "document.write",
            "window.eval",
            "setAttribute",
            "script.src",
            "console[level]",
            "\"error\"",
            "unhandledrejection",
            "securitypolicyviolation",
            "\"message\"",
        ] {
            assert!(
                SENSOR_SCRIPT.contains(needle),
                "sensor script missing hook: {needle}"
            );
        }
    }

    #[test]
    fn sensor_reader_clear_flag_threads_through() {
        assert!(sensor_reader(true).contains("if (true)"));
        assert!(sensor_reader(false).contains("if (false)"));
        assert!(balanced(&sensor_reader(true)));
        assert!(sensor_reader(true).contains("counts"));
    }

    #[allow(clippy::assertions_on_constants)] // contract canary on the public caps
    #[test]
    fn caps_are_sane() {
        assert!(SENSOR_BUFFER_CAP >= 100);
        assert!(SENSOR_SNIPPET_LEN >= 128);
    }
}
