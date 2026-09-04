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
somewhere else. It also sets a **Content-Security-Policy** (below) and COOP/COEP.

### COOP/COEP earn their place by 5 microseconds, not by SharedArrayBuffer

They were added in Task 5 "for the SharedArrayBuffer worker pool", and that reason was
never true: Task 5 shipped eight **module** workers, the engine wasm has zero imports and
no shared memory, and nothing in this tree references `SharedArrayBuffer`. Task 6 measured
what removing them changes, on the same page one header apart:

| | COOP/COEP on | off |
|---|---|---|
| `crossOriginIsolated` | true | false |
| `SharedArrayBuffer` | `function` | `undefined` |
| `performance.now()` step | **5 us** | **100 us** (Chrome's non-isolated clamp) |
| 16,900-byte `slice()`, n=1,920 | median **0.010 ms** | median **0.000 ms**, 1,731 samples exactly zero |
| `__wb.check()` | 12/0 | 12/0 |
| `maxDepthVisited` / tiles | 16 / 39 | 16 / 39 |

That copy is `bench.js`'s `timeHandouts`, and "**a measured 0.02 ms median over n=1,920**"
below is the whole evidence for *the copy on the way out of the cache is not optional*.
Under the 100 us clamp that measurement does not get worse, it **stops existing**. So the
headers stay, for the measured reason rather than the invented one. They cost nothing here
— every response is same-origin and already carries CORP — but they are not free forever:
under COEP `require-corp` any subresource that loses that header fails. If `timeHandouts`
is ever changed to time a *batch* of copies, they stop earning their place.

## The Content-Security-Policy

Task 1 **witnessed** that nothing leaves the origin. But absence of traffic is not absence
of capability: that trace shows Chrome on Windows *did not* phone home, not that the page
*cannot*. `default-src 'self'` is what converts the observation into a guarantee.

```
default-src 'self'; script-src 'self' 'unsafe-eval' blob:; worker-src 'self' blob:;
style-src 'self' 'unsafe-inline'; img-src 'self' data:; object-src 'none';
base-uri 'none'; form-action 'none'; frame-ancestors 'none'
```

Every relaxation was arrived at by starting from `default-src 'self'` **alone** and adding
only what the browser reported as a violation. Nothing here is precautionary:

| token | what forced it |
|---|---|
| `script-src 'unsafe-eval'` | **the vendored bundle, not us.** Cesium 1.145.0 embeds Knockout, whose UMD preamble at `Cesium.js:18266` is `var t = this \|\| (0,eval)("this")`. The bundle is strict, so `this` is undefined and the eval always runs; without the token `Cesium.js` throws `EvalError` at load and `Cesium` is never defined. Present in `index.js` and `index.cjs` too, so no Cesium build avoids it, and patching the vendored tree would break `cesium-manifest.txt`. |
| (`'wasm-unsafe-eval'`) | subsumed by the above, but otherwise required: a bare `default-src 'self'` blocked `WebAssembly.instantiate` **five times from Cesium's own KTX2/Draco modules** and twice from `/app/engine.js`. |
| `script-src blob:` | Cesium's workers are `blob:` URLs and `importScripts()` further `blob:` URLs from inside them; a worker inherits the page's policy. |
| `worker-src 'self' blob:` | `'self'` for `/app/tile-worker.js` (the eight Task 5 module workers), `blob:` for Cesium's own pool. |
| `style-src 'unsafe-inline'` | Cesium both sets style attributes (`Cesium.js:79`, `:6070`, `:6071`) and injects `<style>` elements (`:13394`). |
| `img-src data:` | the `<link rel="icon" href="data:,">` that stops the browser asking for `/favicon.ico`. |

`connect-src`, `font-src`, `media-src` and `frame-src` are deliberately **absent** so they
fall back to `default-src 'self'`. `connect-src` is the one that refuses the net probe.

**`index.html` has no inline `<script>` or `<style>` any more**, so `script-src` needs no
`'unsafe-inline'`. The former inline blocks are `/app/cesium-base-url.js`, `/app/boot.js`
and `/app/viewer.css`, in the same document order — classic non-deferred `<script src>`
still blocks and still runs in order, so `CESIUM_BASE_URL` is still set before `Cesium.js`.

### Proved able to refuse

A policy that has never refused anything is not known to be doing its job. Same page,
`?net-probe=1`, one header apart:

| | securitypolicyviolation | off-origin resource entries | hosts reached |
|---|---|---|---|
| policy **on** | **1** — `connect-src`, `api.cesium.com` | 1, `transferSize 0`, request never completes | **0** |
| policy **off** | 0 | **34** | 6: `api.cesium.com`, `dev.virtualearth.net`, `ecn.t{0,1,2,3}.tiles.virtualearth.net` over plaintext `http://` |

The policy stops the chain at its first link: without the ion endpoint there is no Bing
key, and the 30-odd plaintext tile requests never happen.

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
  scripts/serve.mjs                  loopback static server, CSP + COOP/COEP, logs every request
  scripts/build-wasm.mjs             builds + verifies + fingerprints + copies the engine .wasm
  public/index.html                  the page. No inline script or style: the CSP forbids it
  public/app/cesium-base-url.js      CESIUM_BASE_URL, before Cesium.js
  public/app/boot.js                 the Viewer + the net probe
  public/app/viewer.css              the page's own style
  public/vendor/cesium/              the vendored build (committed)
  public/wasm/                       the built engine artifact (committed)
```

`window.viewer` and `window.__viewerReady` are exposed for the trace harness and for the
later tasks in this slice.

## Building the engine .wasm

```
cd viewer
npm run build:wasm                   # builds, verifies, fingerprints, copies into public/wasm/
npm run build:wasm:self-test         # proves the shape verification can actually fail
npm run check:wasm                   # is the SHIPPED artifact built from current source?
npm run build:wasm:stale-self-test   # proves the staleness fingerprint can actually fail
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

### The staleness guard, and the gap that only existed as a composition

Everything above proves things about the artifact's **shape**. The parity harness
(`crates/worldbuilder-engine/parity`) proves the **shipped bytes** agree with native source
to the bit — 53,251 values, 0 divergent. Neither asks the remaining question: *were these
bytes built from the source that is here now?*

Neither silence is a defect alone. Together they are: **a stale `.wasm` passes parity green
forever while the source moves underneath it**, because the corpus it is replayed against
was recorded from the same stale build.

**This was not hypothetical when the guard was written — the committed artifact was
already stale.** A rebuild from unchanged source produced a *different* artifact of
identical size, differing in exactly five bytes. All five are `panic!` location records
pointing into `crates\worldbuilder-engine\src\wasm.rs`, and all five are line numbers,
shifted by +11 and +28:

```
offset 69815  198 -> 209      offset 69907  477 -> 505
offset 69875  188 -> 199      offset 69923  643 -> 671
offset 69891  456 -> 484
```

Commit `d0c2eff` changed `wasm.rs` by +34/-6 — net **+28** lines — and it landed *after*
`0562500`, the commit that added the artifact. The artifact was never rebuilt. It had been
shipping and passing parity, several commits behind its own source, ever since. Line
numbers in panic metadata never execute, which is exactly why nothing noticed.

The guard is a content hash over every input that can change the artifact, written into
`public/wasm/MANIFEST.txt` at build time:

* every file under `crates/worldbuilder-engine/src`, recursively;
* `crates/worldbuilder-engine/Cargo.toml`, the workspace `Cargo.toml`, `Cargo.lock`;
* the compiler version (`rustc -vV` release, commit hash and host);
* the literal cargo argument list.

24 inputs. Deliberately over-inclusive: `bindings.rs` and `src/bin/` cannot affect a
`--no-default-features --features wasm` build and will still trip it. A false *rebuild it*
is a cheap failure; a false *it is current* is the one that costs. The artifact's own
sha256 is recorded too, so a hand-edited or swapped `.wasm` is caught by the same command.

**A hash over inputs is only sound if the artifact is a function of those inputs**, so that
was checked rather than assumed: two consecutive rebuilds of identical source produced
byte-identical artifacts (`1395f246…`), while the committed one differed (`f2a42266…`) for
the reason above — older source, not a nondeterministic build.

**Proved able to refuse**, three ways, one arm each:

| what was done | `npm run check:wasm` |
|---|---|
| nothing — current tree | `Current: … matches its manifest and the source that is here now`, exit 0 |
| one comment line appended to `wasm.rs`, artifact **not** rebuilt | `STALE ARTIFACT: the shipped .wasm was NOT built from the source that is here now`, exit **1** |
| the previously-committed `.wasm` swapped back in | `STALE ARTIFACT: the shipped .wasm is not the one this manifest describes`, exit **1** |
| a `MANIFEST.txt` from before the guard | `predates the staleness guard`, exit **1** |

And the composition, demonstrated end to end: with that one comment line added and the
artifact not rebuilt, **the parity harness reported `OK: zero divergent` and exited 0**
while `check:wasm` exited 1. That is the whole point of the guard in one run.

`npm run build:wasm:stale-self-test` is the unattended half: it copies the fingerprint
inputs to a temp tree, confirms an *unmodified* copy fingerprints identically (so the
digest depends on content and not on path), appends one line to the copy's `wasm.rs`, and
fails loudly if the digest does not move. It never writes inside `crates/`.

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


## Workers, the cache, and feature-aware availability (Task 5)

Three modules join the four from Task 4:

| file | what it is |
|---|---|
| `public/app/tile-worker.js` | one module worker: its own engine instance, its own world |
| `public/app/pool.js` | the worker pool and the LRU tile cache |
| `public/app/availability.js` | `getTileDataAvailable`, feature-aware |
| `public/app/bench.js` | the frame-budget measurement, `window.__wb.bench()` |

New URL parameters: `?workers=` (0 keeps Task 4's synchronous main-thread fill, and is the
A/B baseline every figure below is measured against), `?cache=0`, `?cacheTiles=`,
`?featureCeiling=`, `?trace=N` (record N frame deltas from boot), and two more faults,
`?fault=stale-worker|cache-key|feature-blind|feature-everywhere`.

### Each worker builds its own world

The engine wasm has zero imports and no shared memory, so every `WebAssembly.instantiate`
gets a fresh linear memory and a world handle is an index into a table inside it. Handles
cannot cross. Each worker therefore calls `wb_world_new` itself, which costs a measured
**3.5–5.9 ms** of `Surface::new` per worker and happens once.

### The copy on the way out of the cache is not optional

`HeightmapTerrainData` keeps the buffer it is given **and transfers it to a Cesium worker
when upsampling a child**, which detaches it. The cache holds a master copy Cesium never
sees and hands out `master.slice()` — 16,900 bytes, a measured **0.02 ms median over
n=1,920**, against a fill measured in milliseconds. Handing the same array out twice would
hand out a detached, length-0 buffer the second time: no error, a flat tile.

### The frame budget, measured in Chrome 151

480 level-12 tiles (76.35 m posts), classified from their own heights after filling. The
coastal population is nominated by bisecting to the zero crossing — a plain 3° grid step is
330 km and produced a coastal population of 4 tiles out of 480, which is not a population.

| population | n | median | p90 | max | mean |
|---|---|---|---|---|---|
| coastal | 137 | 14.46 | 27.93 | 49.77 | 17.41 |
| land | 173 | 4.60 | 19.16 | 32.22 | 9.49 |
| shelf | 39 | 12.37 | 26.84 | 35.04 | 12.34 |
| deep ocean | 131 | 4.05 | 16.45 | 25.71 | 6.66 |
| **all** | **480** | **11.54** | **20.93** | **49.77** | **11.21** |

Milliseconds per tile, serial, on the main thread. Coastal against deep ocean is **3.6× on
medians, 2.6× on means**, and 14.3× between the extremes (49.77 against 3.47).

Frame deltas from `requestAnimationFrame`, traced from boot at the same viewpoint
(20 N 110 W, 60 km), 400 frames, 39 tiles streamed in each case:

| | median | p90 | p99 | max | frames over 16.7 ms |
|---|---|---|---|---|---|
| `?workers=0` (Task 4) | 10.02 | 13.96 | 29.96 | 67.64 | **25 / 399** |
| 8 workers | 9.58 | 11.45 | 16.16 | 52.39 | **3 / 399** |

Wall clock for the 480-tile list, each pool warmed once and the warm pass discarded:

| workers | wall clock | speedup | per tile at that concurrency (median) |
|---|---|---|---|
| 1 | 7,503 ms | 1.00× | 14.61 ms |
| 2 | 3,727 ms | 2.01× | 15.48 ms |
| 4 | 2,195 ms | 3.42× | 17.41 ms |
| 8 | 1,417 ms | **5.29×** | 22.92 ms |

Per-tile cost *rises* with concurrency — eight workers share memory bandwidth and a turbo
budget — which is why the curve is 5.29× and not 8×.

### Feature-aware availability

`MAX_LEVEL = 12` is the **ground** cap and its justification in metres is unchanged. Past
it, `availability.js` refines only inside a feature's footprint.

The footprint is the engine's own: `Placed::weight_at` returns `0.0` outside
`reach_m = hypot(length_m, width_m)` by an early return, so that circle is exactly where a
feature stops existing. The lat/lon box used here is a conservative superset of it.

The depth is `post spacing <= min(length_m, width_m) / 8`, fitted to Task 4's measured
convergence: the harbour's 200 × 60 m mole gets **level 16**, which is where the tile
reaches +3.2 m against a +4 m target; `/4` would have chosen level 15, which reads −39.6 m.
The 900 × 260 m carve gets level 14. `FEATURE_CEILING = 18` bounds the rule.

**Cost, enumerated rather than estimated** (the extra tiles are only reachable by descending
from an available parent, so the set can be walked): L13 2, L14 6, L15 2, L16 6 — **16 extra
tiles in total** for the two-feature harbour. Under `?fault=feature-everywhere` the same
enumeration gives 8 + 32 + 128 + 512 = 680 in that one cone alone.

Measured in the quadtree, camera 120 m above the harbour, same world, one flag apart:

| | maxDepthVisited | tilesVisited | fills | `globe.getHeight` at the mole centre |
|---|---|---|---|---|
| ground cap only (`?fault=feature-blind`) | 13 | 23 | 17 | **−1,773.59 m** |
| feature-aware | **16** | 45 | 30 | **+0.35 m** |

The engine's canonical elevation at that point is **+4.00 m**. JS heap ~30 MB in both cases.

### What was verified, and how it was made to fail

Twelve checks now (`window.__wb.check()`), 12/0 on the harbour world and 11/0 on the
featureless default. Task 4's numbers reproduce exactly through the worker path: 0 of 38,025
posts divergent, worst `interpolateHeight` delta 2.12e-4 m, 2,519/2,520 on land-and-sea.

Three new checks: **worker-path** (0 main-thread fills, every worker used — a pool that
quietly fell back would render identically and every bit-exact check would still pass),
**cache-identity** (two adjacent tiles each match the engine at their own rectangle and are
not each other; a repeat is a hit, equal, and a different object), **feature-availability**
and **feature-resolves**.

| fault | what it is | caught by |
|---|---|---|
| `flip-latitude` | row 0 at the south edge | tile-posts 36,258/38,025; interpolate 1.87e3 m; land-and-sea 80.16% |
| `shift-tile` | filled one post east | tile-posts 35,271/38,025; interpolate 228 m |
| `wrong-world` | seed + 1 everywhere | tile-posts **37,189/38,025 (97.8%)**; interpolate 5,050 m; land-and-sea 57.38% |
| `stale-worker` | **one worker of eight** on seed + 1 | tile-posts **8,050/38,025 (21.2%)** — two of nine tiles; interpolate 530 m; land-and-sea 97.14% |
| `cache-key` | key drops the tile x | **cache-identity** (right tile 4,225/4,225); tile-posts 3,333/38,025; land-and-sea 60.08% |
| `feature-blind` | availability ignores features (the Task 4 behaviour) | **feature-availability**: 2 features requested, 0 known |
| `feature-everywhere` | refine to the feature depth globally | **feature-availability**: available 20° from every feature; zoom-cap |

No fault diverges on 100% of posts; the project's standing warning is that 100% has always
meant a broken harness.

**Two bugs the fault runs found in the checks themselves**, which is the fourth and fifth
time this has paid in this slice:

- `feature-availability` originally compared the availability function against *its own*
  footprint list, so `feature-blind` walked into the "no features on this world" branch and
  passed by agreeing with itself. It now compares against the world spec.
- `feature-resolves` iterated `availability.footprints`, which `feature-blind` empties, so
  it reported nothing and passed — a check with no work to do. It is now driven from the
  spec through `featureLevel` directly.
- `quadtree-depth` asserted `maxDepthVisited <= maxLevel + 3`, from Task 4's measured 15.
  It failed the first time the camera sat 120 m above the harbour, and chasing that turned
  up something bigger, described below.

### `maxDepthVisited` is not what the cap bounds

Task 4 reported `maxDepthVisited` settling at 15 against a cap of 12 — "cap + 3". **That
figure does not survive a real canvas.** With the camera 300 m above 12 N 34 E in a
1200 x 800 tab it settles at **26**, and it does so *identically* on Task 4's own
synchronous path (`?workers=0&cache=0`), so this is not a Task 5 regression: Task 4's
number is an artifact of its hand-driven 560 x 560 backing buffer, where the screen-space
error is much larger. Task 4's report says as much — it could not display the browser pane
and drove `viewer.render()` by hand.

What is flat, in both cases and at both canvas sizes, is everything that costs anything:

| | value |
|---|---|
| deepest tile **requested** | **12** — the cap, exactly |
| tiles visited | 43, flat over 4,000 frames |
| fills after settling | none |
| JS heap | 33–40 MB, oscillating on GC, no trend |

So the traversal overshoots the cap by fourteen levels and it costs nothing, because a tile
above the cap is never requested. The check now asserts **`maxLevelRequested <= ceiling`**,
which is the quantity the availability function actually controls, and reports the depth
alongside it. Under the prototype's `undefined` the requested level climbs without bound,
so the new assertion still catches the failure Task 4 was guarding against — and catches it
by the tiles that get built rather than by the nodes that get walked.
