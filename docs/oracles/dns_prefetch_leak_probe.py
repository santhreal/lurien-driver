#!/usr/bin/env python3
"""X051 live oracle: prove `network.dns.disablePrefetch` stops the DNS-prefetch leak.

Serves a local page carrying a single `<link rel="dns-prefetch">` to a UNIQUE
domain, captures UDP/53 with tcpdump, and launches headless Firefox twice:

  BASELINE (prefetch ON, Firefox default) -> the unique domain IS queried on :53
                                             (the leak: a clear-text DNS query
                                             escapes the host for a link the user
                                             never clicked).
  FIXED    (network.dns.disablePrefetch=true, what proxy_prefs now emits)
                                          -> the unique domain is NOT queried.

Run on a host with passwordless sudo + tcpdump + firefox on PATH.
"""
import http.server, socketserver, threading, subprocess, tempfile, time, os, sys, secrets, signal

FIREFOX = "/usr/local/bin/firefox" if os.path.exists("/usr/local/bin/firefox") else "firefox"
UNIQ = f"dnsleak-probe-{secrets.token_hex(4)}.example.com"
PAGE = f"""<!doctype html><html><head>
<link rel="dns-prefetch" href="//{UNIQ}">
</head><body>prefetch probe</body></html>"""

class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        body = PAGE.encode()
        self.send_response(200); self.send_header("Content-Type","text/html")
        self.send_header("Content-Length",str(len(body))); self.end_headers()
        self.wfile.write(body)
    def log_message(self, *a): pass

def serve():
    httpd = socketserver.TCPServer(("127.0.0.1", 0), H)
    port = httpd.server_address[1]
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    return httpd, port

def run_arm(label, disable_prefetch, port):
    prof = tempfile.mkdtemp(prefix=f"dnsleak-{label}-")
    # Minimal stealth-irrelevant noise reduction so the only :53 query for UNIQ
    # is the prefetch; everything else is filtered by grepping for UNIQ anyway.
    userjs = [
        f'user_pref("network.dns.disablePrefetch", {"true" if disable_prefetch else "false"});',
        f'user_pref("network.dns.disablePrefetchFromHTTPS", {"true" if disable_prefetch else "false"});',
        'user_pref("browser.safebrowsing.malware.enabled", false);',
        'user_pref("browser.safebrowsing.phishing.enabled", false);',
        'user_pref("network.captive-portal-service.enabled", false);',
        'user_pref("network.connectivity-service.enabled", false);',
        'user_pref("toolkit.telemetry.enabled", false);',
        'user_pref("datareporting.healthreport.uploadEnabled", false);',
    ]
    open(os.path.join(prof, "user.js"), "w").write("\n".join(userjs) + "\n")

    cap = tempfile.NamedTemporaryFile(prefix=f"dnscap-{label}-", suffix=".txt", delete=False)
    # -l line-buffered, -n no name resolution; capture on all interfaces.
    td = subprocess.Popen(["sudo", "tcpdump", "-n", "-l", "-i", "any", "udp", "port", "53"],
                          stdout=cap, stderr=subprocess.DEVNULL)
    time.sleep(1.5)  # let tcpdump attach

    env = dict(os.environ, DISPLAY=os.environ.get("DISPLAY", ":1"))
    ff = subprocess.Popen([FIREFOX, "--headless", "--no-remote", "--profile", prof,
                           f"http://127.0.0.1:{port}/"],
                          env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(6)  # allow parse + prefetch
    ff.terminate()
    try: ff.wait(timeout=5)
    except Exception: ff.kill()
    time.sleep(1)
    subprocess.run(["sudo", "kill", str(td.pid)], stderr=subprocess.DEVNULL)
    td.wait()
    cap.flush(); cap.close()
    data = open(cap.name).read()
    leaked = UNIQ.split(".")[0] in data  # the unique label appears in a DNS query
    return leaked, cap.name

def main():
    httpd, port = serve()
    print(f"probe domain: {UNIQ}  page: http://127.0.0.1:{port}/")
    base_leak, base_cap = run_arm("baseline", False, port)
    fix_leak, fix_cap = run_arm("fixed", True, port)
    print(f"BASELINE (prefetch on):  unique domain queried on :53 = {base_leak}  ({base_cap})")
    print(f"FIXED    (disablePrefetch): unique domain queried on :53 = {fix_leak}  ({fix_cap})")
    if base_leak and not fix_leak:
        print("RESULT: PASS, prefetch leaks by default; disablePrefetch closes it.")
        sys.exit(0)
    elif not base_leak:
        print("RESULT: INCONCLUSIVE, baseline did not leak (prefetch may not have fired; "
              "DNS cached, or headless suppressed it). Re-run or widen the wait.")
        sys.exit(2)
    else:
        print("RESULT: FAIL, disablePrefetch did NOT stop the query.")
        sys.exit(1)

if __name__ == "__main__":
    main()
