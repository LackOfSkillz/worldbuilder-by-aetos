// Replay a native corpus through the shipped .wasm and compare bit patterns.
//
// The corpus, its inputs and its native answers all come from `native.txt`, which
// `examples/parity_dump.rs` writes by calling the same `extern "C"` exports this script
// calls. Nothing is recomputed on this side except the wasm answers themselves: every
// f64 is carried as its 16-hex-digit bit pattern, so no decimal text is parsed and the
// comparison is exact.
//
//   node parity.mjs <native.txt> [--wasm <path>] [--mutate seed]
//
// `--mutate seed` is the falsification control: it builds every world with `world_seed + 1`
// and changes nothing else. It must report a large divergent count. A harness that cannot
// be made to fail has not been shown to be able to notice anything.
//
// Exit 0 when divergent === 0 (or, under --mutate, when divergent > 0). Exit 1 otherwise.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const positional = args.filter((a) => !a.startsWith('--'));
const flag = (name) => {
  const i = args.indexOf(`--${name}`);
  return i === -1 ? null : args[i + 1];
};
const dumpPath = positional[0];
if (!dumpPath) {
  console.error('usage: node parity.mjs <native.txt> [--wasm <path>] [--mutate seed]');
  process.exit(2);
}
// The *shipped* artifact by default -- the bytes a browser loads, not a fresh build.
const wasmPath = flag('wasm') ?? resolve(here, '../../../viewer/public/wasm/worldbuilder_engine.wasm');
const mutate = flag('mutate');
if (mutate !== null && mutate !== 'seed') {
  console.error(`unknown mutation "${mutate}"; the only control is --mutate seed`);
  process.exit(2);
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

const lines = readFileSync(dumpPath, 'utf8').split('\n');
const worlds = new Map();
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

const label = mutate ? `CONTROL (--mutate ${mutate})` : 'parity';
console.log(`${label}: ${compared} values compared through the shipped exports, ${divergent} divergent`);
console.log(`artifact: ${wasmPath} (${bytes.length} bytes)`);
for (const [name, g] of groups) console.log(`  ${name}: ${g.compared} compared, ${g.divergent} divergent`);
for (const s of samples) console.log(`  e.g. ${s}`);

if (mutate) {
  if (divergent === 0) {
    console.error('FAIL: the control mutation changed nothing -- this harness cannot notice a divergence');
    process.exit(1);
  }
  console.log('control OK: the harness can be made to fail');
  process.exit(0);
}
if (divergent !== 0) {
  console.error('FAIL: native and WASM disagree');
  process.exit(1);
}
console.log('OK: zero divergent');
