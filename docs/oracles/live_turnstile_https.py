#!/usr/bin/env python3
"""Does the Turnstile interactive challenge iframe attach over HTTPS (vs the
http://localhost runs where it never did)? Serve the force-interactive sitekey
over self-signed HTTPS, load with selenium (acceptInsecureCerts), and observe."""
import json, threading, http.server, socketserver, ssl, time, sys
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

srv = socketserver.TCPServer(("127.0.0.1",0), H)
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.load_cert_chain("/tmp/ts_cert.pem", "/tmp/ts_key.pem")
srv.socket = ctx.wrap_socket(srv.socket, server_side=True)
port = srv.server_address[1]
threading.Thread(target=srv.serve_forever, daemon=True).start()

opts = Options(); opts.add_argument("-headless"); opts.binary_location="/usr/local/bin/firefox"
opts.accept_insecure_certs = True
d = webdriver.Firefox(options=opts, service=Service(executable_path="/tmp/geckodriver"))
PROBE = """
const ifr=[...document.querySelectorAll('iframe')].map(f=>({src:(f.src||'').slice(0,80),w:Math.round(f.getBoundingClientRect().width),h:Math.round(f.getBoundingClientRect().height)}));
const w=document.querySelector('.cf-turnstile');
return JSON.stringify({
  total_iframes: ifr.length, iframes: ifr,
  cf_iframe: !!document.querySelector('iframe[src*="challenges.cloudflare.com"]'),
  solver_sel: !!document.querySelector('iframe[src*="challenges.cloudflare.com/cdn-cgi/challenge-platform"]'),
  widget_children: w?[...w.children].map(c=>c.tagName):null,
  widget_text: w?w.textContent.slice(0,80):null,
  response: (document.querySelector('input[name="cf-turnstile-response"]')||{}).value?'populated':'empty',
  callback_fired: window.__ts.cb,
});
"""
try:
    d.get(f"https://127.0.0.1:{port}/")
    print(f"== sitekey {SITEKEY} over HTTPS (selenium) ==")
    last=None
    for t in range(0,14):
        time.sleep(1)
        last=json.loads(d.execute_script(PROBE))
        if last["cf_iframe"] or last["callback_fired"]:
            print(f"[t={t+1}s] cf_iframe={last['cf_iframe']} solver_sel={last['solver_sel']} cb={last['callback_fired']}")
            break
    print(json.dumps(last, indent=2))
finally:
    d.quit(); srv.shutdown()
