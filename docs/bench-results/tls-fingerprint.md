# Layer-2 Wire Fingerprint: reynard vs stock Firefox 150.0.2

Run: 2026-06-11 (differential vs stock) · reynard re-capture 2026-06-12 (post-cipher-fix).
`cargo test -p guise --features browser --test tls_fingerprint`.
Endpoint: `tls.peet.ws/api/all` (same-origin fetch from a driven page). Persona: FirefoxLinux.

## Result: reynard presents a real Firefox-150 shape at the TLS + HTTP/2 layer

The current measured reynard shape (post-cipher-fix, **17 ciphers** including
`ecdhe_ecdsa_aes_128_sha` / 0xc009). The degreased cipher/extension/H2 **sets** were confirmed
`== stock FF-150` in the 2026-06-11 cipher-fix verification run (exact-hash equality is the wrong
invariant: Firefox's GREASE makes a real browser's own JA3 vary run-to-run; the contract is
degreased ordered-set equality). The hashes below are the 2026-06-12 reynard re-capture and are
recorded in `tls_fingerprint.json` + the `firefox-150-linux` catalogue entry.

| Surface | reynard (2026-06-12, post-fix) | stock FF-150 | Match |
|---|---|---|---|
| JA3 hash | `0e76c7e9d06fa0e211b1827687dd8f43` | (degreased set ==, 2026-06-11) | ✅ sets |
| JA4 | `t13d1717h2_5b57614c22b0_e6dcd7ae0a9e` | shares cipher-hash w/ FF-131 | ✅ |
| HTTP/2 Akamai | `1:65536;2:0;4:131072;5:16384\|12517377\|0\|m,p,a,s` | same | ✅ |
| peetprint hash | `8cc4ac50284a435c7250c5f193aea2f1` | (composite) | ✅ sets |
| User-Agent | `…rv:150.0) Gecko/20100101 Firefox/150.0` | (driven as persona) | |

**Cipher-fix correction (the 16→17 story).** An earlier capture recorded a 16-cipher
`t13d1616h2` shape and read it as `== stock`. A careful stock diff then exposed the real tell:
release FF-150 ships 17 ciphers, but the reynard engine (built from `150.0.2-beta.25`) defaulted
`security.ssl3.ecdhe_ecdsa_aes_128_sha` to `false` in `StaticPrefList.yaml`, dropping 0xc009. The
durable fix is a `defaultPref(...)` line in `camoufox.cfg` (runtime autoconfig, no engine rebuild).
This re-capture confirms it is live: 17 ciphers, 0xc009 present, JA4 `t13d1717h2`.

**Why this is the categorical advantage.** reynard spoofs at the Gecko/NSS layer, so its
ClientHello (cipher list, extension order, GREASE, supported groups, ALPN) and its HTTP/2
SETTINGS frame are a *real Firefox's*, not an approximation. JS-patch competitors
(puppeteer/playwright-stealth, patchright) ride Chromium's TLS; proxy-rewrite tools forge one
layer and miss another. None can present a matching JA3 **and** JA4 **and** Akamai H2
**and** peetprint at once.

Closes (measured): G001, G004, G005, G009, G011–G013, G046–G047, R361–R366.
Cluster membership (G048–G051): `fingerprint::cluster` classifies an emitted shape against the
bundled real-browser catalogue (JA4 primary axis, Akamai corroborating), the anti-uniqueness
self-check. A `Distinguishable` verdict is a catalogue-coverage fact, never a uniqueness claim.

## TCP/IP layer (the Botforensics frontier), coherent for Linux personas

```
tcpip: { ip: { ttl: 55, src_ip: 98.179.77.249, ip_version: 4 }, tcp: { window: 63 }, dst_port: 443 }
```

TTL 55 is a Linux base-TTL (64) minus ~9 hops (i.e. the packet layer honestly says "Linux)."
For the **FirefoxLinux** persona this is *coherent*: UA-OS = TLS-OS = TCP-OS = Linux. The whole
stack lines up from the packet up to the painted pixel.

The Botforensics survey's kill-shot ("TLS says Windows, TCP says Linux") therefore applies to
this stack **only when shipping a non-Linux persona**. That precisely scopes the TCP-OS work:

- **G017–G022, X041–X055** become required the moment a Windows/macOS persona ships, the TCP
  stack (TTL 128 for Windows, distinct window/MSS/option-order) must be rewritten to match.
- For Linux personas, no TCP spoof is needed; the only residual is the **real public IP**
  (`98.179.77.249`, no proxy) (an IP/proxy-layer concern (G023, G050), not an engine tell).

## Reproduce

```bash
REYNARD_BIN=$(readlink -f ~/.local/share/reynard/reynard) \
STEALTH_FIREFOX=/tmp/firefox-150/firefox DISPLAY=:1 MOZ_DISABLE_CONTENT_SANDBOX=1 \
STACK_BENCH_DIR=.../bench-results \
  cargo test -p guise --no-default-features --features browser --test tls_fingerprint -- --nocapture
```

## Next (Layer-2)

- Add a Windows persona and measure its `tcpip` block (quantify the Linux-TCP tell (drives G017)).
- Confirm JA3/JA4 land in the *populated* real-FF cluster, not a rare value (G048–G051).
- HTTP/3 (QUIC) transport-param capture (G016) (peet.ws is H2; need a QUIC probe).
