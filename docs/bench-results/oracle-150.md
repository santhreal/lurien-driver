# Differential Oracle: reynard vs stock Firefox 150.0.2

Run: 2026-06-11 · `cargo test -p guise --features browser --test reynard_gate`
Reference: stock Firefox 150.0.2 (`/tmp/firefox-150/firefox`) driven as the persona UA via
`general.useragent.override`. reynard: camoufox-150.0.2-beta.25 over BiDi, FirefoxLinux persona.

## Result (after `locale:all` fix)

**126/131 surfaces agree · 0 errors · 0 High-severity divergence → GATE PASS**

| Surface | reynard | stock | Severity | Verdict |
|---|---|---|---|---|
| `navigator.webdriver` | false | true | High | reynard BETTER, stock=true is a BiDi-driver artifact; excluded (proof: sannysoft pass) |
| automation-framework globals | `[]` | `["navigator.webdriver"]` | High | reynard clean; excluded for the same reason |
| busy-loop timing jitter | 4.359 | 3.000 | Medium | nondeterministic; tracked R088/R170, needs repeated runs to separate signal from noise |
| `history.length` | 0 | 2 | Low | profile-age artifact; tracked R148 |
| `hardwareConcurrency` | 8 | 32 | Low | intended spoof (8 modal vs 32 real-server); working as designed |

## Fixed this run

- **`navigator.languages` length 1 → 2** (was High divergence, now resolved). Root cause:
  the engine derives `intl.accept_languages` (which populates `navigator.languages`) from
  the `locale:all` config key first, falling back to the single `navigator.language`.
  guise set `navigator.language` + a `navigator.languages` array but never `locale:all`,
  so the array was ignored and `navigator.languages` collapsed to `["en-US"]`. Fix: guise
  now emits `locale:all = "en-US, en"`. Regression-locked by
  `reynard::tests::full_language_list_populates_locale_all`.

## How to reproduce

```bash
REYNARD_BIN=$(readlink -f ~/.local/share/reynard/reynard) \
STEALTH_FIREFOX=/tmp/firefox-150/firefox DISPLAY=:1 MOZ_DISABLE_CONTENT_SANDBOX=1 \
  cargo test -p guise --features browser --test reynard_gate -- --nocapture
```

## Next

- Repeated-run study on the timing-jitter surface (R088/R170): is reynard's 4.36 a stable
  engine delta or launch noise? If stable, it is a real timer-precision tell to align.
- Expand the catalogue past 131 surfaces (G183 → 200+); each new surface re-runs here.
