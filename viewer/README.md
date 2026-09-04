# viewer — offline CesiumJS shell

Slice 2b Task 1. This directory holds a vendored CesiumJS and a minimal `Viewer` that
draws nothing but the ellipsoid, plus the harness used to **witness** that the page makes
no request off its own origin.

The spec forbids live connections outright. A phone-home is disqualifying, and every later
task in this slice is built on the assumption there is none. Before this task that
assumption was documentary and source-based. It is now witnessed with a browser network
trace, in both directions.

## What is vendored

| | |
|---|---|
| Package | `cesium` |
| Version | **1.145.0** (pinned in `package.json`; `package-lock.json` carries the sha512 integrity hash) |
| Licence | **Apache-2.0** — `public/vendor/cesium/LICENSE.md`, "Copyright 2011-2026 CesiumJS Contributors" |
| Third-party | `public/vendor/cesium/ThirdParty.json` — 23 entries, all Apache-2.0 / BSD-3-Clause / ISC / MIT |
| Vendored tree | `public/vendor/cesium/` — 395 files, 22,743,829 bytes, copied byte-for-byte from `node_modules/cesium/Build/Cesium` |
| Manifest | `cesium-manifest.txt` — sha256 of every vendored file |

**Cesium ion is a separate product with its own terms.** "CesiumJS is Apache-2.0" and
"Cesium is free offline" are two different sentences and only the first is relied on here.
Nothing in this viewer uses ion; `Ion.defaultAccessToken` is blanked at boot so an
accidental ion call fails loudly instead of quietly succeeding against Cesium's servers.

## How to vendor

```
cd viewer
npm ci            # installs cesium@1.145.0 exactly, per package-lock.json
npm run vendor    # copies Build/Cesium + LICENSE.md + ThirdParty*.json into public/vendor/cesium
git diff --stat   # MUST be empty: the committed tree already matches the pinned version
```

`npm run vendor` also rewrites `cesium-manifest.txt`. A non-empty `git diff` after a clean
`npm ci` means the vendored tree and the lockfile have drifted apart.

`node_modules/` is gitignored; `public/vendor/` is committed, so a fresh checkout can serve
the viewer with **no network at all**. This is the point — do not replace the vendored tree
with a CDN `<script src>`.

Note: the root `.gitignore` uses unanchored diagnostic-render patterns (`land-*.png`,
`patch.*`, `grid-*.png` …) that match at any depth and swallowed one vendored asset,
`Assets/Textures/maki/land-use.png`. `viewer/.gitignore` re-includes `public/vendor/**` to
undo that. If you add vendored trees elsewhere, check `git check-ignore` first.

## How to serve

```
cd viewer
npm run serve     # http://127.0.0.1:8137/   (PORT= to change)
```

`scripts/serve.mjs` is a static file server over `public/` on loopback only. It proxies
nothing and has no upstream, so it is a second, server-side witness: whatever the page
fetches from this origin appears in its stdout log, and whatever is **not** in that log went
somewhere else. It also sets COOP/COEP `require-corp`, so the page is cross-origin isolated
ready for the SharedArrayBuffer worker pool in Task 5.

## What must be set

1. **`window.CESIUM_BASE_URL = "/vendor/cesium/"`, before `Cesium.js` is evaluated.**
   Every worker, widget image and asset resolves through `buildModuleUrl()`, which only ever
   joins against this base. There is no remote fallback in that function.
2. **`baseLayer: false`.** This is *the* one network-live `Viewer` default:
   `ImageryLayer.fromWorldImagery()`. Terrain's default, `EllipsoidTerrainProvider`, is
   computed rather than fetched and is already offline.
3. **`baseLayerPicker: false`, `geocoder: false`.** Both are ion-backed the moment a user
   touches them.
4. `Ion.defaultAccessToken = undefined`.

## What the trace showed

Chrome DevTools network log plus `performance.getEntriesByType("resource")`, cross-checked
against the server's own request log. Cesium 1.145.0, Chrome, `http://127.0.0.1:8137`.

### Offline (default) — `http://127.0.0.1:8137/`

**19 requests, 0 off-origin.** All of them `http://127.0.0.1:8137/…` or `blob:` URLs minted
from that origin (Cesium's workers). Then the camera was flown to five widely separated
points on the globe (Boston 3000 km, London 1500 km, Tokyo 800 km, Rio 400 km, Cape Town
200 km) and left to settle: still **0 off-origin**, and the resource-timing count stayed at
12 (blob URLs are not reported there). No console messages, no 404s in the server log.

```
GET /                                                     200
GET /vendor/cesium/Widgets/widgets.css                    200
GET /vendor/cesium/Cesium.js                              200
GET /vendor/cesium/Assets/approximateTerrainHeights.json  200
GET /vendor/cesium/Assets/IAU2006_XYS/IAU2006_XYS_18.json 200
GET /vendor/cesium/Assets/Images/ion-credit.png           200
GET /vendor/cesium/Assets/Textures/SkyBox/tycho2t3_80_{px,mx,py,my,pz,mz}.jpg  200
GET /vendor/cesium/Assets/Textures/moonSmall.jpg          200
GET blob:http://127.0.0.1:8137/…  x6                      200
```

`ion-credit.png` is Cesium's default credit logo, served from the vendored tree. It is
branding, not a connection — the "Cesium ion" mark in the bottom-left corner is a local
image with an `<a href>` that is never followed.

Verified in-page at the same time: `viewer.imageryLayers.length === 0`,
`viewer.terrainProvider instanceof Cesium.EllipsoidTerrainProvider === true`,
`Cesium.Ion.defaultAccessToken === undefined`.

### Net probe (deliberate break) — `http://127.0.0.1:8137/?net-probe=1`

Restores `baseLayer: ImageryLayer.fromWorldImagery()` and nothing else. **Off-origin
requests immediately**, to six hosts. Two runs measured 65 and 45 off-origin entries; the
count varies with how many tiles the camera pulls, the host set does not:

```
https://api.cesium.com/v1/assets/2/endpoint?access_token=eyJhbGciOi…   x1  (ion, bundled demo JWT)
https://dev.virtualearth.net/REST/v1/Imagery/Metadata/Aerial?…&key=AmXdbd8Ue…   x1
http://ecn.t{0,1,2,3}.tiles.virtualearth.net/tiles/a….jpeg?n=z&g=15633   x43 (11/11/11/10)
```

So the harness can show something, which is what makes the empty offline trace mean
anything. The probe is left in the page **off by default** and reachable only by that
explicit query parameter, so this is re-checkable rather than a one-off.

Three things worth knowing about the probe path, none of which affect the offline default:

- `fromWorldImagery()` is not a Cesium-hosted tile service. It resolves through ion to
  **Bing Maps / virtualearth.net** — a third party with its own terms and its own key,
  both shipped in the bundle.
- The bundled ion demo JWT's `aud` claim reads `1.145 Release - Delete on November 1, 2026`.
  It is time-limited. Anything depending on it would break by itself.
- The Bing tiles are requested over **plaintext `http://`** (`uriScheme=http`, following the
  page's own scheme).

## Layout

```
viewer/
  package.json / package-lock.json   cesium@1.145.0, pinned
  cesium-manifest.txt                sha256 of every vendored file
  scripts/vendor-cesium.mjs          node_modules -> public/vendor/cesium
  scripts/serve.mjs                  loopback static server, logs every request
  scripts/build-wasm.mjs             builds + verifies + copies the engine .wasm
  public/index.html                  the minimal Viewer + the net probe
  public/vendor/cesium/              the vendored build (committed)
  public/wasm/                       the built engine artifact (committed)
```

`window.viewer` and `window.__viewerReady` are exposed for the trace harness and for the
later tasks in this slice.

## Building the engine .wasm

```
cd viewer
npm run build:wasm             # builds, verifies, copies into public/wasm/
npm run build:wasm:self-test   # proves the verification can actually fail
```

`scripts/build-wasm.mjs` runs
`cargo build -p worldbuilder-engine --release --target wasm32-unknown-unknown
--no-default-features --features wasm`, deletes any existing artifact first so the run
cannot be a stale no-op, then refuses to trust the exit code. It hand-parses the built
module's import section (id 2) and export section (id 7), cross-checks that against
Node's own `WebAssembly.Module.exports`/`imports`, and cross-checks the export *names*
against the `pub extern "C" fn` declarations in `crates/worldbuilder-engine/src/wasm.rs`
itself — not a hardcoded list that could drift from the source. A module under 20 KB, or
with any imports, or whose export names don't match the source, fails the script.

This exists because the first artifact in this project was 327 bytes exporting only
`memory`: a `cdylib` discards every module when nothing is `#[no_mangle] extern "C"`, and
a green `cargo build` cannot tell you that. The current real artifact is 84,856 bytes, 11
exports (`memory` + 10 functions), 0 imports.

`npm run build:wasm:self-test` proves the check itself works, rather than assuming it
does: it builds the crate **without** `--features wasm`, which reproduces the 327-byte
memory-only failure mode exactly, confirms the strict assertion rejects it, then rebuilds
the real artifact so the tree is left in a good state.

No `wasm-bindgen`, no `wasm-opt`, no bundler — the module has zero imports by design, so
`WebAssembly.instantiate(bytes, {})` is the entire loader. `public/wasm/` is committed
(the same choice as the vendored Cesium tree) so a fresh checkout can serve the viewer
without a Rust toolchain; re-run `npm run build:wasm` after any change to
`crates/worldbuilder-engine`.

## The terrain provider (Task 4)

`public/app/` — four ES modules over the global `Cesium` (the vendored build is the IIFE
one; there is no bundler and none is needed).

| file | what it is |
|---|---|
| `engine.js` | the wasm loader and the marshalling for the ten `extern "C"` entry points. `WebAssembly.instantiate(bytes, {})` is the whole loader — zero imports by design. |
| `terrain.js` | `CustomHeightmapTerrainProvider` over `wb_fill_tile_f32`, plus the zoom cap and the deliberate wrong implementations. |
| `main.js` | builds the world, installs the provider, paints a hypsometric ramp, reads URL parameters. |
| `verify.js` | the checks. `window.__wb.check()` in the console. |

```
http://127.0.0.1:8137/
  ?seed= ?radius= ?plates= ?land= ?harbour=1   the world
  ?maxLevel= ?size=                            the tiling
  ?exaggeration= ?paint=0 ?atmosphere=1        what it looks like
  ?fly=lat,lon,height                          where to look
  ?fault=flip-latitude|shift-tile|wrong-world  a deliberate wrong implementation
```

**No provider class is written.** `CustomHeightmapTerrainProvider` exists for procedural
sources: one callback, and it builds the `HeightmapTerrainData` itself. Its constructor
already calls `getEstimatedLevelZeroGeometricErrorForAHeightmap` — measured at
**77,067.34 m** for a 65-post tile on a 2-tile level 0 — and
`getLevelMaximumGeometricError(level)` is that over `1 << level`. There is **no `ready` or
`readyPromise`**; both were removed in 1.107 and the provider is usable the instant it is
constructed.

The buffer is a `Float32Array` with the **default structure** (`heightScale` 1,
`heightOffset` 0, `stride` 1), so the values are metres above the ellipsoid directly. **Row 0
is the north edge** — that is `HeightmapTerrainData.interpolateHeight`'s own convention
(`southInteger = height - 1 - southInteger`), and the fill is handed the rectangle's north
latitude as `lat0Deg`.

### The zoom cap is level 12

`getTileDataAvailable` returning `undefined` is the trap: the prototype's answer is
`undefined`, `GlobeSurfaceTile.prepareNewTile` then falls through to
`terrainData.isChildAvailable`, which is always true for the default child tile mask, and
refinement is bounded only by a screen-space error that halves every level.

**Measured, camera 300 m above 12 N 34 E, 400 frames:**

| | maxDepthVisited | tilesVisited | JS heap |
|---|---|---|---|
| capped at 12 | **15, flat** | **89, flat** | ~40 MB, flat |
| cap removed (`undefined`) | 13 → 16 → 18 → 22 → **25 and climbing** | 80 → **379 and climbing** | 33 → **54 MB and climbing** |

The steady state is **cap + 3, not cap + 1**: the gate is `QuadtreePrimitive.visitTile`'s
`allAreUpsampled`, and a tile is only marked `upsampledFromParent` once it has been visited
and processed, so the traversal overshoots a little before settling.

**Why 12.** A `GeographicTilingScheme` tile spans `180 / 2^level` degrees and a 65-post
heightmap samples it every `180 / (2^level · 64)` degrees; on this project's 6,371,000 m
radius that is `312,735.98 m / 2^level`:

```
level 10 -> 305.4 m    level 12 -> 76.35 m
level 11 -> 152.7 m    level 13 -> 38.18 m
```

The generated field's **resolution floor is 78.125 m** — peak-to-peak relief on a 2 km
transect rises monotonically 0.19 → 5.58 m from `r = 20000` down to `r = 78.125` and is then
bit-identical for 50, 25 and canonical; below ~100 m the field is a tilted plane (4.5 cm of
chord deviation over 100 m). **Level 12 is the first level at or below that floor**, so it is
the last level at which zooming reveals generated ground that was not already there.

**Authored features are the exception, and level 12 is not enough for them.**
`Features::apply` is analytic and outside the octave schedule, so it is
resolution-independent *point-wise* but still **grid-sampling-limited**. Measured on the
extraction's harbour (a 900 × 260 m carve to −12 m with a 200 × 60 m mole to +4 m, on
−4,600 m seabed): the centre reads **exactly +4.00 m at `resolution_m` of −1, 76.35, 152.7
and 305.4 alike**, and a 100 m transect through it has **2,424.8 m of relief at every one of
those resolutions**. But the tile that contains it tops out at

```
L12 (76.4 m posts) -> -819.4 m     L15 (9.5 m) -> -39.6 m
L13 (38.2 m)       -> -457.7 m     L16 (4.8 m) ->  +3.2 m
L14 (19.1 m)       ->  -40.8 m     L17 (2.4 m) ->  +3.2 m
```

against a +4 m target — a 60 m-wide mole is narrower than one level-12 post. **Resolving that
harbour needs about level 16.** A feature-aware availability function (deeper only where a
feature reaches) is the right answer and is deliberately not built here: it needs Task 5's
cache and worker pool to be affordable. `?maxLevel=` exists so the claim stays testable.

### What was verified, and how it was made to fail

`window.__wb.check()`, eight checks, all passing on the default world:

- **witnessed-elevation** — `wb_elevation_m(12, 34, res 250)` is **exactly**
  `682.3921701573904`, the value the extraction pinned three independent ways (Python wheel,
  native Rust, browser WASM) and that `crates/worldbuilder-engine/tests/wasm_exports.rs`
  carries as `WITNESSED_ELEVATION_M`. Nothing in `viewer/` produced that number.
- **tile-posts-exact** — **0 of 38,025 posts divergent** across nine tiles (one per named
  point at level 12, both level-0 hemispheres, one at level 5). Each post is compared as
  `Math.fround(wb_elevation_m(lat, lon, spacing)) === buffer[row·65 + col]` — exact, not a
  tolerance — at the latitude and longitude **Cesium's** row convention places that post.
- **interpolate-height** — through `HeightmapTerrainData.interpolateHeight`, worst |delta|
  **2.12e-4 m** at six named points.
- **land-and-sea** — **2,519 of 2,520** grid points agree on land-vs-sea between the loaded
  terrain and the engine (99.96%); cos(lat)-weighted land fraction **29.2%** against a
  requested `land_fraction` of 0.29.
- **provider-shape**, **heightmap-structure**, **zoom-cap**, **quadtree-depth**.

**Rendered pixels, checked back against the engine.** `scene.pickPosition` on a 6,828-pixel
near-nadir grid: the surface Cesium actually draws sits at the engine's heights to
**max 0.52 m, mean 0.096 m** over a coastline at 60 km, and **max 1.13 m, mean 0.294 m** over
open ocean at 40 km. Sampled at 70 × 35 over a full-disc view, the rendered land/sea glyphs
match the engine's land/sea map **447 of 450**, the three misses all on the limb. So the
picture is a globe with a continent where the engine puts one and ocean where it puts ocean,
and the surface is the generated field, not a fallback ellipsoid.

`?fault=` installs a plausible wrong implementation, because a check that has never rejected
anything is not known to work:

| fault | what it does | caught by |
|---|---|---|
| `flip-latitude` | row 0 at the south edge — an upside-down planet that renders beautifully | tile-posts (first divergence at row 0 col 0), interpolate-height (14.2 m), land-and-sea (80.16%) |
| `shift-tile` | the tile filled one post east of where Cesium places it — 1/64 of a tile, invisible by eye | tile-posts (**35,271 / 38,025** divergent), interpolate-height (0.686 m). land-and-sea stays green, correctly: it cannot see 76 m |
| `wrong-world` | seed + 1 — a different planet, drawn without complaint | tile-posts (**37,189 / 38,025**), interpolate-height (5,050 m), land-and-sea (57.38%) |

`wrong-world` diverging on 97.8% rather than 100% is the healthy signature: the 836 agreeing
posts are almost all the abyssal-floor clamp at −4,600 m.

Two things the checks caught in themselves, worth recording:

- `provider.constructor.name` is **`xA`**. The vendored Cesium is the minified build, so
  every class name is mangled; the shape check now uses `instanceof`.
- `quadtree-depth` reported a **trivial pass on a page that had never rendered**
  (`maxDepthVisited` 0, and 0 ≤ anything). It now reports NOT EXERCISED when
  `frameState.frameNumber` is 0.

Two Cesium picking facts that cost time and are worth writing down:
`camera.pickEllipsoid` converts window coordinates with `canvas.clientWidth/clientHeight`
while the framebuffer is `canvas.width/height`, so a 1280 × 720 container over a 560 × 560
buffer misregisters every ray — this wrecked the first pixel check (50% agreement) before it
was a real finding about anything. And `scene.pickPosition` is **only accurate at short
range**: at 6,000 km it disagreed with the same pixel's shaded height by ~1 km, which at a
coastline flips the sign.

No network: 8 resources on the default page, **0 off-origin**. `wb_world_count()` is 1 after
a full check run — no leaked worlds.
