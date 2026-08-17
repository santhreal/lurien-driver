/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * The pow worker carries its own SHA-256. This checks that digest against a
 * reference implementation, because a hash search that computes the wrong digest
 * still terminates and still reports a nonce, and the page rejects it.
 *
 * Run: node lurien/tests/pow_sha256.mjs
 */

import { createHash, randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(
  join(here, "..", "..", "engine", "additions", "challenge", "PowWorker.js"),
  "utf8"
);

// The worker is a worker: it has no exports. Load its body and hand back the
// pieces under test, which is why this file and not an import.
const load = new Function(
  `${source}\nreturn { sha256, hex, clears, leadingZeroBits, leadingZeroHex };`
);
const worker = load();

function digest(input) {
  return worker.hex(worker.sha256(new TextEncoder().encode(input)));
}

let checked = 0;
const failures = [];

function check(input, label) {
  const mine = digest(input);
  const reference = createHash("sha256").update(input, "utf8").digest("hex");
  checked++;
  if (mine !== reference) {
    failures.push(`${label}: got ${mine} want ${reference}`);
  }
}

// The empty string and the one-block boundary cases, where padding is decided.
check("", "empty");
for (let n = 1; n <= 130; n++) {
  check("a".repeat(n), `${n} bytes`);
}
// Multi-byte input, because the worker hashes what TextEncoder produced.
check("challenge-\u00e9\u4e2d\u{1f600}", "utf8");
// Random input, the shape a live page mints.
for (let i = 0; i < 200; i++) {
  check(randomBytes(24).toString("hex") + i, `random ${i}`);
}

// A hex-zeros difficulty counts hex characters and a bits difficulty counts
// bits. Confusing them is the defect this asserts against.
const zeroWords = new Uint32Array([0x000f_ffff, 0, 0, 0, 0, 0, 0, 0]);
if (worker.leadingZeroBits(zeroWords) !== 12) {
  failures.push(`leadingZeroBits: got ${worker.leadingZeroBits(zeroWords)} want 12`);
}
if (worker.leadingZeroHex(zeroWords) !== 3) {
  failures.push(`leadingZeroHex: got ${worker.leadingZeroHex(zeroWords)} want 3`);
}
if (!worker.clears(zeroWords, "hex-zeros", 3) || worker.clears(zeroWords, "hex-zeros", 4)) {
  failures.push("clears hex-zeros is not an exact leading-zero count");
}
if (!worker.clears(zeroWords, "bits", 12) || worker.clears(zeroWords, "bits", 13)) {
  failures.push("clears bits is not an exact leading-zero-bit count");
}
if (worker.clears(zeroWords, "bits", 5) !== true) {
  failures.push("5 bits should be cleared by 12 leading zero bits");
}
const oneHexZero = new Uint32Array([0x0fff_ffff, 0, 0, 0, 0, 0, 0, 0]);
if (worker.clears(oneHexZero, "bits", 5)) {
  failures.push("one hex zero is 4 bits and must not clear a 5-bit difficulty");
}

if (failures.length) {
  console.error(`FAIL: ${failures.length} of ${checked} digests wrong`);
  for (const line of failures.slice(0, 10)) {
    console.error(`  ${line}`);
  }
  process.exit(1);
}
console.log(`PASS: pow worker SHA-256 matches the reference over ${checked} inputs`);
