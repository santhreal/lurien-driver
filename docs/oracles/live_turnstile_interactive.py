#!/usr/bin/env python3
"""C001/C014: measure what the FORCE-INTERACTIVE Turnstile test sitekey
(3x00000000000000000000FF) actually attaches, and whether the solver's selector
  iframe[src*="challenges.cloudflare.com/cdn-cgi/challenge-platform"]
matches it. Also dump ALL iframes + the widget structure so we know if the
checkbox lives in a cross-origin iframe, a same-origin iframe, or a shadow root."""
import json, threading, http.server, socketserver, time, sys
from selenium import webdriver
from selenium.webdriver.firefox.options import Options
from selenium.webdriver.firefox.service import Service

SITEKEY = sys.argv[1] if len(sys.argv) > 1 else "3x00000000000000000000FF"
PAGE = ("""<!doctype html><html><head><meta charset="utf-8">
<script src="https://challenges.cloudflare.com/turnstile/v0/api.js" async defer></script>
</head><body><form><div class="cf-turnstile" data-sitekey="%s" data-callback="cb"></div></form>
<script>window.__ts={cb:false,token:null};function cb(t){window.__ts.cb=true;window.__ts.token=t;}</script>
</body></html>""" % SITEKEY).encode()

class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200); self.send_header("Content-Type","text/html")
        self.send_header("Content-Length", str(len(PAGE))); self.end_headers(); self.wfile.write(PAGE)
    def log_message(self,*a): pass
srv = socketserver.TCPServer(("127.0.0.1",0), H); port = srv.server_address[1]
threading.Thread(target=srv.serve_forever, daemon=True).start()

opts = Options(); opts.add_argument("-headless"); opts.binary_location="/usr/local/bin/firefox"
d = webdriver.Firefox(options=opts, service=Service(executable_path="/tmp/geckodriver"))
PROBE = """
const iframes = [...document.querySelectorAll('iframe')].map(f => {
  const r = f.getBoundingClientRect();
  return { src: f.src.slice(0,90), w: Math.round(r.width), h: Math.round(r.height) };
});
const widget = document.querySelector('.cf-turnstile');
const SOLVER_SEL = 'iframe[src*="challenges.cloudflare.com/cdn-cgi/challenge-platform"]';
return JSON.stringify({
  total_iframes: iframes.length,
  iframes: iframes,
  solver_selector_matches: !!document.querySelector(SOLVER_SEL),
  widget_has_shadow: widget ? !!widget.shadowRoot : null,
  widget_child_tags: widget ? [...widget.children].map(c=>c.tagName+(c.id?'#'+c.id:'')) : null,
  response_field: (document.querySelector('input[name="cf-turnstile-response"]')||{}).value ? 'populated' : 'empty',
  callback_fired: window.__ts.cb,
});
"""
try:
    d.get(f"http://127.0.0.1:{port}/")
    print(f"== sitekey {SITEKEY} ==")
    last = None
    for t in range(0, 12):
        time.sleep(1)
        last = json.loads(d.execute_script(PROBE))
        if last["solver_selector_matches"] or last["callback_fired"]:
            print(f"[t={t+1}s] selector_match={last['solver_selector_matches']} cb={last['callback_fired']}")
            break
    print(json.dumps(last, indent=2))
finally:
    d.quit(); srv.shutdown()
