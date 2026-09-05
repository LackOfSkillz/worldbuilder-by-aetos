// Not a test -- a one-off measurement, run manually: `node test/relief-ffi-measurement.mjs`.
// Reports the cost of 66,564 (258^2) individual `elevationM` FFI crossings against one
// `fillTileF32` call over the same grid, on the same world, on this host. Named per the
// figure-reporting rule: population is one 258x258 tile over the DEFAULT_WORLD fixture,
// method is Date.now()-bracketed wall time averaged over 5 repeats after 2 warmup reps,
// host is node v22.17.0 on this machine.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { Engine } from "../public/app/engine.js";
import { marginedTileRequest } from "../public/app/relief.js";

const DEFAULT_WORLD = { seed: 20260904, radiusM: 6371000, plateCount: 12, landFraction: 0.29 };

async function loadEngine() {
  const wasmPath = fileURLToPath(new URL("../public/wasm/worldbuilder_engine.wasm", import.meta.url));
  const bytes = readFileSync(wasmPath);
  const { instance } = await WebAssembly.instantiate(bytes, {});
  return new Engine(instance);
}

function median(xs) {
  const s = [...xs].sort((a, b) => a - b);
  const mid = Math.floor(s.length / 2);
  return s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
}

const engine = await loadEngine();
const world = engine.newWorld(DEFAULT_WORLD);

const rectangle = { northDeg: 10, southDeg: -10, westDeg: 20, eastDeg: 40 }; // land-bearing, includes the witnessed point
const size = 256; // -> a 258x258 request grid, per the brief's "258^2" / 66,564 figure
const request = marginedTileRequest({ rectangle, size, worldHandle: world, radiusM: DEFAULT_WORLD.radiusM });
const { grid, lat0Deg, lat1Deg, lon0Deg, lon1Deg, resolutionM } = request;
const crossings = grid * grid;
console.log(`grid: ${grid} x ${grid} = ${crossings} posts, resolutionM=${resolutionM.toFixed(3)}`);

function runFillTileF32() {
  const t0 = performance.now();
  const heights = engine.fillTileF32(request);
  const t1 = performance.now();
  return { ms: t1 - t0, sample: heights[0] };
}

function runElevationM() {
  const t0 = performance.now();
  let acc = 0; // prevents any hypothetical dead-code elimination; also a checksum
  for (let row = 0; row < grid; row += 1) {
    const lat = lat0Deg + ((lat1Deg - lat0Deg) * row) / (grid - 1);
    for (let col = 0; col < grid; col += 1) {
      const lon = lon0Deg + ((lon1Deg - lon0Deg) * col) / (grid - 1);
      acc += engine.elevationM(world, lat, lon, resolutionM);
    }
  }
  const t1 = performance.now();
  return { ms: t1 - t0, checksum: acc };
}

// 5 warmup reps each (JIT warmup, wasm call-site warmup), then 15 measured reps each --
// bumped up from an initial 2/5 pass because the host showed +-40% swing rep to rep,
// almost certainly V8/GC noise rather than a real difference between the two paths.
for (let i = 0; i < 5; i += 1) { runFillTileF32(); runElevationM(); }

const fillTimes = [];
const elevTimes = [];
for (let i = 0; i < 15; i += 1) {
  fillTimes.push(runFillTileF32().ms);
  elevTimes.push(runElevationM().ms);
}

const fillMed = median(fillTimes);
const elevMed = median(elevTimes);
const fillMin = Math.min(...fillTimes);
const elevMin = Math.min(...elevTimes);

console.log(`fillTileF32  (1 call,      ${crossings} samples): median ${fillMed.toFixed(3)} ms, min ${fillMin.toFixed(3)} ms  [${fillTimes.map((x) => x.toFixed(1)).join(", ")}]`);
console.log(`elevationM   (${crossings} calls, 1 sample each): median ${elevMed.toFixed(3)} ms, min ${elevMin.toFixed(3)} ms  [${elevTimes.map((x) => x.toFixed(1)).join(", ")}]`);
console.log(`ratio (median): elevationM/fillTileF32 = ${(elevMed / fillMed).toFixed(2)}x`);
console.log(`ratio (min):    elevationM/fillTileF32 = ${(elevMin / fillMin).toFixed(2)}x`);
console.log(`per-crossing (median): fillTileF32 ${((fillMed * 1000) / crossings).toFixed(4)} us/post, elevationM ${((elevMed * 1000) / crossings).toFixed(4)} us/post`);

engine.freeWorld(world);
