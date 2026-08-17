#!/usr/bin/env python3
"""Launch FF-151 with NO webdriver/marionette at all -- a vanilla `firefox URL`
process -- and have the page fetch() its surface readout back to us. This is the
ground truth for what a normal user's Firefox exposes (no automation prefs)."""
import json, threading, http.server, socketserver, subprocess, time, os, signal

result = {}
done = threading.Event()

PAGE = b"""<!doctype html><title>p</title><script>
(async () => {
  const r = {
    ua: navigator.userAgent,
    webdriver: navigator.webdriver,
    // catalogue_firefox "absent on Firefox" invariants -- secure-context audit:
    getBattery: typeof navigator.getBattery,
    usb: typeof navigator.usb,
    hid: typeof navigator.hid,
    serial: typeof navigator.serial,
    bluetooth: typeof navigator.bluetooth,
    PaymentRequest: typeof window.PaymentRequest,
    connection: typeof navigator.connection,
    // redteam V8_ONLY_ERROR_MEMBERS audit:
    captureStackTrace: typeof Error.captureStackTrace,
    stackTraceLimit: typeof Error.stackTraceLimit,
    prepareStackTrace: typeof Error.prepareStackTrace,
  };
  navigator.sendBeacon('/report', JSON.stringify(r));
})();
</script>"""

class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200); self.send_header("Content-Type","text/html")
        self.send_header("Content-Length", str(len(PAGE))); self.end_headers(); self.wfile.write(PAGE)
    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(n)
        try: result.update(json.loads(body))
        except Exception as e: result["_err"] = str(e)
        self.send_response(204); self.end_headers()
        done.set()
    def log_message(self,*a): pass

srv = socketserver.TCPServer(("127.0.0.1",0), H); port = srv.server_address[1]
threading.Thread(target=srv.serve_forever, daemon=True).start()

prof = "/tmp/ff_nodriver_profile"
os.makedirs(prof, exist_ok=True)
env = dict(os.environ, DISPLAY=":1")
p = subprocess.Popen(["/usr/local/bin/firefox", "-headless", "-no-remote",
                      "-profile", prof, f"http://127.0.0.1:{port}/"],
                     env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
ok = done.wait(timeout=45)
try: p.terminate(); p.wait(timeout=10)
except Exception: p.kill()
srv.shutdown()
print("== VANILLA FF-151 (no webdriver, -no-remote) ==")
print(json.dumps(result, indent=2) if ok else "TIMEOUT: page never reported back")
