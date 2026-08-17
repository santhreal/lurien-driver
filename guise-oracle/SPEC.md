# Technical Specification: `guise-oracle`

## 1. Overview & Scope

`guise-oracle` defines the shared surface taxonomy, probe definitions, and differential report data types for the `guise` browser stealth stack.

It lives under `software/browser/` in the Santh monorepo and functions as a zero-dependency contract crate. It deliberately contains no browser drivers, network/TLS stacks, or JavaScript execution runtimes, allowing downstream crates (`lurien`, `sear`, `guise`) to share fingerprint models cheaply.

---

## 2. Data Models & Taxonomy Specification

### 2.1 Severity & Determinism
- `Severity`: Surface drift weight (`Low`, `Medium`, `High`). High severities fail CI gates; Low severities represent cosmetic differences.
- `Determinism`: Surface reproducibility (`Deterministic`, `Stochastic`). Deterministic surfaces agree by raw JSON value; stochastic surfaces (timers, scheduling jitter) agree by classified outcome class label.

### 2.2 Probe & ProbeOutcome
- `ProbeOutcome`: `Pass`, `Drift(reason)`, `Critical(reason)`, or `ProbeError(message)`.
- `Probe`: Uniquely named JS surface check containing expression, severity, determinism, and a classifier predicate mapping raw `serde_json::Value` to `ProbeOutcome`.

### 2.3 Differential & Three-Way Reports
- `Divergence`: Records a surface mismatch between two captured browsers (`a_value` vs `b_value`), classified by `Severity` and `DivergenceKind` (`PersonaIntended` vs `EngineDivergence`).
- `DifferentialReport`: Summary of two-browser surface comparison (`label_a` vs `label_b`). Includes total `surfaces`, `agreed`, `diverged`, `errors`, and sorted `divergences`.
- `DriftReport`: Per-page evaluation summary recording probe pass/drift/critical counts and `per_probe` reports.
- `Capture`: Offline fixture payload containing human `label` and `surfaces` list of `CapturedSurface` values.
- `ThreeWayReport`: Triangulated comparison between stock Firefox, patched lurien engine, and JS disguise layer.

---

## 3. Invariants & Mathematical Guarantees

1. **Exact Percentage Math (No Integer Floor Panic/Rounding)**:
   `DriftReport::is_green()` enforces `critical == 0 && probed > 0 && (passed * 100 >= probed * 90)`. Computation is performed in `u128` arithmetic to eliminate integer division flooring (preventing e.g. 2/3 = 66% from rounding down to pass) and prevent overflow panics when evaluating hostile reports with count values near `usize::MAX`.

2. **Lossless JSON Serialization & Legacy Default**:
   All data types implement `serde::Serialize` and `serde::Deserialize`. Legacy `Divergence` JSON payloads missing the `kind` field default conservatively to `DivergenceKind::EngineDivergence`.

3. **Severity Ordering Invariant**:
   `Severity` ordering is governed strictly by `severity_rank(&Severity) -> u8` (`High=2 > Medium=1 > Low=0`). `Severity` intentionally omits `#[derive(Ord)]` so enum declaration order cannot silently alter report sorting.

4. **Deterministic Offline Diffing**:
   `Capture::diff()` compares two `Capture` payloads offline. It merges keys into a sorted, deduplicated set, evaluates value equality, and orders recorded `Divergence` entries by `severity_rank` descending, then by `surface` name ascending.

5. **Report Consistency Validation**:
   `DifferentialReport::is_consistent()` guarantees `agreed + diverged + errors == surfaces` and `divergences.len() == diverged`. `DriftReport::is_consistent()` guarantees `passed + drift + critical + probe_errors == probed`.

---

## 4. Public API Contract

### Exports (`guise_oracle::*`)
- Types: `Severity`, `Determinism`, `ProbeOutcome`, `Probe`, `DivergenceKind`, `Divergence`, `DifferentialReport`, `DriftReport`, `ProbeReport`, `CapturedSurface`, `Capture`, `ThreeWaySurface`, `ThreeWayReport`.
- Functions: `severity_rank`.

---

## 5. Verification & Test Strategy

- **`tests/unit/`**: Validates struct creation, formatting, `Display`, and consistency validation methods.
- **`tests/property/`**: Proptest suite enforcing JSON roundtrip preservation, invariant filtering, and label stability.
- **`tests/adversarial/`**: Boundary checks covering 90% threshold edge cases, empty reports, critical leak rejection, integer overflow safety, and tampered count payloads.
- **`tests/gap/`**: Pins documented contract gaps (legacy default routing, strict inequality for engine vs JS wins, severity rank ordering authority).
- **`tests/contract/`**: Ensures API surface stability and README / SPEC doc alignment.
