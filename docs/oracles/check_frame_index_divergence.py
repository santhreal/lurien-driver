#!/usr/bin/env python3
"""Does a non-iframe browsing context (<object data=html>) occupy a window.frames
slot, making querySelectorAll('iframe') index DIVERGE from window.frames index?
That's the precondition for the find_element_centre_in_frames offset-lookup bug."""
import json, threading, http.server, socketserver
from selenium import webdriver
from selenium.webdriver.firefox.options import Options
from selenium.webdriver.firefox.service import Service

def make_server(body):
    class H(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            self.send_response(200); self.send_header("Content-Type","text/html")
            self.send_header("Content-Length", str(len(body))); self.end_headers(); self.wfile.write(body)
        def log_message(self,*a): pass
    s = socketserver.TCPServer(("127.0.0.1",0), H)
    threading.Thread(target=s.serve_forever, daemon=True).start()
    return s, s.server_address[1]

child = b"<!doctype html><title>c</title><div id='m'>x</div>"
csrv, cport = make_server(child)
obj = b"<!doctype html><title>o</title><p>obj-content</p>"
osrv, oport = make_server(obj)

parent = (f"<!doctype html><title>p</title>"
  f"<iframe id='a' src='http://127.0.0.1:{cport}/' style='position:absolute;left:10px;top:10px;width:100px;height:80px'></iframe>"
  f"<object id='o' type='text/html' data='http://127.0.0.1:{oport}/' style='position:absolute;left:10px;top:100px;width:100px;height:80px'></object>"
  f"<iframe id='b' src='http://127.0.0.1:{cport}/' style='position:absolute;left:10px;top:200px;width:100px;height:80px'></iframe>"
  ).encode()
psrv, pport = make_server(parent)

opts = Options(); opts.add_argument("-headless"); opts.binary_location="/usr/local/bin/firefox"
d = webdriver.Firefox(options=opts, service=Service(executable_path="/tmp/geckodriver"))
try:
    d.get(f"http://127.0.0.1:{pport}/")
    import time; time.sleep(1.5)
    probe = """
      const iframes = [...document.querySelectorAll('iframe')];
      const qsIdxOfB = iframes.findIndex(f => f.id === 'b');
      let framesIdxOfB = -1, framesLen = window.frames.length;
      const bWin = document.getElementById('b').contentWindow;
      for (let i=0;i<window.frames.length;i++){ if(window.frames[i]===bWin){framesIdxOfB=i;break;} }
      return JSON.stringify({
        querySelectorAll_iframe_index_of_b: qsIdxOfB,
        window_frames_index_of_b: framesIdxOfB,
        window_frames_length: framesLen,
        DIVERGES: qsIdxOfB !== framesIdxOfB
      });
    """
    print(json.dumps(json.loads(d.execute_script(probe)), indent=2))
finally:
    d.quit(); csrv.shutdown(); osrv.shutdown(); psrv.shutdown()
