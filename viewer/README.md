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
