#!/usr/bin/env python3
"""C001/C004 live reproduction: load the REAL Cloudflare Turnstile api.js with the
auto-pass test sitekey (1x00000000000000000000AA) on a local page and observe
whether the cross-origin challenge iframe ATTACHES and the token populates.
Auto-pass sitekey always succeeds, so a missing iframe/token = a real mechanical
bug, not a risk-score block. Bare Firefox first (webdriver=true) as a baseline."""
import json, threading, http.server, socketserver, time
from selenium import webdriver
from selenium.webdriver.firefox.options import Options
from selenium.webdriver.firefox.service import Service

PAGE = b"""<!doctype html><html><head><meta charset="utf-8">
<script src="https://challenges.cloudflare.com/turnstile/v0/api.js" async defer></script>
</head><body>
<form><div class="cf-turnstile" data-sitekey="1x00000000000000000000AA"
  data-callback="cb"></div></form>
<script>window.__ts={cb:false,token:null};function cb(t){window.__ts.cb=true;window.__ts.token=t;}</script>
</body></html>"""

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
return JSON.stringify({
  api_js_loaded: typeof window.turnstile !== 'undefined',
  cf_iframe_present: !!document.querySelector('iframe[src*="challenges.cloudflare.com"]'),
  cf_iframe_src: (document.querySelector('iframe[src*="challenges.cloudflare.com"]')||{}).src || null,
  widget_div_html: (document.querySelector('.cf-turnstile')||{}).innerHTML ? 'has-children' : 'empty',
  response_field: (document.querySelector('input[name="cf-turnstile-response"]')||{}).value || null,
  callback_fired: window.__ts.cb,
  token_prefix: window.__ts.token ? String(window.__ts.token).slice(0,20) : null,
});
"""
try:
    d.get(f"http://127.0.0.1:{port}/")
    for sec in [2,4,6,8,12]:
        time.sleep(sec - (sec-2 if sec==2 else 0) if False else 0)  # noop; explicit sleeps below
    last = None
    for t in range(0, 13):
        time.sleep(1)
        last = json.loads(d.execute_script(PROBE))
        if last["cf_iframe_present"] and (last["callback_fired"] or last["response_field"]):
            print(f"[t={t+1}s] ATTACHED + SOLVED"); break
    print(json.dumps(last, indent=2))
finally:
    d.quit(); srv.shutdown()
