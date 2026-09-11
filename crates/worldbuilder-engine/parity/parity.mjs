// Replay a native corpus through the shipped .wasm and compare bit patterns.
//
// The corpus, its inputs and its native answers all come from `native.txt`, which
// `examples/parity_dump.rs` writes by calling the same `extern "C"` exports this script
// calls. Nothing is recomputed on this side except the wasm answers themselves: every
// f64 is carried as its 16-hex-digit bit pattern, so no decimal text is parsed and the
// comparison is exact.
//
//   node parity.mjs <native.txt> [--wasm <path>] [--mutate seed|erosion-k|water-pond|tectonic-warp|coast-amplitude|gully-steer|climate-samples] [--no-provenance]
//
// `--mutate seed` is the falsification control: it builds every world with `world_seed + 1`
// and changes nothing else. It must report a large divergent count. A harness that cannot
// be made to fail has not been shown to be able to notice anything.
//
// `--mutate erosion-k` is the second control, Task 5's: it bumps `erodibility_per_yr` by
// exactly one ULP before replaying the `R` (erosion) record, and touches nothing else --
// `world`/`E`/`S`/`B`/`T`/`version` records are unaffected, so only the erosion group can
// diverge. The corpus is built so both the recorded native run and this perturbed replay
// hit `max_iterations` without converging (see `examples/parity_dump.rs`'s doc): the step
// count is therefore identical on both sides by construction, and a divergent height is
// evidence of arithmetic sensitivity to `k`, not of the two runs having taken a different
// number of steps to get there. Measured on this exact corpus: 216 of 3,000 erosion-group
// heights move (of 2,843 recomputable ones -- 157 of the 3,000 are roots, held fixed every
// step regardless of `k`, `erosion.rs`'s module doc's "A root has no receiver" -- so 216 of
// 2,843, 7.6%). That is neither "every recomputable height" nor "none of them": most nodes
// don't move because a one-ULP nudge at this corpus's `c` (~1.0e-3) does not reach the last
// mantissa bit of most heights within only 20 steps, not because most nodes are roots. A
// control that diverged on all of them, or none, would say nothing.
//
// `--mutate water-pond` is the third control, Task 5's (slice 5b): it replays the `W`
// (water manifest) record with `pond_max_surface_area_m2` moved from Task 3's calibrated
// 1.0e5 m^2 to 2.0e10 m^2, and touches nothing else. Its claim is narrower than either of the
// other two and is CHECKED RATHER THAN REPORTED.
//
// `pond_max_surface_area_m2` reaches exactly one field of the manifest: `water::classify_lake_kinds`
// compares it against a body's summed surface area and writes `LakeKind`. So under this
// mutation every `root_node`, every `level_m`, all four extent bounds, the body count and the
// datum must compare EQUAL -- the same discipline slice 5a's erosion control kept when it
// required iterations and convergence to compare equal while 216 heights moved.
//
// How many `kind` fields move is not read off this run. `examples/parity_dump.rs` predicts it
// natively from `water::lake_body_surface_areas_m2` -- the summed surface area per physical
// body, a different quantity from the classifier being perturbed -- and writes the prediction
// into the corpus as a `WCTL` record, after asserting that the classifier agrees with the area
// distribution. This script then checks EVERY group's tally against that prediction (the water
// group must move exactly that many, every other group exactly zero) and exits 1 otherwise.
// Measured on this exact corpus: 60 of 156 bodies, i.e. 60 of the water group's 1,095 values
// and 60 of 71,596 overall. Neither none nor all: 2.0e10 m^2 sits near the median of this
// mesh's measured body-surface distribution, chosen for that reason.
//
// PROVENANCE. Before a single value is compared, this script asks the one question the
// comparison itself cannot: *were these bytes built from the source that is here now?*
// Bit-for-bit agreement between `native.txt` and a `.wasm` says nothing if the corpus and
// the artifact are both several commits stale -- they agree with each other perfectly, and
// with current source not at all. That composition was live in this repo: the committed
// artifact predated commit d0c2eff and still printed `OK: zero divergent` while
// `npm run check:wasm` exited 1 on the same tree.
//
// So the staleness guard runs first, and it is *imported* from
// `viewer/scripts/build-wasm.mjs` rather than reimplemented here, because two copies of a
// provenance rule drift and the copy that drifts is the one that stops refusing.
//
// `--wasm <path>` points at an artifact no manifest describes, so provenance cannot be
// established for it; such a run must say so out loud with `--no-provenance`, which
// labels every line of output UNVERIFIED. There is no flag that silences the guard for
// the shipped artifact.
//
// Exit 0 when divergent === 0 (or, under --mutate, when divergent > 0). Exit 1 otherwise.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { checkFreshness, destArtifact } from '../../../viewer/scripts/build-wasm.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const positional = args.filter((a) => !a.startsWith('--'));
const flag = (name) => {
  const i = args.indexOf(`--${name}`);
  return i === -1 ? null : args[i + 1];
};
const dumpPath = positional[0];
if (!dumpPath) {
  console.error('usage: node parity.mjs <native.txt> [--wasm <path>] [--mutate seed|erosion-k|water-pond|tectonic-warp|coast-amplitude|gully-steer|climate-samples] [--no-provenance]');
  process.exit(2);
}
// The *shipped* artifact by default -- the bytes a browser loads, not a fresh build.
const wasmPath = flag('wasm') ?? resolve(here, '../../../viewer/public/wasm/worldbuilder_engine.wasm');
const mutate = flag('mutate');
const MUTATIONS = ['seed', 'erosion-k', 'water-pond', 'tectonic-warp', 'coast-amplitude', 'gully-steer', 'climate-samples'];

/// f64 per gully record, mirroring `wasm.rs`'s `WB_GULLY_STRIDE`. Ten until the second
/// harmonic shipped, twelve since -- and written once here rather than at each of the six
/// call sites, because `wb_gully_check` and `wb_gully_preset` refuse a length they do not
/// recognise while `wb_world_new_gully`'s buffer is allocated on this side: a stale literal at
/// the wrong one of those would truncate the block that builds the world.
const GULLY_STRIDE = 12;
if (mutate !== null && !MUTATIONS.includes(mutate)) {
  console.error(`unknown mutation "${mutate}"; the controls are ${MUTATIONS.map((m) => `--mutate ${m}`).join(', ')}`);
  process.exit(2);
}
const noProvenance = args.includes('--no-provenance');

// ---------------------------------------------------------------- the provenance gate
{
  const shipped = resolve(wasmPath) === resolve(destArtifact);
  if (!shipped) {
    if (!noProvenance) {
      console.error(`REFUSING: --wasm points at ${wasmPath}, which is not the shipped`);
      console.error(`  artifact (${destArtifact}). No manifest describes those bytes, so this`);
      console.error('  script cannot tell whether they were built from the source that is here');
      console.error('  now. Re-run with --no-provenance if you accept an unverified artifact;');
      console.error('  the result then says nothing about what a browser loads.');
      process.exit(1);
    }
    console.warn('WARNING: --no-provenance on a non-shipped artifact. Every figure below is');
    console.warn(`  UNVERIFIED: nothing vouches that ${wasmPath} was built from current source.`);
  } else if (noProvenance) {
    // The one case with no escape hatch. The shipped artifact is the thing this harness
    // exists to make a claim about; a flag that let the claim be made about stale bytes
    // would put the hole straight back.
    console.error('REFUSING: --no-provenance cannot be used on the shipped artifact.');
    console.error('  Provenance is the whole point of running parity against these bytes.');
    process.exit(2);
  } else {
    let problems;
    try {
      problems = checkFreshness();
    } catch (err) {
      // A guard that cannot run has not passed. `toolchainId` throws rather than guess
      // when rustc is missing, and that must surface as a refusal, not as a green run.
      console.error('REFUSING: the staleness guard could not run, so provenance is unknown:');
      console.error(`  ${err.message}`);
      process.exit(1);
    }
    if (problems.length !== 0) {
      console.error('REFUSING TO REPORT PARITY -- STALE ARTIFACT:');
      for (const p of problems) console.error(`  - ${p}`);
      console.error('');
      console.error('  Parity against a stale artifact is the failure this gate exists for: the');
      console.error('  corpus and the .wasm agree with each other and with nothing else. Rebuild');
      console.error('  with `npm run build:wasm` (in viewer/), regenerate native.txt, re-run.');
      process.exit(1);
    }
    console.log('provenance: the shipped .wasm matches its manifest and current source.');
  }
}

const bytes = readFileSync(wasmPath);
const instance = new WebAssembly.Instance(new WebAssembly.Module(bytes), {});
const wb = instance.exports;
const mem = () => new DataView(wb.memory.buffer);

const scratch = new DataView(new ArrayBuffer(8));
const f64of = (hex) => {
  scratch.setBigUint64(0, BigInt('0x' + hex), true);
  return scratch.getFloat64(0, true);
};
const bitsOf = (value) => {
  scratch.setFloat64(0, value, true);
  return scratch.getBigUint64(0, true).toString(16).padStart(16, '0');
};
const bits32Of = (value) => {
  scratch.setFloat32(0, value, true);
  return scratch.getUint32(0, true).toString(16).padStart(8, '0');
};
// One ULP toward +infinity -- the smallest perturbation `--mutate erosion-k` can make to a
// positive finite value, so the control demonstrates bit-level sensitivity rather than
// gross, obviously-different-planet divergence (`--mutate seed`'s own shape).
const bumpUlp = (value) => {
  scratch.setFloat64(0, value, true);
  const bits = scratch.getBigUint64(0, true) + 1n;
  scratch.setBigUint64(0, bits, true);
  return scratch.getFloat64(0, true);
};

const lines = readFileSync(dumpPath, 'utf8').split('\n');
const worlds = new Map();
// `WCTL` carries the water control's threshold AND the number of bodies the native side
// predicts will flip to `Pond` at it -- derived there from `water::lake_body_surface_areas_m2`,
// i.e. from the summed surface areas rather than from the classifier this control perturbs.
// A gate read off the control's own run is a rubber stamp; this one is a prediction made on
// the other side of the boundary and checked here.
let waterControl = null;
// `TWARP` carries the value `--mutate tectonic-warp` writes into word 14 of every `worldt`
// record; `TCTL` carries the per-group counts the native side predicts will move when it
// does. Same discipline as `WCTL`: a control gate read off the control's own run is a rubber
// stamp, so the number is made on the other side of the boundary and checked here.
let tectonicWarp = null;
let tectonicControl = null;
// `CAMP` carries the value `--mutate coast-amplitude` writes into word 0 of every `worldc`
// record -- canonical's own inert amplitude, read from the engine rather than written here --
// and `CCTL` carries the per-group counts the native side predicts will move when it does.
// Same discipline as `WCTL` and `TWARP`/`TCTL`: a control gate read off the control's own run
// is a rubber stamp, so the number is made on the other side of the boundary and checked here.
let coastAmplitude = null;
let coastControl = null;
// `GSTEER` carries the value `--mutate gully-steer` writes into word 9 of every `worldg`
// block, and `GCTL` carries the per-group prediction it must produce. Both are read out of
// the corpus rather than written here, for the reason `TWARP`/`TCTL` are.
let gullySteer = null;
let gullyControl = null;

let compared = 0;
let divergent = 0;
const samples = [];

// Per-group tallies, so a divergent count is never a single unexplained number.
const groups = new Map();
let group = 'none';
const tally = (ok) => {
  compared += 1;
  const g = groups.get(group) ?? { compared: 0, divergent: 0 };
  g.compared += 1;
  if (!ok) g.divergent += 1;
  groups.set(group, g);
};
const note = (what, expected, got) => {
  divergent += 1;
  if (samples.length < 5) samples.push(`${what}: native ${expected} wasm ${got}`);
};

for (const raw of lines) {
  const line = raw.trim();
  if (line === '') continue;
  const f = line.split(' ');
  switch (f[0]) {
    case 'world': {
      // world <name> <seed> <radius_hex> <plates> <land_hex> [<feature f64 hex>...]
      const [, name, seedText, radiusHex, platesText, landHex] = f;
      const seed = BigInt(seedText) + (mutate === 'seed' ? 1n : 0n);
      const records = f.slice(6);
      let ptr = 0;
      if (records.length > 0) {
        ptr = wb.wb_alloc(records.length * 8);
        if (ptr === 0) throw new Error('wb_alloc refused the feature buffer');
        const view = mem();
        records.forEach((hex, i) => view.setBigUint64(ptr + i * 8, BigInt('0x' + hex), true));
      }
      const handle = wb.wb_world_new(
        seed, f64of(radiusHex), Number(platesText), f64of(landHex), ptr, records.length / 8);
      if (handle === 0) throw new Error(`world ${name} did not build in wasm`);
      if (ptr !== 0) wb.wb_dealloc(ptr, records.length * 8);
      worlds.set(name, handle);
      break;
    }
    case 'E': {
      // E <world> <lat> <lon> <res> <value>
      const h = worlds.get(f[1]);
      const got = wb.wb_elevation_m(h, f64of(f[2]), f64of(f[3]), f64of(f[4]));
      group = `elevation/${f[1]}`;
      const ok = bitsOf(got) === f[5];
      tally(ok);
      if (!ok) note(`elevation ${f[1]} ${f[2]},${f[3]} res ${f[4]}`, f[5], bitsOf(got));
      break;
    }
    case 'S': {
      const h = worlds.get(f[1]);
      const got = wb.wb_structural_m(h, f64of(f[2]), f64of(f[3]));
      group = `structural/${f[1]}`;
      const ok = bitsOf(got) === f[4];
      tally(ok);
      if (!ok) note(`structural ${f[1]} ${f[2]},${f[3]}`, f[4], bitsOf(got));
      break;
    }
    case 'B': {
      // B <world> <lat> <lon> <status> <sand> <mud> <rock>
      const h = worlds.get(f[1]);
      const out = wb.wb_alloc(24);
      if (out === 0) throw new Error('wb_alloc refused the bottom buffer');
      const status = wb.wb_bottom_at(h, f64of(f[2]), f64of(f[3]), out);
      const view = mem();
      group = `bottom/${f[1]}`;
      tally(String(status) === f[4]);
      if (String(status) !== f[4]) note(`bottom status ${f[1]} ${f[2]},${f[3]}`, f[4], String(status));
      for (let k = 0; k < 3; k += 1) {
        const got = bitsOf(view.getFloat64(out + k * 8, true));
        tally(got === f[5 + k]);
        if (got !== f[5 + k]) note(`bottom[${k}] ${f[1]} ${f[2]},${f[3]}`, f[5 + k], got);
      }
      wb.wb_dealloc(out, 24);
      break;
    }
    case 'T': {
      // T <world> <lat0> <lat1> <lon0> <lon1> <width> <height> <res> <cell f32 hex>...
      const h = worlds.get(f[1]);
      const width = Number(f[6]);
      const height = Number(f[7]);
      const cells = f.slice(9);
      if (cells.length !== width * height) throw new Error('tile line is the wrong length');
      const out = wb.wb_alloc(width * height * 4);
      if (out === 0) throw new Error('wb_alloc refused the tile buffer');
      const status = wb.wb_fill_tile_f32(
        h, f64of(f[2]), f64of(f[3]), f64of(f[4]), f64of(f[5]),
        width, height, f64of(f[8]), out, width * height);
      if (status !== 0) throw new Error(`wb_fill_tile_f32 returned ${status}`);
      const view = mem();
      group = `tile/${f[1]}`;
      for (let i = 0; i < cells.length; i += 1) {
        const got = bits32Of(view.getFloat32(out + i * 4, true));
        tally(got === cells[i]);
        if (got !== cells[i]) note(`tile ${f[1]}[${i}]`, cells[i], got);
      }
      wb.wb_dealloc(out, width * height * 4);
      break;
    }
    case 'CL': {
      // CL <world> <lat0> <lat1> <lon0> <lon1> <width> <height> <res> <samples> <f32 hex>...
      //
      // The climate raster, two channels per sample: `[temperature_c_at_datum, moisture]`.
      // They are tallied into two SEPARATE groups on purpose. `--mutate climate-samples`
      // adds one upwind step, which changes the rain-out integral and cannot change a
      // closed-form temperature -- so a control that moved both would be a harness fault
      // wearing the costume of a divergence, and only a per-channel tally can say so.
      const h = worlds.get(f[1]);
      const width = Number(f[6]);
      const height = Number(f[7]);
      let samples = Number(f[9]);
      if (mutate === 'climate-samples') samples += 1;
      const cells = f.slice(10);
      const values = width * height * 2;
      if (cells.length !== values) throw new Error('climate tile line is the wrong length');
      const out = wb.wb_alloc(values * 4);
      if (out === 0) throw new Error('wb_alloc refused the climate tile buffer');
      const status = wb.wb_climate_tile_f32(
        h, f64of(f[2]), f64of(f[3]), f64of(f[4]), f64of(f[5]),
        width, height, f64of(f[8]), samples, out, values);
      if (status !== 0) throw new Error(`wb_climate_tile_f32 returned ${status}`);
      const view = mem();
      for (let i = 0; i < cells.length; i += 1) {
        group = (i % 2 === 0) ? `climate-temp/${f[1]}` : `climate-moist/${f[1]}`;
        const got = bits32Of(view.getFloat32(out + i * 4, true));
        tally(got === cells[i]);
        if (got !== cells[i]) note(`climate ${f[1]}[${i}]`, cells[i], got);
      }
      wb.wb_dealloc(out, values * 4);
      break;
    }
    case 'CK': {
      // CK <world> <res> <samples> <f64 hex>...  -- the per-world calibration.
      //
      // Split into two groups for the same reason `CL` is, and here the split is sharper:
      // indices 0..3 are quantiles of the MARCH and indices 4..7 are two quantiles of
      // elevation, a land count and a constant. One more upwind step must move the first
      // four and none of the last four.
      const h = worlds.get(f[1]);
      let samples = Number(f[3]);
      if (mutate === 'climate-samples') samples += 1;
      const cells = f.slice(4);
      const out = wb.wb_alloc(cells.length * 8);
      if (out === 0) throw new Error('wb_alloc refused the calibration buffer');
      const status = wb.wb_climate_calibration(h, f64of(f[2]), samples, out, cells.length);
      if (status !== 0) throw new Error(`wb_climate_calibration returned ${status}`);
      const view = mem();
      for (let i = 0; i < cells.length; i += 1) {
        group = (i < 4) ? `climate-moist/${f[1]}` : `climate-land/${f[1]}`;
        const got = bitsOf(view.getFloat64(out + i * 8, true));
        tally(got === cells[i]);
        if (got !== cells[i]) note(`calibration ${f[1]}[${i}]`, cells[i], got);
      }
      wb.wb_dealloc(out, cells.length * 8);
      break;
    }
    case 'R': {
      // R <name> <seed> <radius_hex> <plates> <land_hex> <node_count> <uplift_hex>
      //   <erodibility_hex> <timestep_hex> <threshold_hex> <max_iterations> <status>
      //   <iterations> <converged> <height f64 hex>...
      //
      // A world is built fresh here (not reused from the `world` records above) because
      // `wb_erosion_run` takes a world handle, and this record carries everything needed
      // to build the exact one `examples/parity_dump.rs` erodes -- no feature records, so
      // `records.length` is always 0 for this world.
      const [
        , name, seedText, radiusHex, platesText, landHex, nodeCountText,
        upliftHex, erodibilityHex, timestepHex, thresholdHex, maxIterText,
        statusText, iterText, convergedText, ...heights
      ] = f;
      const seed = BigInt(seedText) + (mutate === 'seed' ? 1n : 0n);
      const nodeCount = Number(nodeCountText);
      if (heights.length !== nodeCount) throw new Error('erosion line is the wrong length');
      const worldHandle = wb.wb_world_new(seed, f64of(radiusHex), Number(platesText), f64of(landHex), 0, 0);
      if (worldHandle === 0) throw new Error(`erosion world ${name} did not build in wasm`);

      let erodibility = f64of(erodibilityHex);
      if (mutate === 'erosion-k') erodibility = bumpUlp(erodibility);

      const outHeights = wb.wb_alloc(nodeCount * 8);
      const outIterations = wb.wb_alloc(4);
      const outConverged = wb.wb_alloc(4);
      if (outHeights === 0 || outIterations === 0 || outConverged === 0) {
        throw new Error('wb_alloc refused an erosion output buffer');
      }
      const status = wb.wb_erosion_run(
        worldHandle, nodeCount, f64of(upliftHex), erodibility, f64of(timestepHex),
        f64of(thresholdHex), Number(maxIterText), outHeights, nodeCount, outIterations, outConverged);
      const view = mem();
      group = `erosion/${name}`;
      tally(String(status) === statusText);
      if (String(status) !== statusText) note(`erosion status ${name}`, statusText, String(status));
      const gotIterations = view.getUint32(outIterations, true);
      tally(String(gotIterations) === iterText);
      if (String(gotIterations) !== iterText) note(`erosion iterations ${name}`, iterText, String(gotIterations));
      const gotConverged = view.getUint32(outConverged, true);
      tally(String(gotConverged) === convergedText);
      if (String(gotConverged) !== convergedText) note(`erosion converged ${name}`, convergedText, String(gotConverged));
      for (let i = 0; i < nodeCount; i += 1) {
        const got = bitsOf(view.getFloat64(outHeights + i * 8, true));
        tally(got === heights[i]);
        if (got !== heights[i]) note(`erosion height ${name}[${i}]`, heights[i], got);
      }
      wb.wb_dealloc(outHeights, nodeCount * 8);
      wb.wb_dealloc(outIterations, 4);
      wb.wb_dealloc(outConverged, 4);
      wb.wb_world_free(worldHandle);
      break;
    }
    case 'P': {
      // P <selector> <status> <ten f64 hex>
      //
      // `wb_relief_preset` itself, compared field by field. This is the export that exists so
      // that no host ever transcribes a preset's numbers, which makes it the one export whose
      // whole value is that both sides read the SAME ten f64 -- so it is also the one the
      // parity harness has the most business checking. A seed cannot move these, the same way
      // it cannot move `version`, and the seed control's per-group tally says so.
      const selector = Number(f[1]);
      const out = wb.wb_alloc(80);
      if (out === 0) throw new Error('wb_alloc refused the preset buffer');
      const status = wb.wb_relief_preset(selector, out, 10);
      const view = mem();
      group = `preset/${selector}`;
      tally(String(status) === f[2]);
      if (String(status) !== f[2]) note(`preset status ${selector}`, f[2], String(status));
      for (let k = 0; k < 10; k += 1) {
        const got = bitsOf(view.getFloat64(out + k * 8, true));
        tally(got === f[3 + k]);
        if (got !== f[3 + k]) note(`preset ${selector}[${k}]`, f[3 + k], got);
      }
      wb.wb_dealloc(out, 80);
      break;
    }
    case 'worldr': {
      // worldr <name> <seed> <radius_hex> <plates> <land_hex> <ten relief f64 hex>
      //
      // A world through `wb_world_new_relief`, carrying a NON-canonical block. The relief
      // slice's Task 4 changed the export surface for the first time and flagged that parity
      // had not been re-run against it; this record is that re-run. The block travels to the
      // tile workers in the viewer, so the `T hills` record below is the one that exercises
      // the path a worker actually takes.
      const [, name, seedText, radiusHex, platesText, landHex] = f;
      const seed = BigInt(seedText) + (mutate === 'seed' ? 1n : 0n);
      const relief = f.slice(6);
      if (relief.length !== 10) throw new Error('a relief record must be ten f64');
      const ptr = wb.wb_alloc(80);
      if (ptr === 0) throw new Error('wb_alloc refused the relief buffer');
      const view = mem();
      relief.forEach((hex, i) => view.setBigUint64(ptr + i * 8, BigInt('0x' + hex), true));
      const handle = wb.wb_world_new_relief(
        seed, f64of(radiusHex), Number(platesText), f64of(landHex), 0, 0, ptr, 10);
      if (handle === 0) throw new Error(`relief world ${name} did not build in wasm`);
      wb.wb_dealloc(ptr, 80);
      worlds.set(name, handle);
      break;
    }
    case 'WCTL': {
      // WCTL <threshold_hex> <predicted flips>
      //
      // Configuration and prediction, not a compared value: nothing here goes through
      // `tally`. See `waterControl`'s own comment for why the prediction is made natively.
      waterControl = { threshold: f64of(f[1]), predicted: Number(f[2]) };
      break;
    }
    case 'W': {
      // W <world> <node_count> <sea_level_hex> <pond_max_hex> <status> <body_count>
      //   <sea_level_out_hex> <seven f64 hex per body>...
      //
      // The shipped water manifest, through `wb_water_run`. `water.rs` was unreachable from
      // the export surface until that export existed, exactly as `erosion.rs` was until
      // `wb_erosion_run` did -- its native/WASM claim was unfalsifiable, not merely unchecked.
      //
      // The world is one of the `world` records above, by name: unlike the erosion record,
      // this export takes the same handle every other record already samples, so building a
      // second one would be describing a different planet for no reason.
      const h = worlds.get(f[1]);
      const nodeCount = Number(f[2]);
      const bodyCount = Number(f[6]);
      const rows = f.slice(8);
      if (rows.length !== bodyCount * 7) throw new Error('water line is the wrong length');

      let pondMax = f64of(f[4]);
      if (mutate === 'water-pond') {
        if (waterControl === null) throw new Error('--mutate water-pond needs a WCTL record');
        pondMax = waterControl.threshold;
      }

      // `node_count * 7` is always enough: no body holds fewer than one node, so the
      // count-only query is not needed and the resolution is paid for once.
      const capacity = nodeCount * 7;
      const outRows = wb.wb_alloc(capacity * 8);
      const outCount = wb.wb_alloc(4);
      const outSea = wb.wb_alloc(8);
      if (outRows === 0 || outCount === 0 || outSea === 0) {
        throw new Error('wb_alloc refused a water output buffer');
      }
      const status = wb.wb_water_run(
        h, nodeCount, f64of(f[3]), pondMax, outRows, capacity, outCount, outSea);
      const view = mem();
      group = `water/${f[1]}`;
      tally(String(status) === f[5]);
      if (String(status) !== f[5]) note(`water status ${f[1]}`, f[5], String(status));
      const gotCount = view.getUint32(outCount, true);
      tally(String(gotCount) === f[6]);
      if (String(gotCount) !== f[6]) note(`water body count ${f[1]}`, f[6], String(gotCount));
      const gotSea = bitsOf(view.getFloat64(outSea, true));
      tally(gotSea === f[7]);
      if (gotSea !== f[7]) note(`water sea level ${f[1]}`, f[7], gotSea);
      // Rows are compared position by position because both sides sort ascending by
      // `root_node` (`WB_WATER_BODY_STRIDE`'s own doc), so row i is the same body on both
      // sides by construction rather than by luck.
      const fieldNames = ['root_node', 'kind', 'level_m', 'min_lat', 'max_lat', 'min_lon', 'max_lon'];
      for (let i = 0; i < rows.length; i += 1) {
        const got = bitsOf(view.getFloat64(outRows + i * 8, true));
        tally(got === rows[i]);
        if (got !== rows[i]) {
          note(`water ${f[1]} body[${Math.floor(i / 7)}].${fieldNames[i % 7]}`, rows[i], got);
        }
      }
      wb.wb_dealloc(outRows, capacity * 8);
      wb.wb_dealloc(outCount, 4);
      wb.wb_dealloc(outSea, 8);
      break;
    }
    case 'TP': {
      // TP <selector> <status> <sixteen f64 hex>
      //
      // `wb_tectonic_preset` itself, field by field, at both selectors. Same argument as the
      // `P` (relief preset) record above and the same shape: this export exists so that no
      // host ever transcribes a preset, which makes "both sides read the SAME sixteen f64"
      // the whole of its value and therefore the thing a parity harness has most business
      // checking. A world seed cannot move these -- `tectonics.rs`' own constants are all
      // they hand back -- so this group sits at zero under `--mutate seed` exactly as
      // `preset/0` and `version` do.
      const selector = Number(f[1]);
      const out = wb.wb_alloc(16 * 8);
      if (out === 0) throw new Error('wb_alloc refused the tectonic preset buffer');
      const status = wb.wb_tectonic_preset(selector, out, 16);
      const view = mem();
      group = `tpreset/${selector}`;
      tally(String(status) === f[2]);
      if (String(status) !== f[2]) note(`tectonic preset status ${selector}`, f[2], String(status));
      for (let k = 0; k < 16; k += 1) {
        const got = bitsOf(view.getFloat64(out + k * 8, true));
        tally(got === f[3 + k]);
        if (got !== f[3 + k]) note(`tectonic preset ${selector}[${k}]`, f[3 + k], got);
      }
      wb.wb_dealloc(out, 16 * 8);
      break;
    }
    case 'TC': {
      // TC <name> <status> <sixteen f64 hex>
      //
      // `wb_tectonic_check`, the export that answers *why* a record was refused. Only the
      // status is compared, because the status is all it produces -- but a status is exactly
      // where the two sides could part company, since the whole bounds check runs on the
      // decoded block and one of the six records is refused through a SATURATING `as u32`
      // cast on a loop bound. Three of the six are accepted and three refused, asserted on
      // the native side, so a checker stuck at either answer cannot pass this group.
      const record = f.slice(3);
      if (record.length !== 16) throw new Error('a tectonic record must be sixteen f64');
      const ptr = wb.wb_alloc(16 * 8);
      if (ptr === 0) throw new Error('wb_alloc refused the tectonic check buffer');
      const view = mem();
      record.forEach((hex, i) => view.setBigUint64(ptr + i * 8, BigInt('0x' + hex), true));
      const status = wb.wb_tectonic_check(ptr, 16);
      group = 'tcheck';
      tally(String(status) === f[2]);
      if (String(status) !== f[2]) note(`tectonic check ${f[1]}`, f[2], String(status));
      wb.wb_dealloc(ptr, 16 * 8);
      break;
    }
    case 'worldt': {
      // worldt <name> <seed> <radius_hex> <plates> <land_hex> <sixteen tectonic f64 hex>
      //
      // A world through `wb_world_new_tectonic`, carrying a NON-canonical block --
      // `TectonicParams::ranges()`, the one block the viewer's panel reaches with a button,
      // therefore the one most likely to be in flight when a decode differs. Three tasks in
      // a row flagged that no such record existed; this is it.
      //
      // Two records name the same configuration under two names, so the scattered points and
      // the belt points tally separately. That is not redundancy: it is what lets the
      // control's own output say that the belt moved and the rest of the planet did not.
      //
      // `--mutate tectonic-warp` rewrites word 14, `margin_warp_m`, and nothing else.
      const [, name, seedText, radiusHex, platesText, landHex] = f;
      const seed = BigInt(seedText) + (mutate === 'seed' ? 1n : 0n);
      const block = f.slice(6);
      if (block.length !== 16) throw new Error('a tectonic record must be sixteen f64');
      const ptr = wb.wb_alloc(16 * 8);
      if (ptr === 0) throw new Error('wb_alloc refused the tectonic buffer');
      const view = mem();
      block.forEach((hex, i) => view.setBigUint64(ptr + i * 8, BigInt('0x' + hex), true));
      if (mutate === 'tectonic-warp') {
        if (tectonicWarp === null) throw new Error('--mutate tectonic-warp needs a TWARP record');
        view.setFloat64(ptr + 14 * 8, tectonicWarp, true);
      }
      const handle = wb.wb_world_new_tectonic(
        seed, f64of(radiusHex), Number(platesText), f64of(landHex), 0, 0, 0, 0, ptr, 16);
      if (handle === 0) throw new Error(`tectonic world ${name} did not build in wasm`);
      wb.wb_dealloc(ptr, 16 * 8);
      worlds.set(name, handle);
      break;
    }
    case 'CP': {
      // CP <selector> <status> <six coast f64 hex>
      //
      // `wb_coast_preset`, the export that exists so no host transcribes a coast default or
      // preset. The panel's slider anchor and its preset button are both this call's answer,
      // so a wasm build whose preset disagreed with the native one would put a different
      // number on the owner's slider than the number the engine actually uses.
      const selector = Number(f[1]);
      const out = wb.wb_alloc(6 * 8);
      if (out === 0) throw new Error('wb_alloc refused the coast preset buffer');
      const status = wb.wb_coast_preset(selector, out, 6);
      group = `coast-preset/${selector}`;
      tally(String(status) === f[2]);
      if (String(status) !== f[2]) note(`coast preset ${selector} status`, f[2], String(status));
      const view = mem();
      for (let k = 0; k < 6; k += 1) {
        const got = bitsOf(view.getFloat64(out + k * 8, true));
        tally(got === f[3 + k]);
        if (got !== f[3 + k]) note(`coast preset ${selector}[${k}]`, f[3 + k], got);
      }
      wb.wb_dealloc(out, 6 * 8);
      break;
    }
    case 'CC': {
      // CC <name> <status> <six coast f64 hex>
      //
      // `wb_coast_check`, over three records it must accept and three it must refuse. Two of
      // the three refusals are bounds no per-field ceiling can see: a saturating `as u32` on a
      // per-sample loop bound, and a finest-octave frequency that is a PRODUCT of three fields
      // each of which is inside its own domain.
      const record = f.slice(3);
      if (record.length !== 6) throw new Error('a coast record must be six f64');
      const ptr = wb.wb_alloc(6 * 8);
      if (ptr === 0) throw new Error('wb_alloc refused the coast check buffer');
      const view = mem();
      record.forEach((hex, i) => view.setBigUint64(ptr + i * 8, BigInt('0x' + hex), true));
      const status = wb.wb_coast_check(ptr, 6);
      group = 'coast-check';
      tally(String(status) === f[2]);
      if (String(status) !== f[2]) note(`coast check ${f[1]}`, f[2], String(status));
      wb.wb_dealloc(ptr, 6 * 8);
      break;
    }
    case 'worldc': {
      // worldc <name> <seed> <radius_hex> <plates> <land_hex> <six coast f64 hex>
      //
      // A world through `wb_world_new_coast`, carrying a NON-canonical block --
      // `CoastParams::fractal()`, the one block the viewer's panel reaches with a button.
      //
      // Two records name the same configuration under two names, so the scattered points and
      // the shore points tally separately. That is not redundancy: it is what lets the
      // control's own output say how much of the planet the coastal band actually covers.
      //
      // `--mutate coast-amplitude` rewrites word 0, `amplitude`, and nothing else.
      const [, name, seedText, radiusHex, platesText, landHex] = f;
      const seed = BigInt(seedText) + (mutate === 'seed' ? 1n : 0n);
      const block = f.slice(6);
      if (block.length !== 6) throw new Error('a coast record must be six f64');
      const ptr = wb.wb_alloc(6 * 8);
      if (ptr === 0) throw new Error('wb_alloc refused the coast buffer');
      const view = mem();
      block.forEach((hex, i) => view.setBigUint64(ptr + i * 8, BigInt('0x' + hex), true));
      if (mutate === 'coast-amplitude') {
        if (coastAmplitude === null) throw new Error('--mutate coast-amplitude needs a CAMP record');
        view.setFloat64(ptr, coastAmplitude, true);
      }
      const handle = wb.wb_world_new_coast(
        seed, f64of(radiusHex), Number(platesText), f64of(landHex), 0, 0, 0, 0, 0, 0, ptr, 6);
      if (handle === 0) throw new Error(`coast world ${name} did not build in wasm`);
      wb.wb_dealloc(ptr, 6 * 8);
      worlds.set(name, handle);
      break;
    }
    case 'CAMP': {
      // CAMP <amplitude_hex>
      //
      // The control's value for `amplitude` -- canonical's own, read from the engine on the
      // native side -- carried rather than written here, so the one number the mutation
      // substitutes comes from the corpus like every other input. It arrives BEFORE the
      // `worldc` records because that is where it is used. Not a compared value.
      coastAmplitude = f64of(f[1]);
      break;
    }
    case 'CCTL': {
      // CCTL <elevation/fractal> <structural/fractal> <elevation/shore> <structural/shore>
      //      <tile/shore>
      //
      // Prediction, not a compared value: nothing here goes through `tally`. The five counts
      // are computed natively in `examples/parity_dump.rs` -- through the exports AND, as a
      // second derivation, through the library's own `Surface` with a block read from
      // `continentality.rs` rather than from the six words that crossed the boundary -- and
      // this script requires every one of these groups to move exactly the predicted amount
      // and every other group to move zero.
      coastControl = {
        'elevation/fractal': Number(f[1]),
        'structural/fractal': Number(f[2]),
        'elevation/shore': Number(f[3]),
        'structural/shore': Number(f[4]),
        'tile/shore': Number(f[5]),
      };
      break;
    }
    case 'TWARP': {
      // TWARP <warp_hex>
      //
      // The control's value for `margin_warp_m`, carried rather than written here, so the
      // one number the mutation substitutes comes from the corpus like every other input.
      // It arrives BEFORE the `worldt` records because that is where it is used; the
      // prediction it belongs to is `TCTL`, which cannot be written until the corpus has
      // been sampled. Not a compared value.
      tectonicWarp = f64of(f[1]);
      break;
    }
    case 'TCTL': {
      // TCTL <elevation/ranges> <structural/ranges> <elevation/belt> <structural/belt>
      //      <tile/belt> <hydro/ranges>
      //
      // Prediction, not a compared value: nothing here goes through `tally`. The first five
      // counts are computed natively in `examples/parity_dump.rs` -- through the exports AND,
      // as a second derivation, through the library's own `Surface` with blocks read from
      // `tectonics.rs` rather than from the sixteen words that crossed the boundary. The sixth,
      // `hydro/ranges`, is I6's addition (final review of water 1a): the divergence between the
      // forced-outlet `H ranges` record and the same bake on the warp-0 world, under rule (a)'s
      // length-safe accounting. This script requires every one of these groups to move exactly
      // the predicted amount and every other group to move zero.
      tectonicControl = {
        'elevation/ranges': Number(f[1]),
        'structural/ranges': Number(f[2]),
        'elevation/belt': Number(f[3]),
        'structural/belt': Number(f[4]),
        'tile/belt': Number(f[5]),
        'hydro/ranges': Number(f[6]),
      };
      break;
    }
    case 'GP': {
      // GP <selector> <status> <twelve f64 hex>
      //
      // `wb_gully_preset` itself, field by field, at both selectors. Same argument as the `P`
      // (relief) and `CP` (coast) preset records: this export exists so that no host ever
      // transcribes a preset, and on this channel the preset carries `slope_reference` -- the
      // one number in the record that is a MEASUREMENT of this generator rather than a
      // preference. A second copy of it in a panel would be a second answer to "how steep is
      // steep here". So "both sides read the same twelve f64" is the whole of this export's
      // value and therefore the thing a parity harness has most business checking.
      //
      // **TEN f64 until the second harmonic shipped; twelve since.** The stride is written as
      // `GULLY_STRIDE` here rather than as a literal in four places, because the literal is how
      // a widened record gets half-read: `wb_gully_preset` refuses a length it does not
      // recognise, so a stale `10` would have turned this group red rather than silent -- but
      // the `worldg` case below allocates its own buffer, and a stale `10` THERE would have
      // truncated the block that builds the world.
      //
      // A world seed cannot move these, so this group sits at zero under `--mutate seed`.
      const selector = Number(f[1]);
      const out = wb.wb_alloc(GULLY_STRIDE * 8);
      if (out === 0) throw new Error('wb_alloc refused the gully preset buffer');
      const status = wb.wb_gully_preset(selector, out, GULLY_STRIDE);
      const view = mem();
      group = `gpreset/${selector}`;
      tally(String(status) === f[2]);
      if (String(status) !== f[2]) note(`gully preset status ${selector}`, f[2], String(status));
      for (let k = 0; k < GULLY_STRIDE; k += 1) {
        const got = bitsOf(view.getFloat64(out + k * 8, true));
        tally(got === f[3 + k]);
        if (got !== f[3 + k]) note(`gully preset ${selector}[${k}]`, f[3 + k], got);
      }
      wb.wb_dealloc(out, 10 * 8);
      break;
    }
    case 'GC': {
      // GC <name> <status> <twelve f64 hex>
      //
      // `wb_gully_check`, the export that answers *why* a record was refused. Only the status
      // is compared, because the status is all it produces -- but a status is exactly where
      // the two sides could part company, and one of these six records is the crest-height
      // infinity that `WB_MIN_GULLY_SHARPNESS` exists to refuse. Three accepted and three
      // refused, asserted on the native side, so a checker stuck at either answer cannot pass.
      const record = f.slice(3);
      if (record.length !== GULLY_STRIDE) throw new Error('a gully record must be twelve f64');
      const ptr = wb.wb_alloc(GULLY_STRIDE * 8);
      if (ptr === 0) throw new Error('wb_alloc refused the gully check buffer');
      const view = mem();
      record.forEach((hex, i) => view.setBigUint64(ptr + i * 8, BigInt('0x' + hex), true));
      const status = wb.wb_gully_check(ptr, GULLY_STRIDE);
      group = 'gcheck';
      tally(String(status) === f[2]);
      if (String(status) !== f[2]) note(`gully check ${f[1]}`, f[2], String(status));
      wb.wb_dealloc(ptr, 10 * 8);
      break;
    }
    case 'GSTEER': {
      // GSTEER <steer_lattice_m_hex>
      //
      // The control's value for `steer_lattice_m`, carried rather than written here so the one
      // number the mutation substitutes comes from the corpus like every other input. It
      // arrives BEFORE the `worldg` records because that is where it is used; the prediction it
      // belongs to is `GCTL`, which cannot be written until the corpus has been sampled. Not a
      // compared value.
      gullySteer = f64of(f[1]);
      break;
    }
    case 'worldg': {
      // worldg <name> <seed> <radius_hex> <plates> <land_hex> <twelve gully f64 hex>
      //
      // A world through `wb_world_new_gully`, carrying a NON-canonical block --
      // `GullyParams::drainage()`, the measured preset. Two records name the same
      // configuration under two names so the scattered points and the flank points tally
      // separately: that is what lets the control's own output say the flank moved and the
      // rest of the planet did not.
      //
      // `--mutate gully-steer` rewrites word 9, `steer_lattice_m`, and nothing else.
      const [, name, seedText, radiusHex, platesText, landHex] = f;
      const seed = BigInt(seedText) + (mutate === 'seed' ? 1n : 0n);
      const block = f.slice(6);
      if (block.length !== GULLY_STRIDE) throw new Error('a gully record must be twelve f64');
      const ptr = wb.wb_alloc(GULLY_STRIDE * 8);
      if (ptr === 0) throw new Error('wb_alloc refused the gully buffer');
      const view = mem();
      block.forEach((hex, i) => view.setBigUint64(ptr + i * 8, BigInt('0x' + hex), true));
      if (mutate === 'gully-steer') {
        if (gullySteer === null) throw new Error('--mutate gully-steer needs a GSTEER record');
        view.setFloat64(ptr + 9 * 8, gullySteer, true);
      }
      const handle = wb.wb_world_new_gully(
        seed, f64of(radiusHex), Number(platesText), f64of(landHex), 0, 0, 0, 0, 0, 0, 0, 0,
        ptr, GULLY_STRIDE);
      if (handle === 0) throw new Error(`gully world ${name} did not build in wasm`);
      wb.wb_dealloc(ptr, GULLY_STRIDE * 8);
      worlds.set(name, handle);
      break;
    }
    case 'GCTL': {
      // GCTL <elevation/drainage> <structural/drainage> <elevation/flank> <structural/flank>
      //      <tile/flank>
      //
      // Prediction, not a compared value. The five counts are computed natively in
      // `examples/parity_dump.rs` -- through the exports AND, as a second derivation, through
      // the library's own `Surface` with a block read from `detail.rs` rather than from the ten
      // words that crossed the boundary -- and this script requires every one of these groups
      // to move exactly the predicted amount and every other group to move zero.
      //
      // **Two of the five predictions are ZERO on purpose.** The gully term is detail, and
      // `structural_m` is the signal its steering lattice reads. A structural value that moved
      // under this control would mean the term had escaped its layer and was steering on
      // itself.
      gullyControl = {
        'elevation/drainage': Number(f[1]),
        'structural/drainage': Number(f[2]),
        'elevation/flank': Number(f[3]),
        'structural/flank': Number(f[4]),
        'tile/flank': Number(f[5]),
      };
      break;
    }
    case 'H': {
      // H <world> <params_len> <params hex...> <status> <len> <record hex...>
      //
      // Task 11's hydrology bake, through `wb_hydro_bake` / `wb_hydro_len` / `wb_hydro_copy` /
      // `wb_hydro_free` -- the only door onto the hydrology bake across the shipped surface,
      // exactly the shape `wb_erosion_run` and `wb_water_run` are for their own modules.
      //
      // `--mutate seed` reaches this record for free: it is baked on the `plain` world, whose
      // own `world` line already rebuilds with `world_seed + 1` under that mutation, so a
      // different planet underneath the bake is exactly what should move these words.
      const h = worlds.get(f[1]);
      const pl = Number(f[2]);
      const params = f.slice(3, 3 + pl).map(f64of);
      const status = f[3 + pl];
      const len = Number(f[4 + pl]);
      const words = f.slice(5 + pl);
      if (words.length !== len) throw new Error('hydro line is the wrong length');
      const pp = wb.wb_alloc(pl * 8);
      const idp = wb.wb_alloc(4);
      if (pp === 0 || idp === 0) throw new Error('wb_alloc refused a hydro input buffer');
      new Float64Array(wb.memory.buffer, pp, pl).set(params);
      const got = wb.wb_hydro_bake(h, pp, pl, idp);
      group = `hydro/${f[1]}`;
      tally(String(got) === status);
      if (String(got) !== status) note(`hydro status ${f[1]}`, status, String(got));
      const id = mem().getUint32(idp, true);
      const n = wb.wb_hydro_len(id);
      // Final review I6, rule (a). In a plain run `n` and the recorded `len` must be the same
      // length by construction -- a mismatch there means the bake is not reproducible even
      // before a single word is compared, and reading `len` words out of an `n`-word buffer
      // would walk off the copy. Under a control, a mismatch is exactly what some mutations
      // are supposed to produce (`--mutate seed` rebuilds a differently-sized world), so it is
      // tallied like any other divergence instead of refused, and the word loop below never
      // reads past the `n` words `out` actually holds.
      tally(n === len);
      if (n !== len) {
        note(`hydro len ${f[1]}`, String(len), String(n));
        if (!mutate) {
          throw new Error(
            `hydro len ${f[1]}: wb_hydro_len returned ${n}, recorded length was ${len} -- a ` +
            'plain run must reproduce the same length, and comparing past it would read noise');
        }
      }
      const out = wb.wb_alloc(n * 8);
      if (n > 0 && out === 0) throw new Error('wb_alloc refused the hydro output buffer');
      // Final review I6: a copy that did not happen leaves `out` holding whatever the allocator
      // had there, and comparing that is comparing noise. `out` is exactly `n` words, the length
      // `wb_hydro_len` just gave, so anything but WB_OK (0) here is a broken export, not a
      // divergent value -- refuse outright rather than tally.
      const copied = wb.wb_hydro_copy(id, out, n);
      if (copied !== 0) throw new Error(`wb_hydro_copy returned ${copied} for hydro/${f[1]}`);
      const view = mem();
      for (let i = 0; i < len; i += 1) {
        if (i < n) {
          const bits = bitsOf(view.getFloat64(out + i * 8, true));
          tally(bits === words[i]);
          if (bits !== words[i]) note(`hydro word ${i}`, words[i], bits);
        } else {
          // Rule (a): a recorded word past the copy's own length counts as divergent without
          // reading `out` at that index -- `out` is only `n` words long, and this is exactly
          // the over-read the final review caught.
          tally(false);
          note(`hydro word ${i}`, words[i], '<past the copy>');
        }
      }
      wb.wb_hydro_free(id);
      wb.wb_dealloc(out, n * 8);
      wb.wb_dealloc(pp, pl * 8);
      wb.wb_dealloc(idp, 4);
      break;
    }
    case 'version': {
      const got = String(wb.wb_generator_version());
      group = 'version';
      tally(got === f[1]);
      if (got !== f[1]) note('generator version', f[1], got);
      break;
    }
    default:
      throw new Error(`unknown record "${f[0]}"`);
  }
}

for (const handle of worlds.values()) wb.wb_world_free(handle);
if (wb.wb_world_count() !== 0) throw new Error('the harness leaked a world');

const unverified = resolve(wasmPath) !== resolve(destArtifact);
const label = (unverified ? 'UNVERIFIED ' : '') +
  (mutate ? `CONTROL (--mutate ${mutate})` : 'parity');
console.log(`${label}: ${compared} values compared through the shipped exports, ${divergent} divergent`);
console.log(`artifact: ${wasmPath} (${bytes.length} bytes)`);
for (const [name, g] of groups) console.log(`  ${name}: ${g.compared} compared, ${g.divergent} divergent`);
for (const s of samples) console.log(`  e.g. ${s}`);

if (mutate) {
  if (divergent === 0) {
    console.error('FAIL: the control mutation changed nothing -- this harness cannot notice a divergence');
    process.exit(1);
  }
  // THE WATER CONTROL CHECKS ITS OWN PREDICTION, and this is the difference between a control
  // and a shrug. `--mutate seed` is allowed to move nearly everything and `--mutate erosion-k`
  // is allowed to move a fraction nobody can state in advance; this one is not. The native
  // side predicted, from the summed surface areas and NOT from the classifier, exactly how
  // many bodies fall under the control threshold. Every one of those must move here, nothing
  // else in the water group may move, and no other group may move at all -- because
  // `pond_max_surface_area_m2` reaches exactly one field of one export.
  if (mutate === 'water-pond') {
    if (waterControl === null) {
      console.error('FAIL: --mutate water-pond ran with no WCTL record in the corpus');
      process.exit(1);
    }
    let bad = false;
    for (const [name, g] of groups) {
      const isWater = name.startsWith('water/');
      const expected = isWater ? waterControl.predicted : 0;
      if (g.divergent !== expected) {
        console.error(
          `FAIL: group ${name} moved ${g.divergent} values; the native side predicted ${expected}`);
        bad = true;
      }
    }
    if (bad) {
      console.error('  The water control perturbs pond_max_surface_area_m2 by itself, which');
      console.error('  reaches only Body::kind. A count other than the prediction means either');
      console.error('  the two sides classify differently, or that parameter now reaches');
      console.error('  something else -- and either is a finding, not a tolerance to widen.');
      process.exit(1);
    }
    console.log(
      `control OK: ${waterControl.predicted} of ${groups.get('water/plain')?.compared ?? '?'} ` +
      'water values moved, exactly the bodies the native surface-area distribution predicted, ' +
      'and no value outside the water group moved at all');
    process.exit(0);
  }
  // THE TECTONIC CONTROL CHECKS ITS OWN PREDICTION TOO, group by group, and the groups it
  // requires to stay EQUAL are the informative half. `margin_warp_m` is one word of a world's
  // tectonic block: it cannot reach `wb_tectonic_preset` (which hands back `tectonics.rs`'
  // own constants), it cannot reach `wb_tectonic_check` (whose records this mutation does not
  // touch), and it cannot reach any world built without a tectonic block at all. So a run
  // where the belt moved AND something else did is a finding, not a pass.
  if (mutate === 'tectonic-warp') {
    if (tectonicControl === null) {
      console.error('FAIL: --mutate tectonic-warp ran with no TCTL record in the corpus');
      process.exit(1);
    }
    let bad = false;
    for (const [name, g] of groups) {
      const expected = tectonicControl[name] ?? 0;
      if (g.divergent !== expected) {
        console.error(
          `FAIL: group ${name} moved ${g.divergent} values; the native side predicted ${expected}`);
        bad = true;
      }
    }
    if (bad) {
      console.error('  The tectonic control turns margin_warp_m off and touches nothing else.');
      console.error('  It reaches the collision profile of a world built from a tectonic block,');
      console.error('  and nothing else in this corpus -- not the presets, not the checker, not');
      console.error('  a world built without a block. A count other than the prediction means');
      console.error('  either the two sides decode the block differently or that field now');
      console.error('  reaches something it does not name, and either is a finding rather than');
      console.error('  a tolerance to widen.');
      process.exit(1);
    }
    const named = Object.entries(tectonicControl)
      .map(([name, n]) => `${name} ${n}/${groups.get(name)?.compared ?? '?'}`)
      .join(', ');
    console.log(
      `control OK: ${named} moved, exactly as the native side predicted, and every other ` +
      'group -- both tectonic presets, the checker, and every world without a tectonic ' +
      'block -- moved nothing at all');
    process.exit(0);
  }
  // THE GULLY CONTROL CHECKS ITS OWN PREDICTION TOO, group by group, and the groups it
  // requires to stay EQUAL are the informative half -- more so here than on any other channel.
  // `steer_lattice_m` is one word of a world's gully block: it cannot reach `wb_gully_preset`
  // (which hands back `detail.rs`'s own constants), it cannot reach `wb_gully_check` (whose
  // records this mutation does not touch), it cannot reach any world built without a gully
  // block, and -- the claim worth making -- **it cannot reach `structural_m` at all**, because
  // the gully term is detail and `structural_m` is the signal its lattice reads. Two of the
  // five predictions are therefore zero, and they are assertions rather than absences.
  if (mutate === 'gully-steer') {
    if (gullyControl === null) {
      console.error('FAIL: --mutate gully-steer ran with no GCTL record in the corpus');
      process.exit(1);
    }
    let bad = false;
    for (const [name, g] of groups) {
      const expected = gullyControl[name] ?? 0;
      if (g.divergent !== expected) {
        console.error(
          `FAIL: group ${name} moved ${g.divergent} values; the native side predicted ${expected}`);
        bad = true;
      }
    }
    if (bad) {
      console.error('  The gully control moves the steering lattice and touches nothing else.');
      console.error('  It reaches the drainage displacement of a world built from a gully');
      console.error('  block, and nothing else in this corpus -- not the presets, not the');
      console.error('  checker, not a world without a block, and above all not structural_m,');
      console.error('  which is the field the lattice READS. A count other than the prediction');
      console.error('  means either the two sides decode the block differently or that the');
      console.error('  term has escaped detail, and either is a finding rather than a');
      console.error('  tolerance to widen.');
      process.exit(1);
    }
    const named = Object.entries(gullyControl)
      .map(([name, n]) => `${name} ${n}/${groups.get(name)?.compared ?? '?'}`)
      .join(', ');
    console.log(
      `control OK: ${named} moved, exactly as the native side predicted, and every other ` +
      'group -- both gully presets, the checker, every world without a gully block, and ' +
      'every structural value anywhere -- moved nothing at all');
    process.exit(0);
  }
  // THE COAST CONTROL CHECKS ITS OWN PREDICTION TOO, group by group, and the groups it
  // requires to stay EQUAL are the informative half. `amplitude` is one word of a world's
  // coast block: it cannot reach `wb_coast_preset` (which hands back `continentality.rs`' own
  // constants), it cannot reach `wb_coast_check` (whose records this mutation does not touch),
  // and it cannot reach any world built without a coast block at all -- including both
  // tectonic worlds. So a run where the shore moved AND something else did is a finding, not
  // a pass.
  if (mutate === 'coast-amplitude') {
    if (coastControl === null) {
      console.error('FAIL: --mutate coast-amplitude ran with no CCTL record in the corpus');
      process.exit(1);
    }
    let bad = false;
    for (const [name, g] of groups) {
      const expected = coastControl[name] ?? 0;
      if (g.divergent !== expected) {
        console.error(
          `FAIL: group ${name} moved ${g.divergent} values; the native side predicted ${expected}`);
        bad = true;
      }
    }
    if (bad) {
      console.error('  The coast control turns the roughening amplitude back to canonical and');
      console.error('  touches nothing else. It reaches the land/sea field of a world built');
      console.error('  from a coast block, and nothing else in this corpus -- not the presets,');
      console.error('  not the checker, not a world built without a block. A count other than');
      console.error('  the prediction means either the two sides decode the block differently');
      console.error('  or that field now reaches something it does not name, and either is a');
      console.error('  finding rather than a tolerance to widen.');
      process.exit(1);
    }
    const named = Object.entries(coastControl)
      .map(([name, n]) => `${name} ${n}/${groups.get(name)?.compared ?? '?'}`)
      .join(', ');
    console.log(
      `control OK: ${named} moved, exactly as the native side predicted, and every other ` +
      'group -- both coast presets, the checker, and every world without a coast block -- ' +
      'moved nothing at all');
    process.exit(0);
  }
  // THE CLIMATE CONTROL CHECKS A SHAPE RATHER THAN A COUNT, and the shape is the informative
  // half. `--mutate climate-samples` adds ONE upwind step to every climate record. That
  // changes the rain-out integral, so every `climate-moist/*` group must move; it cannot
  // change `climate::temperature_c` (a closed form in latitude, with no march in it) and it
  // cannot change a quantile of elevation, a land count or a lapse rate -- so every
  // `climate-temp/*` and `climate-land/*` group must sit at exactly zero, and so must every
  // group belonging to another channel.
  //
  // The prediction is stated here rather than carried in a record because it is not a
  // number the native side has to compute: it is "all of one kind, none of the others", and
  // a record would only be a second place for it to be written down.
  //
  // The one cell of the corpus that CANNOT move is the zero-budget tile -- a march of no
  // steps returns exactly 1.0 whatever the budget becomes, because the mutation takes it to
  // one step, which does march. So that tile is expected to move too, and the assertion is
  // a lower bound on the moisture groups rather than an equality: what is asserted exactly
  // is the zeros.
  if (mutate === 'climate-samples') {
    let bad = false;
    let moistMoved = 0;
    for (const [name, g] of groups) {
      if (name.startsWith('climate-moist/')) {
        moistMoved += g.divergent;
        if (g.divergent === 0) {
          console.error(`FAIL: group ${name} moved nothing; one more upwind step must change the march`);
          bad = true;
        }
      } else if (g.divergent !== 0) {
        console.error(`FAIL: group ${name} moved ${g.divergent} values; the march budget cannot reach it`);
        bad = true;
      }
    }
    if (moistMoved === 0) bad = true;
    if (bad) {
      console.error('  The climate control adds one step to the upwind march and touches');
      console.error('  nothing else. It reaches the moisture channel and the four moisture');
      console.error('  band edges, and nothing else in this corpus -- not the datum');
      console.error('  temperature, which is a closed form in latitude; not the two landform');
      console.error('  edges, which are quantiles of elevation; not the land count; not the');
      console.error('  lapse rate; and not one value of any other channel. A group that moved');
      console.error('  when it should not have is a finding, not a tolerance to widen.');
      process.exit(1);
    }
    const named = [...groups]
      .filter(([name]) => name.startsWith('climate-'))
      .map(([name, g]) => `${name} ${g.divergent}/${g.compared}`)
      .join(', ');
    console.log(
      `control OK: ${named} -- every moisture group moved, and every temperature group, ` +
      'every landform group and every other channel in the corpus moved nothing at all');
    process.exit(0);
  }
  console.log('control OK: the harness can be made to fail');
  process.exit(0);
}
if (divergent !== 0) {
  console.error('FAIL: native and WASM disagree');
  process.exit(1);
}
console.log(unverified
  ? 'zero divergent -- but against an artifact of unverified provenance'
  : 'OK: zero divergent');
