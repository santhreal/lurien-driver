# Engine

The lurien engine is a Camoufox / Firefox 150 fork. MPL-2.0. Separate
process. Never linked into the MIT crates.

The tree is `software/browser/engine/`.

lurien spoofs at the Gecko C++ layer, so every surface holds natively in
all realms (main thread, Web Workers, cross-origin iframes) with nothing
for a page to `toString`-probe.

It inherits the full Camoufox patch set (`patches/`) and is driven over
WebDriver BiDi (foxdriver), not Juggler. lurien's own deltas:

- `LURIEN_CONFIG` first, then one-release `REYNARD_CONFIG`, then
  upstream `CAMOU_CONFIG`. Additive so older binaries keep working.
- Per-identity device noise and WebRTC masking live in guise / the
  bridge. The engine honors `canvas:seed`, `audio:seed`,
  `fonts:spacing_seed`, and `window.setWebRTCIPv4`.

Bespoke beyond-Camoufox engine patches are later work.

## Build

v1 is Linux x86_64 only.

1. Cargo target dir must sit outside the Santh tree (`~/.cargo/config.toml`).
2. From the engine directory: `make dir && make build`.
3. The native product is still `obj-*/dist/bin/camoufox`.
4. `install.sh` copies or symlinks it to `~/.local/share/lurien/lurien`.

Or set `LURIEN_BIN` to that camoufox binary. `install.sh` does not download
Gecko until a Release tarball exists.

The proprietary OS font bundles under `bundle/fonts/` are not shipped.
Linux matched-host does not need them. Cross-OS personas stay unsupported
in v1.

## Run

```
export LURIEN_BIN=~/.local/share/lurien/lurien
export DISPLAY=:10   # private Xvfb; never a logged-in desktop session
```

`lurien --version` prints crate semver and the engine `--version` string.

## Config

The launch wrapper exports `LURIEN_CONFIG`, then one-release `REYNARD_CONFIG`,
then `CAMOU_CONFIG`, all the same JSON. The June 2026 installed binary reads
`REYNARD_CONFIG` then `CAMOU_CONFIG`. A later engine rebuild reads
`LURIEN_CONFIG` first. Persona JSON is written by guise at launch. Do not
hand-edit a live file.

## Verify

`lurien_gate` plus the IP-independent detectors in guise: `navigator.webdriver`
is false over BiDi, persona UA holds in main and worker realms, sannysoft /
areyouheadless stay clean. Stock Firefox 150 is the oracle only. On this host
it lives at `/mnt/FlareTraining/firefox-150/firefox/firefox`. Set
`STEALTH_FIREFOX` to that path. It is not a product fallback.
