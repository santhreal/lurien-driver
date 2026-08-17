#!/usr/bin/env python3
"""Systematic oracle: evaluate EVERY Firefox-truth probe's exact JS against a
vanilla, non-automated FF-151 (no webdriver) on a SECURE origin, and report the
raw value + whether the probe's contract would flag it. Any 'VIOLATES' line is
another stale invariant that false-flags a real Firefox."""
import json, threading, http.server, socketserver, subprocess, os

result = {}
done = threading.Event()

# Each entry: (probe name, exact probe JS, contract). Contract is how the Rust
# classifier judges the value (mirrored here to flag real-FF-151 violations).
PROBES = [
    ("navigator.vendor is empty",        "navigator.vendor",                                  "empty_string"),
    ("navigator.userAgent is Firefox",   "navigator.userAgent",                               "firefox_ua"),
    ("navigator.productSub 20100101",    "navigator.productSub === '20100101'",               "true"),
    ("navigator.oscpu nonempty string",  "typeof navigator.oscpu === 'string' && navigator.oscpu.length > 0", "true"),
    ("window.chrome absent",             "(typeof window.chrome === 'undefined') ? null : 'present'", "undefined"),
    ("getBattery absent",                "(typeof Navigator.prototype.getBattery === 'undefined' && typeof navigator.getBattery === 'undefined') ? null : 'present'", "undefined"),
    ("navigator.usb absent",             "(typeof navigator.usb === 'undefined') ? null : 'present'", "undefined"),
    ("navigator.hid absent",             "(typeof navigator.hid === 'undefined') ? null : 'present'", "undefined"),
    ("navigator.bluetooth absent",       "(typeof navigator.bluetooth === 'undefined') ? null : 'present'", "undefined"),
    ("window.PaymentRequest absent",     "(typeof window.PaymentRequest === 'undefined') ? null : 'present'", "undefined"),
    ("navigator.connection absent",      "(typeof navigator.connection === 'undefined') ? null : 'present'", "undefined"),
]

probe_js = ",".join(f"{json.dumps(name)}: (function(){{ try {{ return ({js}); }} catch(e) {{ return '__throw:'+e; }} }})()"
                     for name, js, _ in PROBES)
PAGE = ("<!doctype html><title>p</title><script>(async()=>{const r={" + probe_js +
        "};navigator.sendBeacon('/report',JSON.stringify(r));})();</script>").encode()

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
prof = "/tmp/ff_audit_profile"; os.makedirs(prof, exist_ok=True)
p = subprocess.Popen(["/usr/local/bin/firefox","-headless","-no-remote","-profile",prof,
                      f"http://127.0.0.1:{port}/"], env=dict(os.environ, DISPLAY=":1"),
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
ok = done.wait(timeout=45)
try: p.terminate(); p.wait(timeout=10)
except Exception: p.kill()
srv.shutdown()

if not ok:
    print("TIMEOUT"); raise SystemExit(1)

def violates(contract, val):
    if contract == "empty_string":  return val != ""
    if contract == "firefox_ua":    return not ("Firefox" in str(val) or "Gecko" in str(val))
    if contract == "true":          return val is not True
    if contract == "undefined":     return val is not None
    return False

print("== Firefox-truth probe audit vs vanilla FF-151 (no webdriver) ==")
for name, _, contract in PROBES:
    val = result.get(name, "<missing>")
    flag = "  VIOLATES (stale!)" if violates(contract, val) else "ok"
    print(f"  [{flag:18s}] {name:34s} = {val!r}")
