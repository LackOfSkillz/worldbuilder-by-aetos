# viewer — offline CesiumJS shell

Slice 2b. A vendored CesiumJS and a **read-only** window onto a world built by
`worldbuilder-engine` in WebAssembly: a custom heightmap terrain provider over the
generator, a pool of eight workers with an LRU tile cache, feature-aware refinement, twelve
in-page checks and a `?fault=` switch that makes them fail on demand. No placement, no
editing, no worldfile — **slice 3 owns those**.

> **STALE FIGURES, NAMED RATHER THAN SILENTLY WRONG (re-measured at slice mountains Task 6,
> 2026-09-05; first written at slice 5b Task 6 / relief Task 5, at `1004f4d`).** This file's
> *current-tense* engine and parity numbers were written in slice 2b and have not moved since;
> several slices have landed on top of them. Rather than leave a reader to trust them, here is
> what the same commands actually print today, re-run on this host while writing this note --
> **and the previous version of this note was itself stale by four exports and 18,265 corpus
> values, which is the whole argument for re-deriving rather than carrying forward**:
>
> | this file says | measured today |
> |---|---|
> | the artifact is 84,856 bytes, 11 exports (`memory` + 10 functions) | **225,277 bytes, 19 exports** (`memory` + 18 functions), 0 imports |
> | parity is 53,251 values, 0 divergent | **89,861 values, 0 divergent** |
> | `--mutate seed` diverges 50,778 | **86,190 of 89,861** -- and there are now three further, narrower controls: `--mutate erosion-k` (216), `--mutate water-pond` (60) and `--mutate tectonic-warp` (6,186, all of them on a world built from a tectonic block) |
> | "There is no CI" | there is: `.github/workflows/gates.yml`, six gates plus two count gates, on every push. See `docs/ci.md`. |
>
> The *historical* narrative in this file -- the staleness-guard investigation, the
> `?fault=` evidence, the frame budgets -- describes specific past commits and their
> evidence, and is deliberately left as written: re-stamping it with today's numbers would
> misrepresent what was true at the time it describes. The authority for the engine's current
> figures is `crates/worldbuilder-engine/README.md`, `crates/worldbuilder-engine/parity/README.md`
> and `viewer/public/wasm/MANIFEST.txt`.
>
> **The browser-side half of that note is now discharged, and this is what a browser said**
> (relief Task 6, 2026-09-05 at `bbb2108`; Chromium 148 in the agent browser pane, ANGLE
> Intel UHD D3D11, 32 cores, viewport emulated to 1400x900 or 1000x800, frames driven by hand
> — see *[Two hosts, and what neither of them can do](#two-hosts-and-what-neither-of-them-can-do)*):
>
> | this file says | measured today |
> |---|---|
> | "52 requests — 48 same-origin plus 4 `blob:` — 39 resource-timing entries, 0 off-origin" | **86 server requests** for one default page load (85 `200` plus one `404` for `/favicon.ico`), **76 resource-timing entries** over 29 distinct paths, **0 off-origin**. The growth is the relief layer's own modules and the pool's per-worker module graph: 8 x `tile-worker.js` plus 8 copies each of `engine.js`, `relief.js`, `relief-params.js`, `terrain.js`, `pool.js` and the `.wasm`. |
> | `__wb.check()` 11/0 featureless, 12/0 harbour | **reproduced: 11 passed / 0 failed** on `/`, **12 / 0** on `/?harbour=1`, no console errors |
> | "nine copies of an 84,856-byte artifact over loopback" | nine copies is right (1 main thread + 8 workers, counted in the server log); the **size is not** — see the row above about 220,452 bytes |
>
> **The radius-slider bug this box used to report as open is FIXED**, in `6a07530`: the travel
> is `step: 1e3` and the panel now reads **6.371 Mm** where it used to print "6.4 Mm" at one
> decimal for both values. It was not fixed alone — see
> *[The panel-default family, closed by a check](#the-panel-default-family-closed-by-a-check)*,
> which found a fourth member of the same family nobody had reported.
>
> **What still needs a person and a headed browser**, and is not repaired here: every
> *frame-time* figure in this file. The agent browser pane reports `document.hidden === true`
> at every moment, so `requestAnimationFrame` never fires in it and the render loop never
> runs; frames can only be driven by hand through `scene.render()`. Wall-clock "time to
> settle" and frames-per-second cannot be measured that way at all.

**New here? Read [The record (Task 7)](#the-record-task-7) first.** It is the consolidated
statement of what this thing guarantees, what it costs, what it deliberately does not claim,
and what is still open. Everything between here and there is the working notes of the six
tasks that built it, kept because the reasoning is the expensive part.

**Then read [The relief imagery layer](#the-relief-imagery-layer) at the end**, which is the
record of the slice that came after it and which changed what the viewer draws. It is the one
section that answers *"why does the planet look like that, and how much of it got fixed."*

The sections below begin where the slice did: with a minimal `Viewer` that drew nothing but
the ellipsoid, and the harness used to **witness** that the page makes no request off its own
origin.

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

### What this policy does not claim

**It does not claim the page cannot evaluate a string.** `'unsafe-eval'` is in
`script-src` — not the narrower `'wasm-unsafe-eval'`, which would have been enough for the
WebAssembly and nothing else. It is there because the vendored bundle's embedded Knockout
opens with `var t = this || (0,eval)("this")` (`Cesium.js:18266`, and once in each of
`index.js` and `index.cjs` — verified by grep in Task 7, so no Cesium build avoids it). The
bundle is strict, so `this` is `undefined` and **the eval always fires**; without the token
`Cesium.js` throws `EvalError` at load and `Cesium` is never defined. Patching it would
break `cesium-manifest.txt`, which is the point of the manifest.

So state the guarantee precisely, because the two halves are not the same claim:

* **`script-src` governs execution, not egress.** Every relaxation in it — `'unsafe-eval'`,
  `blob:` — widens *what code may run*. None of them widens *where a byte may go*. The
  offline guarantee is carried by `default-src 'self'` and the absent `connect-src`,
  `img-src`, `font-src`, `media-src` and `frame-src` that fall back to it, and it is
  untouched by every token in `script-src`.
* **"This page cannot eval a string" is not a claim this policy makes, and must not be
  quoted as one.** It can. It does, at every load, from a third-party UMD preamble inside a
  dependency this project vendored deliberately. What it cannot do is reach another host.

**`index.html` has no inline `<script>` or `<style>` any more**, so `script-src` needs no
`'unsafe-inline'`. The former inline blocks are `/app/cesium-base-url.js`, `/app/boot.js`
and `/app/viewer.css`, in the same document order — classic non-deferred `<script src>`
still blocks and still runs in order, so `CESIUM_BASE_URL` is still set before `Cesium.js`.

### Proved able to refuse

A policy that has never refused anything is not known to be doing its job. Same page,
`?net-probe=1`, one header apart:

| | securitypolicyviolation | off-origin resource entries | hosts reached |
|---|---|---|---|
| policy **on** | **1** — `connect-src`, `api.cesium.com` | 1, and the chain stops there | **0** |
| policy **off** | 0 | **34** | 6: `api.cesium.com`, `dev.virtualearth.net`, `ecn.t{0,1,2,3}.tiles.virtualearth.net` over plaintext `http://` |

Both arms were re-run in Task 7, on the same page one header apart, and both reproduced
exactly: 1 violation naming `api.cesium.com` and one host in the entry list under the policy;
**34 entries across those six hosts, and both `https:` and plaintext `http:`, without it.**

**One correction to how the ON arm is read.** An earlier version of this table offered
`transferSize 0` as the evidence that the blocked request never completed. It is not
evidence: a cross-origin entry with no `Timing-Allow-Origin` reports `transferSize 0`
whether it was refused or fully served, and in Task 7's OFF arm **all 34 entries report 0
as well** while the tiles plainly arrive. The sound discriminator is the pair either side of
it — a `securitypolicyviolation` event naming `connect-src`, and a host set that **stops at
one name instead of growing to six**. The policy breaks the chain at its first link: no ion
endpoint, therefore no Bing key, therefore none of the 30-odd plaintext tile requests.

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

**This is the Task 1 page, and the page has grown four times since.** The request *list*
below is a historical trace of the bare ellipsoid viewer; the only line in it that is a
standing guarantee is the second number. Re-measured in Task 7 against the page as it ships
today: **52 requests — 48 same-origin plus 4 `blob:` — 39 resource-timing entries, and 0
off-origin.**

The additions over Task 1 are all this slice's own, and all same-origin: the same 13 Cesium
files, plus ten `/app/*` scripts and the one `/wasm/worldbuilder_engine.wasm` the page loads,
plus **`tile-worker.js`, `engine.js` and the `.wasm` once per worker** — 24 requests, because
the pool has eight workers, none of them shares a module instance with any other, and
`cache-control: no-store` means none of them shares a fetch either. Nine copies of an 84,856-
byte artifact over loopback is the price of eight independent linear memories, and it is the
right trade.

The Task 1 trace, for the record:

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
  public/app/engine.js               the wasm loader and the extern "C" entry points
  public/app/terrain.js              CustomHeightmapTerrainProvider, the cap, the faults
  public/app/availability.js         getTileDataAvailable, feature-aware
  public/app/pool.js                 the eight-worker pool and the LRU tile cache
  public/app/tile-worker.js          one module worker: its own engine, its own world
  public/app/main.js                 wiring, the hypsometric ramp, the URL parameters
  public/app/controls.js             the parameter panel, built from panel-fields.js
  public/app/panel-fields.js         ONE copy of every default and every slider's travel
  public/app/relief.js               the shaded-relief raster: hillshade + slope colour
  public/app/relief-provider.js      the Cesium ImageryProvider over that raster
  public/app/relief-params.js        the engine's relief presets, across the wasm boundary
  public/app/verify.js               the twelve checks -- window.__wb.check()
  public/app/bench.js                the frame budget -- window.__wb.bench()
  public/vendor/cesium/              the vendored build (committed)
  public/wasm/                       the built engine artifact + MANIFEST.txt (committed)
```

Every URL parameter, read from `main.js` and `boot.js` rather than remembered:

```
?seed= ?radius= ?plates= ?land= ?harbour=1     the world
?maxLevel= ?size= ?featureCeiling=             the tiling and the caps
?workers= ?cache=0 ?cacheTiles=                the pool and the cache
?relief=0 ?reliefSize= ?reliefMaxLevel=        the relief imagery layer
?reliefPreset= ?mountainM= ?quietingStrength= ?octavePersistence=   the relief channel
?sse=                                          THE detail knob -- see below
?exaggeration= ?paint=0 ?atmosphere=1 ?rampMin= ?rampMax=   what it looks like
?fly=lat,lon,height                            where to look
?trace=N                                       record N frame deltas from boot
?fault=<one of seven>                          a deliberate wrong implementation
?net-probe=1                                   the CSP's proof of refusal (boot.js)
```

`window.viewer` and `window.__viewerReady` are exposed for the trace harness and for the
later tasks in this slice.

### `?sse=` -- the one detail knob, and why the default is the cheap one

`maximumScreenSpaceError` is the **only** parameter that changes how much detail the
whole-planet view resolves. `?reliefSize=` does not: imagery tile width is detail-invariant
the same way heightmap width is -- a wider tile lowers geometric error and Cesium simply
refines one level less, landing on the same metres per texel. Measured at the default
orbital camera (`?fly=10,20,9000000`), 8 workers, hardware ANGLE/D3D11, 32 logical cores:

| `?reliefSize=` | relief tiles | deepest level | total texels | worker CPU | settle |
| --- | --- | --- | --- | --- | --- |
| 128 | 244 | 4 | 4.00 M | 4.81 s | 1.85 s |
| **256** (default) | 61 | 3 | 4.00 M | 4.79 s | 1.95 s |
| 512 | 15 | 2 | 3.93 M | 4.26 s | 2.09 s |

Same texels, same CPU, sixteen times fewer tiles. It is a **batching** knob.

`?sse=` is the one that moves detail, and it costs:

| | relief tiles | deepest level | m/texel | worker CPU | settle |
| --- | --- | --- | --- | --- | --- |
| **`sse=2`** (default) | 61 | 3 | 9,811 | 4.8 s | 1.95 s |
| `sse=1.5` | 83 | 3 | 9,811 | 6.3 s | 2.12 s |
| `sse=1` | 155 | 4 | 4,906 | 14.8 s | 3.35 s |

`1.5` buys 36% more tiles and **not** one more level -- the level is a step function of
this value, so the only two settings worth having are 2 and 1. The default is the cheap
one; **`?sse=1` is what buys the extra imagery level at orbital distance**, and it is worth
it if you are looking at the planet rather than flying over it. Note that it is a *global*
knob: it refines the terrain mesh too, so its cost is not confined to the relief layer.

**Both tables above were taken on headed Playwright Chromium with hardware ANGLE/D3D11.**
They were re-run for the record task on a second host (the agent browser pane, same GPU and
core count, frames driven by hand). **Everything that is a property of the geometry
reproduced exactly and everything that is a property of the machine did not**, which is the
same split this project keeps finding:

| re-run, second host | reproduced exactly | did not reproduce |
|---|---|---|
| `?reliefSize=` 128 / 256 / 512 | tiles **244 / 61 / 15**, deepest level **4 / 3 / 2**, texels **4.00 / 4.00 / 3.93 M** | worker CPU 14.1 / 9.9 / 12.3 s against 4.81 / 4.79 / 4.26 s |
| `?sse=` 2 / 1.5 / 1 | tiles **61 / 83 / 155**, deepest level **3 / 3 / 4**, m/texel **9,811 / 9,811 / 4,906** | worker CPU 9.9 / 17.1 / 26.0 s, i.e. `sse=1` costs **2.6x** rather than 3.1x |

So *"`sse=1` buys one imagery level and costs roughly three times the worker CPU"* is the
durable claim, and 3.1x is one host's instance of it. The **metres per texel** column is the
load-bearing one and it is arithmetic, not a measurement.

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
* every file under `crates/worldbuilder-engine/examples` and
  `crates/worldbuilder-engine/tests`, recursively;
* `crates/worldbuilder-engine/Cargo.toml`, the workspace `Cargo.toml`, `Cargo.lock`;
* the compiler version (`rustc -vV` release, commit hash and host);
* the literal cargo argument list.

28 inputs. Deliberately over-inclusive: `bindings.rs` and `src/bin/` cannot affect a
`--no-default-features --features wasm` build and will still trip it. A false *rebuild it*
is a cheap failure; a false *it is current* is the one that costs. The artifact's own
sha256 is recorded too, so a hand-edited or swapped `.wasm` is caught by the same command.

**It was 24 until 2026-09-04.** `examples/` and `tests/` were outside it, and CI proved that
mattered: run
[33916847441](https://github.com/LackOfSkillz/worldbuilder-by-aetos/actions/runs/33916847441)
shrank the parity corpus from 53,251 values to 52,451 by editing `examples/parity_dump.rs`,
and this command still said *matches the source that is here now* while parity said *zero
divergent*. Adding them changed the fingerprint (`cf3a437d…` → `64d7e7c3…`) but **not the
artifact** — the rebuilt `.wasm` was byte-identical, `60244aec…`, which is the reproducibility
claim below re-tested for free. The cost is that editing a test now requires
`npm run build:wasm` to re-bless the manifest.

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

### The guard could not pass on anyone else's machine, and that took a clean clone to find

Every arm above was run in the tree the artifact was built in. **A reviewer cloned the branch
and `npm run check:wasm` reported STALE ARTIFACT on the first try**, with no edit to
anything.

`sourceFingerprint` hashes the **working-tree bytes** of its inputs (24 at the time; 28
now). The repository had no
root `.gitattributes`, so those bytes were a property of the machine that checked the tree
out rather than of the commit: at `core.autocrlf=true` — the Git-for-Windows default — the
inputs arrive CRLF; at `core.autocrlf=false`, and on every Linux CI runner, LF. The tree this
was authored in held a *mix* of the two (14 LF, 9 CRLF and one file with both, measured with
`git ls-files --eol`) and so matched neither. Three trees, one commit `4595f5e`, three
digests:

| tree | `source-fingerprint` | `npm run check:wasm` |
|---|---|---|
| the author's working tree | `02744c04…` — the recorded one | exit 0 |
| clone, `core.autocrlf=true` | `a87614ad…` | **STALE ARTIFACT**, exit 1 |
| clone, `core.autocrlf=false` | `cf3a437d…` | **STALE ARTIFACT**, exit 1 |

**And the parity harness refuses on a stale artifact — by design, added one section above —
so the 53,251 / 0 parity result was unreproducible from git by anybody.** The strongest claim
in this slice existed on exactly one machine. A gate whose first act on a reviewer's machine
is a false alarm is a gate that gets switched off, which is worse than not having one.

`viewer/.gitattributes` had already met this exact hazard and fixed it for the vendored
Cesium tree, for the same reason in a different currency: a clone that checked the vendored
bytes out with CRLF would not match the npm package and every sha256 in the manifest would be
wrong. **The lesson was learned in one directory and not generalised**, and the fingerprinted
inputs live in `crates/`, which that file does not cover.

The fix is a root `.gitattributes` with `* text=auto eol=lf`: every text file stored LF and
checked out LF, on every platform, whatever `core.autocrlf` says. It changes checkouts and
not history — every tracked text blob was already LF in the index (`git ls-files --eol` over
537 files: 291 `i/lf`, 210 `i/-text` binary, 36 `i/none` empty; **zero `i/crlf`, zero
`i/mixed`**), so `git add --renormalize` stages nothing. The working tree was renormalised
and the artifact rebuilt once: `source-fingerprint` `02744c04…` → `cf3a437d…`, which is
exactly the digest the all-LF clone had been computing all along. **The `.wasm` bytes did not
change** — `artifact-sha256` `60244aec…` before and after — because line endings never
reached `rustc`, only the hash of the files handed to it.

**Proved on two fresh clones rather than on the machine that produced it**, which is the
whole point:

| clone of the fixed commit | `git ls-files --eol` on the inputs | `check:wasm` |
|---|---|---|
| `git config core.autocrlf true`, then checkout | `i/lf w/lf` | **exit 0** |
| `git config core.autocrlf false`, then checkout | `i/lf w/lf` | **exit 0** |

and the same two clones at the parent commit `4595f5e` still report `a87614ad…` and
`cf3a437d…`, exit 1 — so the two-clone test can tell the two states apart and is not
vacuously green. **`core.autocrlf` has to be set on the clone and not passed as `git -c` to
`git clone`**: the first attempt did the latter, both clones inherited the global `true`, and
both arms silently measured the same thing. It read as a pass.

Then, in the `autocrlf=true` clone, from a cold `CARGO_TARGET_DIR`:

```
provenance: the shipped .wasm matches its manifest and current source.
parity: 53251 values compared through the shipped exports, 0 divergent
CONTROL (--mutate seed): 53251 values compared, 50778 divergent
```

**53,251 / 0 and the 50,778 control now reproduce from a pristine clone**, which is the
claim this whole mechanism exists to support and which nothing had previously demonstrated.

### The guard is now wired into the parity harness

That gap — *green parity on a stale artifact* — is closed. `checkFreshness()` and
`destArtifact` are exported from `scripts/build-wasm.mjs`, and
`crates/worldbuilder-engine/parity/parity.mjs` **imports and runs them before it compares a
single value**. It imports rather than reimplements on purpose: two copies of a provenance
rule drift, and the copy that drifts is the one that stops refusing. The CLI still works
because `main()` only runs when this file is the program (`import.meta.url` against
`process.argv[1]`), so importing it has no side effects.

Re-run of the identical experiment, with the fix in place:

| what was done | `parity.mjs native.txt` |
|---|---|
| nothing — current tree | `provenance: the shipped .wasm matches its manifest and current source.` then `OK: zero divergent`, exit 0 |
| one comment line appended to `wasm.rs`, artifact **not** rebuilt | `REFUSING TO REPORT PARITY -- STALE ARTIFACT`, exit **1** (it was exit 0 before) |
| one byte of the shipped `.wasm` flipped, source untouched | `REFUSING … the shipped .wasm is not the one this manifest describes`, exit **1** |
| `--wasm <some other file>` | `REFUSING: … no manifest describes those bytes`, exit **1** |
| `--wasm <some other file> --no-provenance` | runs, every line labelled `UNVERIFIED`, exit 0 |
| `--no-provenance` on the **shipped** artifact | `REFUSING: --no-provenance cannot be used on the shipped artifact`, exit **2** |

There is deliberately **no flag that silences the guard for the shipped artifact**. An
escape hatch there would put the hole straight back, since the shipped bytes are the only
thing the harness exists to make a claim about.

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
radius that is **`312,735.73 m / 2^level`** — `postSpacingM(0, 65, 6371000)`, and
`π · 6,371,000 / 64` by hand. The error is 25 cm at level 0 and vanishes by level 3; the
four levels tabulated below were right all along, and `postSpacingM` computes the figure
rather than reading any comment. It is recorded anyway, because the way it survived is the
point: the *derivation* was checked and the *arithmetic* was not, by anybody, for six
commits.

**And then the correction reached the record and not the source.** Task 7 fixed this
paragraph, wrote that the line "read 312,735.98 m until now", and left
`public/app/terrain.js:52` — *the file the number came from* — still reading
**312,735.98**, where it stayed for a seventh commit until a reviewer ran
`git grep 312735` and got three hits, two of them right and one of them shipped. Both sites
now read 312,735.73. A correction that lands in the write-up and not in the code leaves the
wrong number in the place the next reader will actually look, and makes the write-up's
account of it false into the bargain.

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
a full check run — no leaked worlds. (The resource count is Task 4's page; on the shipped
page it is 39. **`0 off-origin` is the number that has never moved**, across every
measurement in every task of this slice.)


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

**Reproduce it with `window.__wb.bench({ perClass: 160 })`.** `runBench`'s default is
`perClass = 96` over three hints, so a bare `__wb.bench()` returns **288** tiles and the
table below cannot be matched against it at all. The parameter was never recorded beside the
measurement; Task 7 recovered it by arithmetic (480 = 3 × 160) and confirmed it — a second
session at `perClass: 160` reproduced **every population exactly**: 480 tiles, coastal 137,
land 173, shelf 39, deep ocean 131. A measurement whose invocation is not written down is
one edit away from being unrepeatable.

| population | n | median | p90 | max | mean |
|---|---|---|---|---|---|
| coastal | 137 | 14.46 | 27.93 | 49.77 | 17.41 |
| land | 173 | 4.60 | 19.16 | 32.22 | 9.49 |
| shelf | 39 | 12.37 | 26.84 | 35.04 | 12.34 |
| deep ocean | 131 | 4.05 | 16.45 | 25.71 | 6.66 |
| **all** | **480** | **11.54** | **20.93** | **49.77** | **11.21** |

Milliseconds per tile, serial, on the main thread. Coastal against deep ocean is **3.6× on
medians, 2.6× on means**, and 14.3× between the extremes (49.77 against 3.47).

**Say which statistic, every time.** "A coastal tile costs 9× a deep-ocean one" is a claim
this table does not make at any percentile. It was written into `src/wasm.rs`'s module docs
and corrected here first, on the reasoning that editing that file would invalidate the
shipped `.wasm`. That reasoning was wrong twice over: the file *was* edited and the artifact
*was* rebuilt in `4595f5e`, both sites now read historically (*"the 9x once quoted here
compares extremes"*), and a rebuild is a few seconds anyway — "it would invalidate the
artifact" is a reason to rebuild, never a reason to leave a wrong number in shipped source.
This paragraph claimed the opposite for one commit after it had stopped being true. The
defensible sentence is **~3.6× on medians**, with the extremes an order of magnitude apart.

**The populations are stable; the milliseconds are a host.** Task 7 re-ran the identical
measurement in a second session and got the same 137 / 173 / 39 / 131 split and a *different*
cost table — coastal median 13.65 ms, deep ocean 2.30 ms, all-tiles median 7.63 ms, which is
a coastal penalty of **5.9×** on medians rather than 3.6×. That session rendered through a
software rasteriser competing for the same cores, so its absolute milliseconds are not
comparable and are not being substituted here. What survives both sessions, and is the
finding: **a coastal tile costs several times a deep-ocean one, the multiple is a property of
the run, and a viewer looks at coasts.** Quote the ratio with the table it came from or not
at all.

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

Task 7's second session reproduced the *shape* and not the number: 1.00× / 1.94× / 3.57× /
**5.83×** over the same 480 tiles, with the same rising per-tile median (14.47 → 17.93 ms).
So "eight workers buy between five and six times, never eight, and per-tile cost rises as
you add them" is the durable claim; 5.29 is one host's instance of it.

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
| ground cap only (`?fault=feature-blind`) | 13 | 23 | 17 | **−1,770 m** (see below) |
| feature-aware | **16** | 45 | 30 | **+0.35 m** |

The engine's canonical elevation at that point is **+4.00 m**. JS heap ~30 MB in both cases.

**The feature-blind height is a between-sessions figure and is written as one.** It was
recorded here as a bare −**1,773.59 m**, which reads as exact and is not: it is
`globe.getHeight` over whichever coarse tile is resident, so it is a property of what the
globe had streamed. Three sessions, headless Chromium, camera set to 120 m above
121.5°E 18.25°S: Task 7 read −1,773.59 m, and two later sessions each read
−1,762.60 m. It does **not** drift with settling — 30 samples at 2 s intervals over 60 s
and 693 frames in the third session gave −1,762.60 m every time, with `maxDepthVisited` 13
and `tilesVisited` 23 flat throughout. So the spread is between sessions, not within one, and
the honest quantity is **≈ −1,770 m, n = 3 sessions, range 11 m**. The two-orders-of-
magnitude gap against +0.35 m is the finding; the digits are not. `maxDepthVisited`,
`tilesVisited` and the feature-aware +0.35 m reproduced exactly in all three.

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
| `wrong-world` | seed + 1 everywhere | tile-posts **37,189/38,025 (97.8%)**; interpolate 5,050 m; land-and-sea 57.38%; **worker-path** (8 of 8 workers on the wrong world) |
| `stale-worker` | **one worker of eight** on seed + 1 | tile-posts **8,050–8,064/38,025 (21.2%)** — two of nine tiles; interpolate 530 m or 5,050 m; land-and-sea 95.32–97.14%; **worker-path** (1 of 8). **Session-dependent — see below** |
| `cache-key` | key drops the tile x | **cache-identity** (right tile 4,225/4,225); tile-posts **3,333/38,025**; land-and-sea 57.78–60.08% |
| `feature-blind` | availability ignores features (the Task 4 behaviour) | **feature-availability**: 2 features requested, 0 known |
| `feature-everywhere` | refine to the feature depth globally | **feature-availability**: available 20° from every feature; zoom-cap |

No fault diverges on 100% of posts; the project's standing warning is that 100% has always
meant a broken harness.

**`worker-path` is on those two rows as of this fix round, and was not before.** It computed
`pool.stats().staleWorkers`, printed it in its detail line, and never pushed it into
`problems` — so the one check named for the worker path reported PASS under the fault whose
entire definition is a worker on the wrong world. Nothing in the record claimed otherwise, so
this was a gap rather than a false figure, but it is exactly the shape of a check that looks
thorough in its output and asserts less than it prints. Counts with it: `stale-worker` and
`wrong-world` now fail **5** checks each rather than 4; the other five faults are unchanged.
**It is corroboration, not detection.** The flag is the worker's own confession, set at init
because the fault told it to; a real version-skew bug sets no flag and is caught by
tile-posts-exact, which is why that check and not this one carries those rows' headline
numbers.

**Five of the seven rows reproduce to the digit. Two do not, and the reason is worth more
than the digits were.** Re-run in Task 7 in a second browser session, three runs each:
`flip-latitude` (36,258 posts / 1.87e3 m / 80.16%), `shift-tile` (35,271 / 228 m),
`wrong-world` (37,189 / 5,050 m / 57.38%), `feature-blind` and `feature-everywhere` all came
back identical. `stale-worker` came back **8,064** rather than 8,050, with interpolate
5,050 m rather than 530 m and land-and-sea 95.32% rather than 97.14% — stable across three
runs *within* a session, different *between* sessions. `cache-key` moved the same way on
land-and-sea (57.78%), while its two headline figures, 3,333 and 4,225 of 4,225, held exactly.

Neither is a defect, and neither weakens the fault. Both are **positional**: `stale-worker`
poisons worker index 0, and which of the nine probe tiles that worker is handed depends on
how far the pool's round-robin cursor had advanced — which is moved by whatever the globe
streamed before `check()` was called. `cache-key` serves a row-neighbour, so which neighbour
depends on what is in the cache.

So the counts that are properties **of the fault** — 3,333; 4,225 of 4,225; "two of nine
tiles"; the 21.2% band — are exact and should be held to. The counts that are properties of
**dispatch order** are not, and quoting the two kinds in the same voice is how a measurement
becomes folklore. A future run that reports 8,064 has found nothing wrong.

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


## The record (Task 7)

Everything above is the working notes of six tasks, written as each one landed. This section
is the part that has to survive them: what this viewer guarantees, what it refuses to
guarantee, what it costs, and what is still open. **Every figure in it was re-derived from
the current source or from a run made while writing it, and where a re-run disagreed with
what was written the disagreement is recorded rather than smoothed over.** Four figures
moved.

### What it is, and what it is not

A **read-only window onto a generated world.** Give it parameters, it builds a world in the
engine and draws it. It has no way to change one: no placement, no editing, no anchor tree,
no worldfile, no persistence of any kind. There is nothing to save because there is nothing
a user can alter — the only inputs are the URL parameters, and reloading is the only undo
anyone needs.

**Slice 3 owns placement**, and that boundary is why several things here are shaped the way
they are. `Surface::new` is milliseconds rather than a background job precisely so that a
parameter change can rebuild a world inside one animation frame once there are controls to
change it with. The handle table never reuses a slot precisely so that a worker holding a
handle from before such a change gets an error rather than a different planet. This slice
exercises neither property. It makes sure they are there.

### The offline guarantee, stated exactly

**Nothing leaves the origin.** Three independent lines of evidence, in increasing strength:

1. **Witnessed.** A browser network trace plus `performance.getEntriesByType("resource")`,
   cross-checked against the server's own request log, on a server that proxies nothing and
   has no upstream. 0 off-origin, and still 0 after flying the camera to five widely
   separated points. Re-measured in Task 7 against the page as it ships: **52 requests, 39
   resource-timing entries, 0 off-origin.**
2. **Confirmed independently.** Several hundred rendered frames across the checks, the
   bench and the fault runs, in three separate sessions, on both worlds and under all seven
   faults. The off-origin count has been 0 in every one.
3. **Enforced.** `default-src 'self'`, proved able to refuse **one header apart on the same
   probe**: with the policy, 1 `securitypolicyviolation` and **zero hosts reached**; without
   it, **34 off-origin entries across six hosts**, including plaintext `http://` tile
   requests to Bing. A trace shows a browser *did not* phone home. The policy is why it
   *cannot*.

Witnessing is the weakest of the three and is the one people quote. Quote the third.

**Ion is only the broker.** This is worth being blunt about, because "turn the default
imagery back on" sounds like a rendering decision and is not: `ImageryLayer.fromWorldImagery()`
resolves *through* Cesium ion to **Bing Maps / virtualearth.net** — Microsoft, with its own
terms, and with the key shipped in the bundle. Enabling default imagery here would be a
Microsoft licensing conversation, not a Cesium one. The bundled ion demo JWT is time-limited
besides: its `aud` claim reads `1.145 Release - Delete on November 1, 2026`.

And the limit, restated because it is the easiest sentence in this file to over-claim:
**`script-src 'unsafe-eval'` is required, and "this page cannot eval a string" is not
something this policy says.** See *What this policy does not claim*, above.

### Provenance: what parity proves, and what it does not

Parity proves the **shipped bytes** — the ones a browser loads — reproduce native source
exactly: **53,251 values, 0 divergent**, through the shipped exports on both sides, with a
control (`--mutate seed`) moving **50,778** of them and every group carrying a continuous
height moving entirely. Both were re-run for this task.

**That was green on a stale artifact for several commits.** The committed `.wasm` differed
from its source by five bytes — all of them panic-location line numbers, which never execute
— and parity passed anyway, because the corpus it replayed had been recorded from the same
stale build. Two things that are stale *together* agree with each other and with nothing
else, and no number in the parity output can tell you so.

So parity now refuses to report at all on an artifact that does not match current source,
by importing the freshness check rather than reimplementing it. The lesson generalises past
this file: **a verification and a provenance check are different questions, and a suite that
answers only the first will hold the wrong answer indefinitely without ever going red.**

### What the viewer draws, and what it caps

**Level 12 for generated ground.** A 65-post tile at level 12 has 76.35 m posts, and the
generated field's measured resolution floor is 78.125 m — below it the field is a tilted
plane and further levels add nothing. Level 12 is the first level at or below that floor, so
it is the last level at which zooming reveals ground that was not already there. Above it
`getTileDataAvailable` returns `false` rather than the prototype's `undefined`, which
refines until the tab dies.

**Feature-aware refinement past it**, because authored features do not obey that argument.
`Features::apply` is analytic, so it is resolution-independent *point-wise* and still
**grid-sampling-limited**: the harbour's mole reads exactly +4.00 m at every `resolution_m`
tried, while the level-12 *tile* containing it tops out at **−819 m against a +4 m target**,
because a 60 m mole is narrower than one level-12 post. It needs about level 16. So
availability refines inside a feature's own footprint — the engine's `reach_m` circle, not a
guess — to `post spacing ≤ min(length_m, width_m) / 8`, bounded at 18.

What that buys, measured with the camera 120 m above the harbour, one flag apart:

| | maxDepthVisited | tilesVisited | `globe.getHeight` at the mole |
|---|---|---|---|
| ground cap only (`?fault=feature-blind`) | 13 | 23 | **≈ −1,770 m** |
| feature-aware | **16** | 45 | **+0.35 m** |

against an engine truth of **+4.00 m**, for **16 extra tiles** — enumerated by descending
from the cap, not estimated. Heap ~30 MB either way. (The feature-blind height is read from
whichever coarse tile is loaded at that instant, and moved between −1,762.60 m and
−1,773.59 m across Task 7's runs. The two-orders-of-magnitude gap is the finding, not the
digits.)

**What zoom actually reveals, by scale:**

| from | to | what appears |
|---|---|---|
| whole disc | ~L5 | continents, shelves, the abyssal clamp at −4,600 m |
| ~L5 | L10 (305 m posts) | coastline shape, relief, the shelf break |
| L10 | **L12 (76.35 m)** | the last generated detail — the octave schedule's floor is 78.125 m |
| L12 | L16 (4.77 m) | **only where a feature reaches.** Elsewhere the tile is upsampled, and says so |
| past L16 / L18 | — | nothing. `FEATURE_CEILING` bounds the rule |

### The frame budget, with its populations

The most misusable number in this slice would be "a tile costs N ms", so it is not recorded
as one. Four populations, measured separately, because a coastal tile costs several times a
deep-ocean one **and coasts are what the viewer looks at** — a mean over a uniform sample of
the globe is a mean over mostly ocean and describes nothing anyone will ever see.

**State the statistic.** The coastal penalty is **~3.6× on medians** in the recorded run,
and 5.9× in Task 7's second one (see the table's own notes). The **9×** that appears in
`src/wasm.rs`'s module docs is not any percentile of that table — it lives only between
extremes, and it is wrong as written.

Eight workers buy **5.29×**, not 6.08× and not 8×, because per-tile cost *rises* with
concurrency: eight of them share memory bandwidth and a turbo budget. Task 7's re-run gave
5.83× with the same rising curve. *Five to six times, never eight* is the durable claim.

Reproduce any of it with `window.__wb.bench({ perClass: 160 })`. The default `perClass` is
96 and gives a different, smaller population.

### The `?fault=` mechanism

**Seven faults**, each a plausible wrong implementation rather than a corruption — chosen so
that a check which cannot see it would not have caught the real mistake either.

| fault | caught at |
|---|---|
| `flip-latitude` | 36,258 / 38,025 posts |
| `shift-tile` | 35,271 / 38,025 |
| `wrong-world` | **37,189 / 38,025 — 97.8%**, and worker-path: 8 of 8 |
| `stale-worker` | **8,050–8,064 / 38,025 — 21.2%**, two of nine tiles, and worker-path: 1 of 8 |
| `cache-key` | 3,333 / 38,025, and cache-identity at 4,225 / 4,225 |
| `feature-blind` | feature-availability: 2 features requested, 0 known |
| `feature-everywhere` | feature-availability, and zoom-cap |

**97.8% is the healthy signature, not a shortfall.** `wrong-world` is a different planet and
still agrees on 836 posts, almost all of them the abyssal clamp at −4,600 m: two different
worlds have the same floor. **No fault in this slice diverges on 100% of posts, and in this
project 100% has always meant a broken harness rather than a thorough one.** A fault
reporting 38,025 / 38,025 would be the thing to investigate.

### The lesson that outlives this slice

**Breaking things on purpose found a broken *verifier* nine times here** — more often than
it found a broken implementation, which it never did. And a tenth was found by handing the
branch to a reviewer with a clean clone rather than by breaking anything: the freshness guard
itself (below), whose first act on any machine but the author's was a false alarm. In
order:

1. A network trace that could not have shown traffic even if there had been some. Fixed by
   `?net-probe=1`, which is what made the empty trace mean anything.
2. A pixel check on a canvas whose `clientWidth` and framebuffer disagreed, misregistering
   every ray — 50% agreement, before it was a real finding about anything.
3. `quadtree-depth` passing on a page that had **never rendered**: `maxDepthVisited` 0, and
   0 ≤ anything.
4. `quadtree-depth` again, bounding the *traversal* when the cap governs what is
   **requested** — right answer, wrong quantity, and it failed the first time a camera sat
   somewhere new.
5. `feature-availability` comparing the availability function against **its own** footprint
   list, so `feature-blind` passed by agreeing with itself.
6. `feature-resolves` iterating `availability.footprints`, which `feature-blind` empties —
   a check with nothing to do, reporting success.
7. `quadtree-depth` a third time, reporting 12/0 while the CSP blocked Cesium's blob workers
   and the globe rendered an empty ellipsoid.
8. `feature-resolves` a second time — assertions gated on `compose === "raise"` while a
   carve's posts still counted as work, so an all-carve world passed having asserted nothing.
   The countermeasure built for 3, 6 and 7 could not see it, because it counted heights read
   rather than assertions made.
9. `worker-path` computing `staleWorkers`, printing it, and never failing on it — so it
   passed under `?fault=stale-worker`, the fault named after it.

Three of those — 3, 6 and 7 — are the same bug: **a check counted zero work as success.** So
it was made structural rather than remembered: `ok(name, pass, detail, work)` took a **work**
count, and where one was supplied zero was never a pass.

**Then a fourth walked straight through it, because `work` measured the wrong quantity.**
`feature-resolves` gated its assertions on `compose === "raise"` and counted a `carve`
feature's heightmap posts as work. On a world whose features are all carves it reported a
confident pass having asserted nothing at all — with a work count in the tens of thousands.
Run against the pre-fix code on a carve-only harbour: **PASS, "8,450 heights scanned", zero
assertions**, and a detail line calling the carve "4,604.5 m off target" while passing.
Volume examined and assertions made are different numbers, and it was the second one that
was zero.

**The obvious repair — count assertions instead — is not a repair, and this is the part
worth carrying forward.** `quadtree-depth` makes exactly one assertion and always makes it;
counting assertions there would report a healthy 1 on the very globe that drew nothing, which
is broken-verifier 7 above. Its zero is a *volume* zero (the quadtree walked no tiles);
`feature-resolves`'s zero is an *assertion* zero (the loop ran and asserted nothing).
**Neither quantity contains the other**, so one counter can never see both.

So the fourth argument is now a set of **named witnesses** — `ok(name, pass, detail,
{ "posts compared": n })` — every one of which must be positive or the check reports NOT
EXERCISED, and each check names every quantity whose being zero would make its pass vacuous.
`feature-resolves` names two: `heights scanned` **and** `assertions made`. Eight of the
twelve declare witnesses; the other four are self-guarding, and `worker-path` refuses
`poolFills === 0`, any idle worker, and — now — any worker on the wrong world, by name.

Driven to zero and seen to refuse, on the live page: with the carve arm's counter forced to
zero, `feature-resolves` reports `NOT EXERCISED: 0 assertions made` while still scanning
8,450 heights — the exact case the old counter passed. With the carve assertion inverted, it
fails with "a one-way carve filled instead of cutting". Unmodified, the baselines are
unchanged: **11/0** featureless, **12/0** harbour.

`feature-resolves` also **asserts on carves now**, one-sidedly and for a stated reason:
`CARVE` is one-way (`features.rs` applies it only where `lift < 0`), so the minimum over the
feature's tile either comes down to the target or was already below it, and can never end up
above it. This world is the second case — the −12 m basin sits over abyssal floor near
−4,616 m — so the check now says **INERT** and explains it, instead of printing a 4,604 m
"error" that was never an error.

The rule to carry forward is not "add a work count", and it is not "add an assertion count".
It is: **a check must name every quantity whose being zero would make its pass vacuous**, and
the fault switch is what reveals which checks those are. `?fault=` costs a few dozen lines
and has now paid nine times.

### Three things that are open, not solved

- **`zoom-cap`'s zero-work guard is not demonstrated end to end.** The other guards have
  been driven to zero and seen to refuse. This one cannot be: `zoom-cap`'s work is the number
  of levels it interrogates, and driving that to zero means an undefined or NaN `maxLevel`,
  which kills the page during boot long before the check runs. The guard is **defensive and
  unproved**. It is written down here rather than quietly counted among the ones that were
  demonstrated.
- **No human has watched this viewer run.** Across three earlier sessions the browser pane
  never composited and every frame was hand-driven through `viewer.render()` — which is
  exactly the condition that produced broken-verifier #3 above. Task 7 closed half of this:
  the page was driven under a browser that **did** composite, and a full-disc frame shows a
  continent, shelves and ocean where the engine puts them, with a coastline resolving at
  60 km. But it was a headless software rasteriser with no display attached, which is why the
  millisecond tables above are annotated the way they are, and **nobody has yet sat in front
  of this thing and moved the camera.** The measurements are strong. The gap is that no one
  has *looked*.
- **There is no CI.** Nothing runs the parity harness, the freshness guard, the build-shape
  self-tests or the twelve checks automatically. Every figure in this file is defended by a
  command someone has to remember to type, and the stale-artifact incident above is precisely
  what that costs. This is the same standing gap recorded in slice 1p, unchanged.

### Everything that must be re-run when this changes

```
# engine, FIVE configurations -- the two feature flags are independent
cargo test -p worldbuilder-engine                        # 409
cargo test -p worldbuilder-engine --no-default-features  # 409
cargo test -p worldbuilder-engine --features python      # 409
cargo test -p worldbuilder-engine --features wasm        # 439
cargo test -p worldbuilder-engine --features python,wasm # 439

# the artifact
cd viewer
npm run check:wasm                  # is the shipped .wasm built from current source?
npm run build:wasm:self-test        # can the shape check fail?      (327-byte artifact)
npm run build:wasm:stale-self-test  # can the fingerprint fail?

# ...and the same question asked from OUTSIDE this working tree, which is the one the
# guard failed for six commits. `core.autocrlf` must be set ON THE CLONE: passing it to
# `git clone` as `-c` leaves the global value in place and both arms measure the same
# thing. Both must exit 0.
for mode in true false; do
  git clone -n -b slice-2b-viewer . /tmp/clone-$mode
  git -C /tmp/clone-$mode config core.autocrlf $mode
  git -C /tmp/clone-$mode checkout slice-2b-viewer
  ( cd /tmp/clone-$mode/viewer && node scripts/build-wasm.mjs check )   # exit 0, both
done

# parity, through the shipped exports, with its control
cargo run --release -p worldbuilder-engine --example parity_dump --features wasm > native.txt
node crates/worldbuilder-engine/parity/parity.mjs native.txt               # 53,251 / 0
node crates/worldbuilder-engine/parity/parity.mjs native.txt --mutate seed # 50,778 divergent

# the page: both worlds, then every fault
npm run serve
#   /                  -> 11 checks, 11 passed
#   /?harbour=1        -> 12 checks, 12 passed
#   /?harbour=1&fault=flip-latitude | shift-tile | wrong-world | stale-worker |
#                        cache-key | feature-blind | feature-everywhere
#                     -> each must FAIL, at its own count above
```

All of it was run for that task on Windows 11 (10.0.26200), x86_64-pc-windows-msvc,
cargo 1.98.0 / rustc 1.98.0, Node v22.17.0, Chrome 151 (headless). Every command exited 0
except the fault runs, which are supposed to report failures, and did.

**The relief slice added a node suite and a second provider, so add these** — every line below
was run for [the relief record](#the-relief-imagery-layer), on this host, and both commands
exited 0:

```
cd viewer
npm test          # 57 tests, 57 pass -- relief.js, relief-provider.js, pool.js,
                  # tile-worker.js and panel-fields.js. No browser needed.
npm run check:wasm

# and the pages, which need a browser that actually composites:
#   /?fly=10,20,9000000            relief on: 61 tiles at level 3, 0 main-thread rasters
#   /?fly=10,20,9000000&workers=0  the A/B baseline: 61 main-thread rasters, and it fails
#                                  worker-path, cache-identity and quadtree-depth BY
#                                  CONSTRUCTION -- it is not a supported configuration
#   /?fly=10,20,9000000&sse=1      155 tiles at level 4 -- the expensive detail setting
#   /?relief=0                     must still differ from the relief-on digest, and the
#                                  pool and ?workers=0 digests must still be identical
```


---

# The relief imagery layer

The slice after slice 2b, and the one that changed what the planet looks like. It is
**viewer-only**: no engine change, no new wasm export, no rebuild. It adds a second provider
beside the terrain one — a Cesium `ImageryProvider` that generates a shaded-relief,
slope-coloured raster per tile from the same engine, in the same worker pool.

It exists because of two sentences from the repository's owner:

> one looks like minecraft (ours) and gpts looks like an actual image from space

> when I zoom in the resolution doesnt increase. it just looks blurry.

Read *[What still does not look like a photograph](#what-still-does-not-look-like-a-photograph)*
before believing this section solved either one. It moved the second a long way and the first
only partly, and the reason the first is hard is physical rather than a bug.

**Every figure below was measured while writing this section**, on this host, from the current
source or from a run performed here — never copied from a task report. Where a run disagreed
with what an earlier task recorded, both are shown and the disagreement is named. The commands
and populations are in
*[Everything this section's figures came from](#everything-this-sections-figures-came-from)*.

## The measurement that motivated it: the coastline is a polygon through 78 km posts

At the whole-planet view the globe draws sixteen terrain tiles at levels 1 and 2. A 65-post
heightmap tile at level `L` on this world's 6,371,000 m radius samples every
`π · 6,371,000 / 64 / 2^L` metres — `postSpacingM(level, size, radiusM)` in
`public/app/terrain.js`, which computes it rather than quoting it:

```
level 0 -> 312,735.73 m    level 2 ->  78,183.93 m
level 1 -> 156,367.87 m    level 3 ->  39,091.97 m
```

**Every coastline in a whole-planet screenshot is a polygon through points 78 km apart.** That
is the whole of the "low-poly" look, and it is a *resolution* problem rather than a shading
one — which is why the appearance work that preceded this slice (lighting, sky limb, a ramp
with depth and snow bands) did not touch it.

Where refinement stops is not a choice either. The live provider reports
`getLevelMaximumGeometricError(0) = 77,067.34 m` — Cesium's own
`getEstimatedLevelZeroGeometricErrorForAHeightmap` for a 65-post tile on a two-tile level 0 —
halving every level, and at orbital distance the level-2 error lands under the default
screen-space threshold. **Nothing is broken; the picture is exactly what the defaults
predict.**

**Brute force is not the answer.** Reaching ~1 km detail across a visible hemisphere means
level 7–8 *terrain*: thousands of meshes to build, upload and draw where there are now sixteen.
The cost is not in generating heights.

## Why "shade at a different frequency than you tessellate" is the fix

Three facts, each verified on the running viewer rather than restated.

**1. The terrain provider cannot give you relief shading at all — ever.** Measured live on the
default page:

```
viewer.terrainProvider instanceof Cesium.CustomHeightmapTerrainProvider   true
viewer.terrainProvider.hasVertexNormals                                   false
viewer.scene.globe.enableLighting                                         true
```

`HeightmapTerrainData` has no normals path anywhere in Cesium; oct-encoded vertex normals exist
only on `QuantizedMeshTerrainData`. So `GlobeFS` falls back to `czm_geodeticSurfaceNormal` —
**the ellipsoid normal** — and the mountains are lit as though the planet were a perfect smooth
sphere. `hasVertexNormals` is `false` *while lighting is on*, which is the whole point: this
project turned `enableLighting` on expecting relief and got nothing, for this reason. It also
silently disables Cesium's own `SlopeRamp` and `AspectRamp`, which read
`czm_octDecode(encodedNormal)`; `ElevationRamp` works only because `v_height` comes straight
off vertex position, **which is exactly why the picture before this slice was colour-by-height
and nothing else.**

**That is why the shading is baked into an imagery raster.** It is not the best of several
options; on this provider it is the only one.

**2. Imagery level equals terrain level — it is not clamped to it.** Measured on the same
settled page: `globe._surface._debug.maxDepthVisited` and the relief provider's own
`maxLevelRequested` are **the same number** (2 at the pane's own size, 3 at an emulated
1400x900). Cesium picks the imagery level from `getLevelWithMaximumTexelSpacing` against the
*terrain* provider's geometric error with a hardcoded `errorRatio` of 1.0, clamped only to the
imagery provider's own min/max. One imagery tile per terrain tile — but **256 texels across a
rectangle the geometry samples with 65 posts**, a free ~4x linear colour resolution over the
same tile budget. And the raster is sampled from the *true field*, where `Globe.material` only
ever sees values interpolated across those 78 km posts.

**3. Raster size is a batching knob, not a detail knob — and this is why widening tiles was
never going to work.** Screen-space post density is invariant to heightmap width, and the
imagery tile is invariant the same way. Measured here at the default orbital camera
(`?fly=10,20,9000000`), 8 workers, one flag apart:

| `?reliefSize=` | relief tiles | deepest level | m/texel | total texels |
| --- | --- | --- | --- | --- |
| 128 | 244 | 4 | **9,850** | 4.00 M |
| **256** (default) | 61 | 3 | **9,811** | 4.00 M |
| 512 | 15 | 2 | **9,792** | 3.93 M |

**A 4x change in tile width moves the tile count by 16x and the metres per texel by 0.6%.** A
wider tile lowers the geometric error, Cesium refines one level less, and it lands on the same
ground spacing. There is no quality/cost trade here and it must not be read as one; the only
knob that moves detail at this camera is `maximumScreenSpaceError`.

So: **keep the mesh coarse — it only has to hold the silhouette — and put the detail in a
per-pixel raster generated at a higher frequency from the same height function.** Three shipped
planet renderers (Outerra, Infinity, Elite Dangerous) say publicly that they do exactly this.
One honesty note carried from the plan's research: no graphics paper appears to quantify
"shading dominates terrain-shape perception". The nearest rigorous evidence is cartographic —
the terrain-reversal effect, where illuminating from the lower right makes mountains read as
valleys **with no change to the elevation model at all**. The practice is well supported; the
perceptual claim is not measured.

## The layer shipped flat, and the check that shipped before it said so

This is the part worth keeping, because the failure was caught by measurement rather than by a
person looking at the screen.

The provider went in, `?relief=0` was proved byte-identical, and **the hillshade was invisible
at every zoom level.** Method: render the same tile twice through `reliefTile` — once normally,
once with `ambient = 1`, which sets `shade === 1` and leaves the height/slope colour untouched —
and take the per-texel ratio. That ratio *is* the shade factor. As shipped, it was a
**near-constant 19% darkening with a spread of about 0.004, flat from level 5 to level 12**. On
a mid-tone that is well under one luminance unit. A constant darkening is not relief, and it did
not improve with zoom — so the owner's second complaint was not answered at all.

**The check that shipped one task earlier was honest about not covering that case.** Its
`hasStructure` threshold (`luminance sd >= 2`) had been reported, with numbers, as catching a
*blank* raster and not a *weakened* one: a mutant that blended 80% of the relief away still
cleared it. One task later the shipped raster was exactly that thing — structurally present,
visually useless — and the threshold refused those tiles from level 8 down while the node suite
stayed green on a level-2 fixture where colour supplied the spread. **A threshold tuned until it
looked decisive would have hidden this.**

### And the baseline it was measured against flattered the result

Re-run here, on the tile containing −9, 65 at size 256, levels 5–12, through the identical ratio
method:

| level | `zFactor` 1 mean | `zFactor` 1 sd | constant-shade control sd | shipped mean | shipped sd |
| --- | --- | --- | --- | --- | --- |
| 5 | 0.8094 | 0.0039 | 0.0028 | 0.7998 | **0.0415** |
| 6 | 0.8086 | 0.0043 | 0.0031 | 0.7759 | **0.0431** |
| 8 | 0.8078 | 0.0028 | 0.0019 | 0.7416 | **0.0430** |
| 9 | 0.8074 | 0.0036 | 0.0024 | 0.7280 | **0.0492** |
| 10 | 0.8072 | 0.0040 | 0.0025 | 0.7256 | **0.0617** |
| 11 | 0.8069 | 0.0039 | 0.0023 | 0.7075 | **0.0570** |
| 12 | 0.8073 | 0.0034 | 0.0023 | 0.7087 | **0.0400** |
| **L5–12 mean** | **0.808** | **0.0037** | **0.0025** | **0.741** | **0.0479** |

0.8096 is `AMBIENT + (1 - AMBIENT) · sin(45°)` **exactly** — the shade of perfectly flat ground.
The `zFactor: 1` column reproduces the shipped-flat baseline through today's code.

**The middle column is the correction, and it belongs in the record rather than in a ledger.**
It is a synthetic control: take the *unshaded* raster, multiply every channel by the exact
constant 0.8096, round to bytes, and measure the ratio the same way. Its true shade spread is
**zero by construction**, and it still reports **sd 0.0025** — two thirds of the baseline's own
0.0037. **The original "near-constant 19% darkening" was, if anything, understating how flat it
was**, because a baseline taken through the same 8-bit path as the result is a baseline that
flatters the result. The shipped raster clears that quantisation floor by **19x**.

(One trap found while measuring this, worth a line: the `zFactor: 1` and shipped arms must each
be divided by **their own** unshaded raster. Dividing both by the shipped one reported an
apparent sd of 0.0313 for the `zFactor: 1` arm — six times too high — because the two arms draw
different colours and quantisation is a property of the colour, not of the shade.)

## The z-factor, and the lie it tells on purpose

The cause is the terrain, not the shading code. Slope-angle distribution of land texels, from
`reliefTile`'s own central differences over its own margined grid, three probe tiles, size 256:

| level | spacing | −9,65 p50 | p90 | max | 20,10 p50 | p90 | max |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 2 | 19,623 m | 0.033° | 0.129 | 0.90 | 0.016 | 0.093 | 0.41 |
| 3 | 9,811 m | 0.033° | 0.122 | 0.93 | 0.037 | 0.170 | 0.43 |
| 5 | 2,453 m | 0.208 | 0.460 | 1.19 | 0.211 | 0.384 | 0.76 |
| 8 | 307 m | 0.776 | 1.064 | 1.63 | 0.326 | 0.591 | 1.23 |
| 12 | 19 m | 1.194 | 1.527 | **1.85** | 0.304 | 0.578 | 1.12 |

**Nothing on this planet is steeper than 1.9° at any raster spacing.** A hillshade's response to
a 1° slope is a 1° tilt of the normal — under half a luminance unit. Every desktop GIS hillshade
carries a **z-factor** for exactly this reason. `Z_FACTOR = 25` multiplies both gradients before
the normal is built, and it was chosen by sweep against an assertion that already existed and
could already fail (`hasStructure`), not by eye.

**It is one constant across every level, on purpose.** A level-dependent z would shade two
adjacent tiles differently wherever the quadtree straddles a level — which it does constantly
during a descent — and that is a seam. What legitimately grows with level is the terrain: finer
sampling resolves steeper local faces. **That is the answer to "when I zoom in it just looks
blurry", and it is why the spread holds across the range** — sd 0.0400 to 0.0617 over eight
levels, minimum at level 12, level 5 within 4% of it.

**Say plainly what it is: at level 12 the raster renders a median 1.2° slope as a 27° one.**
This is a legibility choice and not a realism one, it is the largest single lie `relief.js`
tells, and it is named in that file's own module doc rather than buried.

## The cost of the default

**Before and after the worker-pool move**, measured on one host, same world, same camera, one
flag apart — `?workers=0` is the before, the shipped default is the after. Host: the agent
browser pane, Chromium 148, ANGLE Intel UHD D3D11, 32 logical cores, viewport emulated to
1400x900, default orbital camera `?fly=10,20,9000000`, **61 relief tiles at levels up to 3**,
`reliefSize=256`, 8 workers, frames driven by hand.

| | `?workers=0` | default (pool) | |
| --- | --- | --- | --- |
| main-thread rasters | **61** | **0** | the counter, not the picture |
| main-thread rasterisation, per tile | **178.1 ms** | **0.319 ms** | **558x** |
| main-thread rasterisation, total | **10,864 ms** | **19.3 ms** | |
| worst single main-thread block from a tile | **346.4 ms** | **0.76 ms** | |
| worker rasterisation, total | 0 | ~9.9–13.2 s | the cost **moved**, it was not removed |

Eleven seconds of blocked main thread for one orbital view is what "the camera stops
responding" means quantitatively. **The worker move was a prerequisite for shipping the layer
on by default, not an optimisation.** On the other host, over a fixed eight-second descent from
orbit, the same change took the page from **53 long tasks totalling 4,372 ms to zero, and 25.7
to 46.5 fps** — a frame-time claim this host cannot reproduce and does not repeat as its own
(see *[two hosts](#two-hosts-and-what-neither-of-them-can-do)*).

**The picture is unchanged across the move, and that is proven by digest rather than by eye.**
SHA-256 of the rendered PNG after 20 driven frames at a pinned viewport (1000x800), pinned
camera and **pinned frame time** — `Scene.render()` with no argument defaults its frame time to
`JulianDate.now()`, so pinning `viewer.clock` alone does nothing and every capture is lit
differently:

| configuration | SHA-256 |
| --- | --- |
| `?relief=0` | `2f6ac98ac10dcdb208d1e8db56dda8f2974be0e57dd18276bb968c874f43e7ed` |
| relief on, **pool path** | `79f11efe1ea5fab354b2d9dbc0cc4cb3a9b0987e85b3107a2aa3d129c159422c` |
| relief on, **`?workers=0`** | `79f11efe…` — **identical** |
| relief on, pool path, second and third capture | `79f11efe…` — repeatable |

Row 1 differs from rows 2–4, which is the **positive control**: the instrument is sensitive to
exactly the thing being claimed unchanged. (A digest is only comparable to another taken on the
same GPU at the same viewport; these are this host's.)

### Which default ships, what the other one buys, and how to reach it

**`sse=2` ships, unchanged, and it is the cheap one.** `?sse=1` is the *only* knob that buys
detail at the whole-planet view — `?reliefSize=` provably cannot, per the invariance above.
Measured here:

| | relief tiles | deepest level | m/texel | worker CPU |
| --- | --- | --- | --- | --- |
| **`sse=2`, shipped** | 61 | 3 | 9,811 | 9.9 s |
| `sse=1.5` | 83 | 3 | **9,811 — no change** | 17.1 s |
| `sse=1` | 155 | **4** | **4,906** | 26.0 s |

- **`sse=1` buys one imagery level — 4.9 km per texel instead of 9.8 — for 2.5x the tiles and
  ~2.6x the worker CPU** (3.1x on the host that chose the default). It is worth turning on if
  you are *looking at* the planet rather than flying over it.
- **It is not a relief knob.** `maximumScreenSpaceError` refines the terrain mesh too, so its
  cost is not confined to the thing it improves. A global rendering default should not be
  changed from a relief-specific argument.
- **It costs most on hardware nobody has measured.** 26 s of worker CPU across 8 workers on 32
  cores is comfortable; the same work on a four-core laptop is not, and a default has to be safe
  on the machine you did not test.
- **`sse=1.5` is a dead middle**, and that is a finding: 36% more tiles and **not** one more
  level, because the level Cesium picks is a step function of this value. The only two settings
  worth having are 2 and 1.

**A cost setting nobody can find is a setting that does not exist**, so it is in the
always-visible status line, verbatim as it prints today:

```
… | reliefLayer=256px cap=12 workers paint=off | sse=2 (?sse=1 for one more level, ~3x cost) | fault=none
```

Under `?workers=0` that field reads **`MAIN THREAD`** instead of `workers`, so a silent fallback
to the slow path is visible rather than merely felt. **`?workers=0` is a measurement baseline,
not a supported configuration** — it fails three browser checks by construction, and
`verify.js` says so in its own text.

## The panel-default family, closed by a check

Two default bugs were fixed here, and **the check that closed the family was worth more than
either fix, which is now evidence rather than a preference.**

- The `ElevationRamp` window had been narrowed to −7,000..2,400 m without moving the gradient's
  stops, so the "strand" stop at 0.60 landed at **−1,360 m**. **The default path had been
  drawing the coastline 1.4 km below sea level.** The arithmetic explains why it was not
  careless: `0.60` of the *previous* −9,000..+6,000 window is exactly 0 m. The stops were right
  until the window moved. So the fix that matters is not re-placing the fraction — that would
  produce a correct picture with the same shape of bug — but making the placement a function of
  the datum, which `wb_elevation_m` fixes at 0 by construction.
- The radius slider's `min: 1e6, step: 1e5` could not express its own default of `6371000`; a
  range input snaps to `min + k·step`, so the control read back **6,400,000 m** and pressing
  generate built a different planet. It survived every screenshot ever taken of it because the
  readout was `(value / 1e6).toFixed(1)` Mm, at which precision both values print "6.4 Mm". It
  is three decimals now and reads 6.371 Mm.

`public/app/panel-fields.js` now holds **one** table — `DEFAULT_WORLD`, the harbour, the ramp
window, the ramp stops, and the travel of all nine range inputs — which `controls.js` builds
sliders from and `main.js` draws its gradient from. Neither restates a default.
`panelFieldFaults()` asserts, for every range input, that its default is a value the slider can
actually produce, and **running it immediately found a fourth member nobody had reported**:
`rampMax`, `min 500 step 250`, cannot express 2,400, so the ramp's top had silently been 2,250
or 2,500. That is not a thing anyone can see, which is why nobody saw it.

The red proof is **kept as a test**: both as-shipped mis-stepped rows are asserted to be
refused, by message, so the guard cannot quietly stop being able to fail.

## The mountain sliders, and why they are tectonic and not relief

The owner asked for two knobs, in their own words — *"1 to raise and lower mountains and one to
make more mountains and less as desired"* — and then, looking at the finished relief work,
*"we still have no mountains. why is it so hard to make mountains?"*

**The answer was that the peak on their own world is 98.9% tectonic.** Measured on seed
123925603, radius 4,500,000 m, 28 plates, land fraction 0.16 — the world from their screenshot —
over a 0.5-degree global grid (720 x 359 = 258,480 sites) refined to 0.05 degrees around the
coarse maximum, through the Python wheel and again natively: **highest point 1,454.04 m,
structural 1,437.81 m, detail 16.24 m.** No relief parameter could ever have moved it, which is
what the two `NOT_WIRED` entries that came off the panel had been saying all along.

And the ramp is a constant, not an accident. `CONTINENT_COLLISION_M = 1500` over
`CONTINENT_COLLISION_WIDTH_M = 400_000` is a **0.375% grade** at the profile's own scale, and a
**1.787% steepest 2 km step on the flank** on that world. Real ranges run 3–8%. "Wheelchair
ramps", months of work ago, was an accurate reading of a constant.

### The calibration

Same population and world; grade is the **steepest single 2 km step on the flank**, 12 bearings
walked out from the peak to 250 km. Measuring across the summit measures the one place a
mountain is flat, and the first version of the probe did exactly that and reported the opposite
of the truth. Host: this machine, native release build, `src/bin/mountain_probe.rs`.

| collision x width | peak | grade |
| --- | --- | --- |
| **1,500 m / 400 km — canonical** | 1,454.0 m | **1.787%** |
| 1,500 m / 100 km | 1,377.6 m | 2.575% |
| 3,000 m / 400 km | 2,500.0 m | 2.677% |
| 3,000 m / 150 km | 2,441.1 m | 2.852% |
| 6,000 m / 150 km | 4,551.5 m | 4.936% |
| **6,000 m / 100 km** | 4,540.5 m | **7.030%** |

**Amplitude alone is not enough, and the table says so**: 3,000 m at the canonical 400 km is
2.677%, barely above canonical, while the same 3,000 m at 150 km is 2.852% and 6,000 m at 100 km
is 7.030%. Height comes from amplitude; **steepness comes from the pair.** A panel offering only
height would let you raise a 4 km peak that still looked like a ramp. That is why there are two
sliders and not one.

### The count slider had no table, so one was made

`continental_blend` is the width of the oceanic-to-continental transition in `continental_with`,
and it decides how readily a margin runs the **collision** profile at all. The probe above never
varied it, so Task 4 measured it the way a count has to be measured — by counting sites, not by
maximising a peak. Same world, same 0.5-degree grid, at 6,000 m / 150 km:

| blend | 0.01 | 0.05 | **0.10** | 0.20 | 0.30 | **0.45** | 0.60 | 0.80 | **1.00** | 1.50 | 2.00 | 4.00 | 8.00 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| sites > 1,000 m | 964 | 949 | **925** | 847 | 757 | **618** | 496 | 402 | **332** | 244 | 212 | 153 | 126 |
| peak (m) | 5,798 | 5,798 | 5,798 | 5,778 | 5,302 | **4,552** | 3,917 | 3,392 | 3,077 | 2,667 | 2,473 | 2,195 | 2,062 |

**The knob runs the opposite way to its name.** `collision = inboard * outboard` and each side is
`smoothstep((value / blend) * 0.5 + 0.5)`, so a *narrower* transition lets a genuinely
continental side saturate to 1 while a wider one drags every margin towards a diluted half. More
mountains is a **smaller** blend, and the widget's position is negated so that dragging right is
more.

The travel is **0.10 to 1.00**, and both ends are where the measurement stops being informative.
Below 0.10 the counts saturate — 925 / 949 / 964 for a tenfold narrowing — into the hard-test
regime `CONTINENTAL_BLEND`'s own doc records, where *"the ground jumped five hundred and fifty
metres wherever a margin crossed it"*. Above 1.00 the curve flattens (332 to 212 over the next
whole unit) as the ramp grows past the `[-1, 1]` range `fbm` can produce at all.

### The three controls

| panel row | field | travel | step |
| --- | --- | --- | --- |
| **height** | `continent_collision_m` | canonical 1,500 m **up to** 6,000 m | 100 m, 45 positions |
| **steepness** | `continent_collision_width_m` | canonical 400 km **down to** 100 km | 10 km, 30 positions |
| **count** | `continental_blend` | 1.00 (fewest) **through** canonical 0.45 **to** 0.10 (most) | a ninth of canonical, 19 positions |

Both width sliders run *away* from canonical in one direction only, because canonical is already
the widest a centred profile may be: `MAX_TECTONIC_RANGE_M` is 420 km and beyond it a margin is
not evaluated at all, so a wider profile would be **truncated rather than faded** — a cliff. The
boundary refuses those rather than clamping them, and the two *offset* profiles get tighter
ceilings than the centred ones for the same reason (a bump centred 70 km inboard still carries
weight out to `70 km + width`, so coastal uplift tops out at 350 km and the island arc at
360 km).

**No slider is bound to `island_arc_m` or `island_arc_width_m`.** Task 1 proved seven of the nine
fields are read by perturbing each by one ULP across three fixtures, and its tests state
explicitly that those two have **no coverage**. A control with no evidence that the path reads it
is a control that might do nothing.

### Nothing is written down twice, and position 0 is exact

There is **no tectonic default literal anywhere in `viewer/`** — not 1500, not 400000, not 0.45.
All three sliders are anchored on `wb_tectonic_preset`'s answer, read across the boundary at
boot, and `viewer/test/tectonic-params.test.mjs` asserts the three literals appear in neither
`controls.js` nor `main.js`, and appear in none of `tectonic-params.js`'s value-producing
functions.

Position 0 maps to canonical **bit-for-bit**, and that is load-bearing rather than tidy:
`tectonicToParams` drops every field still equal to canonical, so an untouched panel writes no
tectonic parameter and the reload takes the engine's `None` path. A position-0 value one ULP off
would be written into every shared link and would take the untouched viewer off the default
path. **`canonical * (n / 9)` is exact at n = 9 and `canonical * n / 9` is one ULP out** — found
by the engine-side test failing, not by inspection.

### The default path did not move, proved by digest

SHA-256 of the rendered PNG after 20 driven frames at a pinned viewport (1000x800), pinned camera
(20°E 10°N, 9,000 km) and **pinned frame time** — `Scene.render()` with no argument defaults its
frame time to `JulianDate.now()`, so pinning `viewer.clock` alone does nothing.

| configuration | SHA-256 |
| --- | --- |
| `?relief=0` | `2f6ac98a…` — **the value this file recorded before the mountains slice** |
| default path (relief on) | `79f11efe…` — **the value this file recorded before the mountains slice** |
| default path, second capture | `79f11efe…` — repeatable |
| `?mtnHeight=6000&mtnWidth=100000` | `da0bc39c…` — **moves** |
| `?mtnCount=0.09999999999999999` | `6d01c824…` — **moves** |

Rows 1–3 are the claim; rows 4 and 5 are the **positive control**, and there are two of them
because the count slider's travel was chosen by this slice rather than read from a table. A
digest is only comparable to another taken on the same GPU at the same viewport; these are this
host's.

**One caveat, recorded because it nearly went unnoticed.** These captures were taken in a working
tree that another session was committing into. Between the second and a later capture, commit
`1999313` ("Make shade a colour rather than a brightness") changed `relief.js` and the default
path's digest became `12f42dd1…`. Reverting *only* `relief.js` to its pre-`1999313` content, with
every mountain-slice change still in place, returns exactly `79f11efe…` — which is what
attributes the move to that commit and not to this one. `?relief=0` never moved at all, because
it does not run `relief.js`'s shader.

### Three more sliders, a preset, and two fields with no widget

The three sliders above were the first half of the slice and they shipped deliberately early:
the owner had twice said there were still no mountains, and the honest reason was that the task
before them was *required* to change nothing. Giving them the knobs before choosing a preset for
them was the point.

The second half is the **structure field** -- what turns a smooth blade into a range -- and it
needed the channel widened twice. `WB_TECTONIC_STRIDE` went **9 -> 14 -> 16**, and for a whole
task none of the structure work was reachable from the browser at all: it existed in the engine,
was measured in the engine, and `decode_tectonic` filled its fields from `canonical()` with a
comment at that exact line saying what a later task had to do. **A feature that has not crossed
this boundary does not exist as far as the owner is concerned**, and saying so in a comment at
the line that drops it is the difference between a stop and an omission.

| panel row | field | travel | positions |
| --- | --- | --- | --- |
| **vergence** | `collision_asymmetry` | canonical 1.00 **up to** 3.00, a quarter a step | 0…8 |
| **structure** | `structure_depth` | canonical 0.0 **up to** 0.9, a tenth a step | 0…9 |
| **massif size** | `structure_wavelength_m` | canonical 120 km **down to** 40 km, 20 km a step | 0…4 |

Labelled for what they do to the picture rather than for what they are called in the engine,
because "collision asymmetry" and "structure depth" name mechanisms and the owner is choosing
between a smooth blade and two parallel belts of separate massifs.

**`massif size` carries one dead position and it is the one that is required.** The measured
working band is 40-80 km; at 120-250 km the summit count falls back to 0-4 at every depth. But
position 0 has to be canonical **bit-for-bit**, because `tectonicToParams` drops every field
still equal to canonical and a position-0 value one ULP off would write a wavelength into every
shared link and take an untouched viewer off the engine's `None` path. So the travel begins at a
value that does nothing, and stops well short of the 250 km that also does nothing. The same
reasoning is why `structureDepth` is `position / 10` and not `position * 0.1`: `0.1 * 7` is
`0.7000000000000001` and the preset's depth is `0.7`, so the multiplied form gives **a slider
that cannot express the value its own preset button sets** -- the panel-default defect arriving
through a new door for the fourth time.

### The preset crosses as sixteen numbers and never as a name

**ranges preset** reads `wb_tectonic_preset(WB_TECTONIC_RANGES)` and puts every field on the
control that owns it. There is no tectonic literal anywhere in `viewer/`:
`tectonic-params.test.mjs` strips comments out of `controls.js` and `main.js` and asserts that
`6000`, `100000`, `0.7`, `80000` and `300000` appear in neither. The panel's slider anchors and
its preset button are the same export's answer, so `tectonics.rs` stays the only place the
numbers live.

**Two of the sixteen have no widget, and they are shown rather than hidden.** The suture pair
(`suture_count`, `suture_spread_m`) is jointly constrained -- a count slider at the canonical
spread is a height knob, which the engine now refuses outright -- and its useful setting is a
*point*, not a travel. The warp pair (`margin_warp_m`, `margin_warp_wavelength_m`) has a
sharper reason: **a slider was built for it and the panel's own check went red at 40 km.**
`margin_warp_m` is jointly constrained with the steepness slider through
`collision_reach_m()`, and at the panel's widest steepness -- canonical's 400 km -- the 420 km
range gate leaves 20 km of room. A control most of whose travel turns the viewer blank is worse
than no control. The honest fix is a travel that depends on another slider's position, which
this travel model cannot express and which is its own task.

So both pairs are **driven, carried and read out**: the preset sets them, the query string
carries them (`?mtnBelts=2&mtnBeltSpacing=100000&mtnWander=80000&mtnWanderWave=300000`), and the
panel says *"2 parallel belts, 100 km apart · wander ±80 km over 300 km"*, or *"· no wander (the
margin is a great circle)"* when it is off. A preset that changed something the panel never
mentioned would be a preset the owner cannot reason about.

**The status line names the whole block**, because that line is what a screenshot carries as its
caption: `mtn 6000 m / 100 km blend 0.450 verg 2.00 belts 2x100km struct 0.70@80km wander
80@300km`. It named three of eight fields when the block had eight, which would have said
"canonical" about a world whose entire shape had changed.

**And the panel can now produce a block the engine refuses**, which it could before this slice
finished too: press **ranges preset**, then drag steepness back to 400 km, and the collision
profile reaches 535 km against a 420 km gate. `admissibleNote` asks `wb_tectonic_check`
**through the engine** -- never a reach re-derived in JavaScript -- and says so before generate.

### What the owner actually gets when they press the button

**The height slider says 6,000 m. The ground delivers 3,034.6 m**, at a 9.293% flank grade, with
ten summits and two across-range crests. `structure_depth` costs 22% of the amplitude and the
warp a further 9%, and both multiply the number the slider names. That is a real gap between a
control's label and its result, and the panel's own note carries the reason
(*"Structure carves the delivered peak down by up to a fifth; 40-80 km is where it bites."*)
rather than leaving the owner to discover it -- and it carries it **without a number for the
depth**, because a literal there is a literal the "no tectonic number written twice" test would
have to allow through.

**A figure to distrust if you meet it elsewhere:** several documents in this slice quote the
preset as delivering **3,323.8 m**. That was the number before the warp shipped, and it is now
the value of `ranges()` **with the warp switched off**. Re-measured for this write-up, the
shipped preset delivers 3,034.6 m.

### The pictures, and the one thing this slice could not photograph

The preset's pictures are real: Cesium drawing the shipped `.wasm`, at 1600x900 in headless
Chromium against the running dev server, taken by **clicking the panel** rather than by composing
a query string -- canonical against the preset on one camera, and the same range from 900 km up.
They show two parallel belts of separate massifs with a ridge-and-valley interior.

**The warp's pictures are not.** The task that shipped the along-margin warp could not obtain a
legible viewer frame: in its browser pane the globe rendered only while the pane was fronted, the
relief-layer imagery would not refine past level 7 however long it waited, and every frame came
out too blurred at 800x450 to show a belt's shape. Its before/after pair is therefore a
`Surface::elevation_m` raster from `mountain_survey.rs` at a stated 8x vertical exaggeration --
**the same physics the viewer draws, but not the viewer**. The channel and the panel *were*
driven live on the owner's own world (the query string round-trips both warp fields, the readout
says *"wander ±80 km over 300 km"*, and `wb_tectonic_check` accepts the preset and refuses a
canonical-width 80 km warp), so what is missing is a picture and not a verification. **The
1600x900 headless harness that took the preset's shots is the fix, and nobody has run it on the
warp.** Recorded here so that the four rasters are not mistaken for screenshots.

## What still does not look like a photograph

The most valuable section here, and the reason this slice is not "done".

**Say this first: the reference image could not be found.** Nothing in `docs/`, `.superpowers/`
or any scratchpad is the picture the owner compared against; the plan records it only through
his words. **So the comparison below is against named qualities, not against pixels, and no
pixel comparison happened.**

**And this second: the world does not have mountains, and nothing here gave it any.** Mountain
height on this generator is **tectonic**, not roughness. The engine's own `detail.rs` records
the measurement: the most extreme corner of an 80-configuration roughness sweep tops out at
**161.34 m of relief on the peak population and 82.10 m on land** over a 2 km run, where
Hammond's *low mountains* begin at **300 m**. The roughness spectrum's measured ceiling sits
inside the *hills* band and never reaches mountains. **No z-factor and no colour ramp changes
that.** The relief layer makes low hills legible; it does not make mountains, and this write-up
must not be read as saying it did.

Then, itemised:

1. **The whole-planet view is the weakest case, and the reason is PHYSICAL rather than a bug.**
   At the default orbital camera Cesium asks for imagery levels 2–3 — **9.8 to 19.6 km per
   texel**. Measured at exactly those spacings, this planet's **median land slope is 0.033°**
   (0.033° and 0.037° on the two large-land probes at 9.8 km; 0.091° on a small-island probe
   with only a thousand land texels). **There is no relief at that scale for any z-factor to
   recover.** The exaggeration still raises the level-2 spread by an order of magnitude and it
   *is* visible — but this is the weakest level, and it is the level the original complaint was
   about. `?sse=1` is the honest answer to it, and that is a cost setting rather than a fix.
2. **The rock/green boundary drifts with zoom, and it is the largest weakness shipped.** Rock
   keys on the *exaggerated* slope, and slope grows as sampling gets finer, so the mean rock
   blend on the −9,65 tile runs **0.000 (L2 and L3), 0.004 (L5), 0.016 (L6), 0.138 (L8), 0.241
   (L9), 0.369 (L11), 0.552 (L12)** — a mountain that is green from orbit is majority rock at
   level 12. Per level the change is 10–30%, so it reads as detail appearing rather than as a
   seam, and real imagery does get rockier as individual faces resolve; but the cumulative hue
   shift over seven levels is real and is not being called intentional. **The known fix**: read
   the colour's slope at a fixed ground scale rather than at the tile's own. That needs a second
   engine fill or a wide stencil, and the wide stencil has to clamp at the tile edge —
   reintroducing exactly the one-sided-difference bias this module's margin exists to prevent,
   for **hue**, where a seam is far more visible than in shade. A second fill is much cheaper to
   consider now than when it was first raised: it is worker-side work, and the pool has seconds
   of budget in hand at this camera.
3. **No clouds, and no settlement lights.** Both are out of scope by the plan's own statement,
   both are confirmed absent, and one of them is a roadmap feature rather than an oversight.
4. **The vegetation colour is altitude, slope and latitude only.** There is no climate, so no
   deserts, no rainforest, no tundra belt: a mid-latitude coast and an equatorial one are the
   same green at the same height. After clouds this is probably the single largest remaining
   difference from a real image, and it is the "climate + biomes" line the panel already lists
   as designed-but-not-built.
5. **The sun is per-tile, not global.** Every tile is lit from its own local north-west
   (azimuth 315°, altitude 45° — the ArcGIS / QGIS / `gdaldem hillshade` default, so it is a
   convention a reader's eye is already trained on). There is therefore no terminator and no
   globally coherent shadow direction. Correct for a cartographic relief map; not what a
   photograph does.
6. **No cast shadows, only surface shading.** A ridge does not darken the valley behind it.
7. **No atmospheric perspective on the ground, and no specular glint on the ocean.** A real
   orbital photograph has haze thickening toward the limb and a glint lobe over water; this has
   a hard sky-limb boundary, fully saturated ground colour up to the silhouette, and flat matte
   blue sea. Ground atmosphere is off by default for a measured reason: it washed the ramp to a
   uniform pale green from orbit.
8. **No rivers, lakes or ice shelves.** Water is the datum and nothing else.
9. **The parallel ridges on the plate-boundary range look regular** at `?sse=1`. Fold mountains
   along a collision boundary genuinely do look like that, and this is the tectonic term rather
   than the noise term — but that has not been proved, and it is the thing in these pictures
   that most reads as procedural.

### A figure this section refuses to inherit: the height of the highest point

Two files in this tree disagree with each other about the same quantity, and a re-run settles it
against the one that is quoted most often.

| source | says |
| --- | --- |
| `public/app/relief.js` (three sites) and `public/app/controls.js` (two sites) | "the highest point on this planet is **1,381 m**", of which 1,378 m is structural |
| `public/app/panel-fields.js` | "the highest land at **1,979 m** on `DEFAULT_WORLD`" |
| **measured here** — `wb_fill_tile_f32` over 8x4 tiles of 45° at 361x361 posts, canonical resolution, 4,170,724 samples | **peak 1,978.7 m**, deepest −6,345 m, land p50 421 m, p99.9 1,551 m |

**1,381 m does not reproduce, and it is not a scan-density artefact**: the same scan at grid
steps of 1°, 0.5°, 0.25°, 0.125° and 0.0625° gives **1,954.8 / 1,954.8 / 1,978.7 / 1,978.7 /
1,987.5 m**, converging near 1,987 m and never approaching 1,381. The 1,381 m figure comes from
a sibling slice whose record does not state the population it was measured over, and the engine
has moved several commits since. **`panel-fields.js` is right; the comment sites in `relief.js`
and `controls.js` are stale and should be corrected in the files themselves** — this note is
deliberately not treated as the fix, because *a correction that lands in the write-up and not in
the code leaves the wrong number where the next reader will actually look*, a lesson recorded
twice already in this file.

**Nothing above depends on which number is right**, which is why this is a correction rather
than a retraction: `SNOW_LINE_M` at 3,500 m sits above the peak either way (so the snow band
really was dead code), the rock band really was unreachable at a 1.9° maximum slope either way,
and 1,987 m on a 6,371 km sphere is a small fraction of Earth's relief either way. **The
1,378-of-1,381 structural decomposition was not re-derived here** — it needs a tectonic /
roughness split the viewer cannot see — so it is quoted as the sibling slice's own, and the
conclusion it supports is corroborated independently by the `detail.rs` sweep above, which does
not depend on the peak at all.

## A disagreement recorded rather than resolved

**Main-thread rasterisation cost per relief tile has now been measured three times, at three
different values, and this record is not picking a winner.**

| measured by | value | host as reported |
| --- | --- | --- |
| the task that built the provider | **191.6 ms** mean over 26 tiles | "hardware Chrome", via the browser preview pane |
| the task that moved it into the pool | **55.7 ms** mean over 61 tiles, 3 reps | headed Playwright Chromium, ANGLE/D3D11, 32 cores |
| **this section** | **178.1 ms** mean over 61 tiles | agent browser pane, ANGLE/D3D11, 32 cores, frames driven by hand |

Most likely the host and the harness. **No conclusion in any of the three depends on which is
right** — at 55.7 ms the main thread still blocks for 3.4 s per orbital view, at 178 ms for
10.9 s, and all three say the same thing about whether the main thread could carry this work.
**A record that quietly drops one of two disagreeing measurements is worse than one that shows
both**, and there are now three.

The same split runs through every millisecond figure this slice produced and through none of its
counts. Tiles, levels, texels, metres per texel and `mainThreadRasters` reproduced **exactly**
across hosts; worker CPU per tile varied from **162 to 216 ms across three runs on one host in
one session**. **Quote the ratio with the table it came from, or not at all.**

### And a refusal that survived its own correction

The provider task **declined to publish a frame-time A/B** because the only host on which
`requestAnimationFrame` fired for it was a software rasteriser whose baseline was already 224 ms
per frame — the relief work was buried under it. It guessed the callback was silent for one
reason; the pool task found a different one and got real frame numbers on headed hardware.
**The refusal was right and the diagnosis was wrong**, and that is the point: declining to
publish a figure the host invalidates costs nothing when the explanation later changes.

## Two hosts, and what neither of them can do

Every browser figure in this section came from the **agent browser pane** (Chromium 148, ANGLE
Intel UHD D3D11, 32 logical cores, Windows 11 10.0.26200). Figures attributed to another host
came from **headed Playwright Chromium** with `--use-gl=angle --use-angle=d3d11` on the same
machine. Three facts about the pane are worth writing down, because they are harness traps that
have already produced two wrong diagnoses in this project:

1. **Its viewport is 0x0 by default.** `innerWidth`, `innerHeight`, `canvas.width` and
   `canvas.height` are all 0 on load, so nothing is ever requested and `globe.tilesLoaded` is
   **vacuously true with zero tiles**. Emulating a viewport (1400x900 here) fixes the canvas and
   gives a real hardware WebGL context.
2. **`requestAnimationFrame` still never fires, and the viewport was not the reason.** With a
   real 1400x900 canvas the pane reports `document.hidden === true` and
   `document.visibilityState === "hidden"` at every moment, fronted or not — **0 rAF callbacks
   over 2.7 s**, and `scene.frameState.frameNumber` stuck at 0. Chrome does not run the
   animation-frame loop for a hidden document. **So the pane can never produce a frame time, an
   fps, or a wall-clock time-to-settle**, and none is claimed here. That is a refinement of the
   earlier diagnosis rather than a contradiction of it: fixing the viewport is necessary and not
   sufficient.
3. **A hand-driven render loop must yield, or it measures a globe that drew nothing.** 600
   consecutive `initializeFrame()` / `render(PIN)` calls with no yield left `tilesVisited = 0`,
   `maxDepthVisited = 0` and **zero relief tiles requested**, because the worker replies and
   tile callbacks never got a turn on the event loop. Yielding through a `MessageChannel` every
   five frames settles the same page in about 1,200 frames. (`setTimeout` is clamped to roughly
   a second in a hidden document and is the wrong yield here.) **This is broken-verifier #7's
   exact shape** — a check reporting on a globe that never drew — and the `quadtree-depth`
   zero-work guard added for that case caught it, by name, on the first run: *"the scene
   rendered 2,000 frames and the quadtree visited 0 tiles… this is not a pass."* A guard written
   for one harness caught a different one, four slices later.

## Three lessons this slice produced that outlive its code

1. **Byte-identity proves the picture, never the path.** The first mutation of the worker move —
   a provider that *ignores the pool* — **passed the byte-identity test**, because a provider
   that ignores the pool draws exactly the same pixels. Only the `mainThreadRasters` counter
   catches it. **Three of this slice's tasks found an assertion that looked load-bearing and was
   not, each by mutating rather than by reading.** One of the others was a byte-identity
   assertion fully shadowed by a pre-existing mean-equality assertion — found only because a
   *plausible* mutation was run after an *easy* one had already killed everything unaided, which
   is the entire reason to run two.
2. **Dead code looks like a feature.** All three colour blends in `relief.js` had never once been
   selected against this terrain: `ROCK_SLOPE_LOW_DEG` was 22° against a steepest texel of 1.9°,
   `SNOW_LINE_M` was 3,500 m against a peak under 2 km, and the top two `LAND_BANDS` stops sat
   above the 99.9th percentile of land. The `ElevationRamp` on the `?relief=0` path had the same
   problem: its own snow band had never been drawn, in the very change that added it. **The layer
   was a two-band green-and-ochre height ramp wearing the vocabulary of a slope-aware one.** Code
   whose condition is never met is the same family as a verifier that cannot fail. Every band is
   now placed on measured hypsometry, and a test asserts each stop lies inside the measured range,
   so widening one again fails loudly.
3. **A panel default that is not the engine's default was this viewer's characteristic defect** —
   four instances, the fourth found by the check rather than by a person. The countermeasure is
   not four fixes; it is one table the panel *reads* rather than restates, plus one assertion
   that a slider can express its own default.

## Everything this section's figures came from

Run for this section, on this host, in this order. Both exited 0, read from `$?` rather than
from a summary line.

```
cd viewer
npm test                 # 57 tests, 57 pass, 0 fail
npm run check:wasm       # the shipped .wasm matches its manifest and the source that is here now
```

Node measurements, against this repository's checked-in
`viewer/public/wasm/worldbuilder_engine.wasm` on Node v22.17.0, all on `DEFAULT_WORLD`
(`seed 20260904`, radius 6,371,000 m, 12 plates, land 0.29):

- **shade factor** — the tile containing −9, 65 at levels 2–12, `size = 256`; per-texel ratio of
  the normal raster to one rendered with `ambient = 1`, skipping denominators under 8.
- **quantisation floor** — the same, plus a synthetic arm whose shade is the exact constant
  0.8096 applied to that arm's own unshaded raster through the same 8-bit rounding.
- **slope distribution** — `reliefTile`'s own central differences over its own margined grid,
  three probe tiles (−9,65 / 40,−100 / 20,10), land texels only.
- **hypsometry and the peak** — `wb_fill_tile_f32` over 8x4 tiles of 45° at 361x361 posts,
  canonical resolution (4,170,724 samples), repeated at 46 / 91 / 181 / 361 / 721 posts per tile
  for the scan-density arm.
- **rock blend** — mean rock-blend fraction over land texels, same probe tile, levels 2–12.
- **post spacing** — `postSpacingM(level, 65, 6371000)` from `public/app/terrain.js` itself.

Browser measurements, agent browser pane as described above, over `npm --prefix viewer run
serve` on port 8137 (there is an untracked `.claude/launch.json` that starts exactly that;
it is tooling and is deliberately not committed). Viewport emulated to 1400x900 except the
digests, which are at 1000x800:

```
/?fly=10,20,9000000                      61 relief tiles, level 3, 0 main-thread rasters
/?fly=10,20,9000000&workers=0            61 main-thread rasters, 10,864 ms, worst block 346.4 ms
/?fly=10,20,9000000&sse=1                155 tiles, level 4, 4,906 m/texel
/?fly=10,20,9000000&sse=1.5              83 tiles, level 3 -- the dead middle
/?fly=10,20,9000000&reliefSize=128       244 tiles at L4, 9,850 m/texel
/?fly=10,20,9000000&reliefSize=512       15 tiles at L2, 9,792 m/texel
/?fly=10,20,9000000&relief=0             the digest control
/                                        __wb.check() -- 11 checks, 11 passed, 0 failed
/?harbour=1                              __wb.check() -- 12 checks, 12 passed, 0 failed
```

No console errors in any run. **0 off-origin requests in every one**, which remains the number
that has never moved across every measurement in every task that has touched this viewer.
