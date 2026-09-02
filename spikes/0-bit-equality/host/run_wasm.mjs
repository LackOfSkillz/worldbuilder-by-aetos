// Runs the WASM build of the probe over the same corpus as the native build, and writes
// the results in the same format. Deliberately dependency-free.

import { readFile, writeFile, mkdir } from "node:fs/promises";

const count = BigInt(process.argv[2] ?? 5_000_000);
const wasmPath = "target/wasm32-unknown-unknown/release/bit_equality_spike.wasm";

const bytes = await readFile(wasmPath);
const { instance } = await WebAssembly.instantiate(bytes, {});
const probeAt = instance.exports.probe_at;
if (typeof probeAt !== "function") {
  throw new Error("probe_at is not exported from the wasm module");
}

// One f64 scratch buffer, reused, so the bit pattern is read without allocation per sample.
const scratch = new DataView(new ArrayBuffer(8));
const lines = [];

for (let i = 0n; i < count; i++) {
  scratch.setFloat64(0, probeAt(i));
  const hi = scratch.getUint32(0).toString(16).padStart(8, "0");
  const lo = scratch.getUint32(4).toString(16).padStart(8, "0");
  lines.push(hi + lo);
}

await mkdir("results", { recursive: true });
await writeFile("results/wasm.bits", lines.join("\n") + "\n");
console.error(`wrote ${count} samples to results/wasm.bits`);
