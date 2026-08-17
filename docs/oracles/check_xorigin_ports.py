#!/usr/bin/env python3
"""Does Firefox treat localhost:P1 -> localhost:P2 iframe as CROSS-ORIGIN
(opaque contentDocument)? That's the precondition for the coordinate-summing
cross-origin click path. If contentDocument is readable, ports alone don't make
a cross-origin boundary and the test needs a different scheme."""
import json, threading, http.server, socketserver
from selenium import webdriver
from selenium.webdriver.firefox.options import Options
from selenium.webdriver.firefox.service import Service

def make_server(body):
    class H(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            b = body
            self.send_response(200); self.send_header("Content-Type","text/html")
            self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)
        def log_message(self,*a): pass
    s = socketserver.TCPServer(("127.0.0.1",0), H)
    threading.Thread(target=s.serve_forever, daemon=True).start()
    return s, s.server_address[1]

# child origin: a page with a clickable box that records isTrusted
child_body = (b"<!doctype html><title>c</title>"
    b"<div id='box' style='position:absolute;left:40px;top:30px;width:50px;height:20px;background:#0a0'></div>"
    b"<script>window.__lastClick=null;document.getElementById('box').addEventListener('click',"
    b"e=>{window.__lastClick={isTrusted:e.isTrusted,x:e.clientX,y:e.clientY};});</script>")
csrv, cport = make_server(child_body)

parent_body = (f"<!doctype html><title>p</title>"
    f"<iframe id='cf' src='http://127.0.0.1:{cport}/' style='position:absolute;left:100px;top:60px;width:300px;height:150px;border:0'></iframe>"
    ).encode()
psrv, pport = make_server(parent_body)

opts = Options(); opts.add_argument("-headless"); opts.binary_location="/usr/local/bin/firefox"
d = webdriver.Firefox(options=opts, service=Service(executable_path="/tmp/geckodriver"))
try:
    d.get(f"http://127.0.0.1:{pport}/")
    import time; time.sleep(1)
    probe = """
      const f = document.getElementById('cf');
      let opaque = false, err = null;
      try { const doc = f.contentDocument; opaque = (doc === null); }
      catch(e){ opaque = true; err = String(e); }
      const r = f.getBoundingClientRect();
      return JSON.stringify({contentDocument_opaque: opaque, err: err,
        iframe_rect: [r.left, r.top, r.width, r.height], frames_len: window.frames.length});
    """
    print(json.dumps(json.loads(d.execute_script(probe)), indent=2))
finally:
    d.quit(); csrv.shutdown(); psrv.shutdown()
