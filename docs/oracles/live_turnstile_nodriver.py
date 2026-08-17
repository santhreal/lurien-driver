#!/usr/bin/env python3
"""Is the Turnstile iframe-no-attach (C001) gated on webdriver detection?
Load the force-interactive sitekey in a NON-automated Firefox (-no-remote, no
marionette, webdriver=false) and have the page report back whether the
challenge iframe attached. Compares against the bare-selenium (webdriver=true)
result where total_iframes=0."""
import json, threading, http.server, socketserver, subprocess, os, sys

SITEKEY = sys.argv[1] if len(sys.argv) > 1 else "3x00000000000000000000FF"
result = {}; done = threading.Event()

PAGE = ("""<!doctype html><html><head><meta charset="utf-8">
<script src="https://challenges.cloudflare.com/turnstile/v0/api.js" async defer></script>
</head><body><form><div class="cf-turnstile" data-sitekey="%s" data-callback="cb"></div></form>
<script>
window.__ts={cb:false,token:null};function cb(t){window.__ts.cb=true;window.__ts.token=t;}
setTimeout(function(){
  const ifr=[...document.querySelectorAll('iframe')].map(f=>({src:(f.src||'').slice(0,80),w:Math.round(f.getBoundingClientRect().width)}));
  const w=document.querySelector('.cf-turnstile');
  navigator.sendBeacon('/report', JSON.stringify({
    webdriver: navigator.webdriver,
    total_iframes: ifr.length, iframes: ifr,
    cf_iframe: !!document.querySelector('iframe[src*="challenges.cloudflare.com"]'),
    widget_children: w?[...w.children].map(c=>c.tagName):null,
    callback_fired: window.__ts.cb,
    token: window.__ts.token?String(window.__ts.token).slice(0,16):null,
  }));
}, 9000);
</script></body></html>""" % SITEKEY).encode()

class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200); self.send_header("Content-Type","text/html")
        self.send_header("Content-Length", str(len(PAGE))); self.end_headers(); self.wfile.write(PAGE)
    def do_POST(self):
        result.update(json.loads(self.rfile.read(int(self.headers.get("Content-Length",0)))))
        self.send_response(204); self.end_headers(); done.set()
    def log_message(self,*a): pass
srv = socketserver.TCPServer(("127.0.0.1",0), H); port = srv.server_address[1]
threading.Thread(target=srv.serve_forever, daemon=True).start()
prof="/tmp/ff_ts_profile"; os.makedirs(prof, exist_ok=True)
p = subprocess.Popen(["/usr/local/bin/firefox","-headless","-no-remote","-profile",prof,
                      f"http://127.0.0.1:{port}/"], env=dict(os.environ, DISPLAY=":1"),
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
ok = done.wait(timeout=40)
try: p.terminate(); p.wait(timeout=10)
except Exception: p.kill()
srv.shutdown()
print(f"== sitekey {SITEKEY} on VANILLA non-webdriver FF-151 ==")
print(json.dumps(result, indent=2) if ok else "TIMEOUT")
