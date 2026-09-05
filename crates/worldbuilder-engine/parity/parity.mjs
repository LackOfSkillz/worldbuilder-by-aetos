// Replay a native corpus through the shipped .wasm and compare bit patterns.
//
// The corpus, its inputs and its native answers all come from `native.txt`, which
// `examples/parity_dump.rs` writes by calling the same `extern "C"` exports this script
// calls. Nothing is recomputed on this side except the wasm answers themselves: every
// f64 is carried as its 16-hex-digit bit pattern, so no decimal text is parsed and the
// comparison is exact.
//
//   node parity.mjs <native.txt> [--wasm <path>] [--mutate seed|erosion-k|water-pond] [--no-provenance]
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
  console.error('usage: node parity.mjs <native.txt> [--wasm <path>] [--mutate seed|erosion-k|water-pond] [--no-provenance]');
  process.exit(2);
}
// The *shipped* artifact by default -- the bytes a browser loads, not a fresh build.
const wasmPath = flag('wasm') ?? resolve(here, '../../../viewer/public/wasm/worldbuilder_engine.wasm');
const mutate = flag('mutate');
if (mutate !== null && mutate !== 'seed' && mutate !== 'erosion-k' && mutate !== 'water-pond') {
  console.error(`unknown mutation "${mutate}"; the controls are --mutate seed, --mutate erosion-k and --mutate water-pond`);
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
