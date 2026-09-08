// The viewer bootstrap. Extracted verbatim from index.html's second inline <script> in
// Task 6 so the page can be served under `script-src 'self'` with no `'unsafe-inline'`.
// Still a classic (non-module) script, still runs in document order after Cesium.js and
// before /app/main.js (a module script, which is deferred by definition), so the ordering
// the page relied on is unchanged.

// NET PROBE — deliberately off by default.
//   /            -> offline: baseLayer disabled, no imagery provider at all.
//   /?net-probe=1 -> restores Cesium's own default, ImageryLayer.fromWorldImagery(),
//                    which is the single network-live Viewer default. This exists so the
//                    network trace can be shown to be capable of catching a phone-home.
//                    A trace that shows nothing proves nothing unless it can show something.
//
//   Since Task 6 the probe is ALSO the CSP's proof of refusal: under
//   `default-src 'self'` the probe's requests are blocked by the browser before they
//   reach the network, so the same switch that once demonstrated the trace can see
//   something now demonstrates the policy can refuse something.
const netProbe = new URLSearchParams(location.search).has("net-probe");

// Ion is a separate, paid Cesium product with its own terms. Nothing here uses it,
// and blanking the bundled demo token makes any accidental ion call fail loudly
// rather than quietly succeed against Cesium's servers.
if (!netProbe) {
  Cesium.Ion.defaultAccessToken = undefined;
}

// The credit strip carries this project's own name, because nothing here is attributed to
// anybody else.
//
// **This is a fact about what the page loads, not a preference.** Cesium ion is a separate
// paid product whose terms require its logo when its assets are used, and none are: the
// token above is blanked, `baseLayer` is false, every ion-backed widget is off, and the
// terrain and imagery are generated here. CesiumJS itself is Apache 2.0, which requires the
// licence notice to travel with the distribution - it does, at `vendor/cesium/LICENSE.md` -
// and not a logo on the screen.
//
// **Put it back the moment anything attributed is added.** A base map, a real DEM, an ion
// asset, an OSM layer: each carries an attribution requirement, and Cesium will add the
// credit to this container automatically, where nobody would see it. Anyone wiring one in
// should delete this line first.
const HIDE_CREDITS = true;

/// What stands where the credits would.
///
/// Cesium still gets a container to write into - it is hidden, and empty, because nothing
/// on this globe is somebody else's - and this sits in the same corner in its place.
function brandTheCorner(document) {
  const brand = document.createElement("div");
  brand.id = "wb-brand";
  brand.textContent = "World Builder by Aetos";
  brand.style.cssText = [
    "position:absolute", "left:10px", "bottom:8px", "z-index:5",
    "pointer-events:none", "user-select:none",
    "font:600 13px/1 system-ui, sans-serif", "letter-spacing:0.02em",
    "color:rgba(255,255,255,0.82)",
    "text-shadow:0 1px 3px rgba(0,0,0,0.85), 0 0 12px rgba(0,0,0,0.6)",
  ].join(";");
  document.body.appendChild(brand);
  return brand;
}

const viewer = new Cesium.Viewer("cesiumContainer", {
  // The one network-live default. `false` means: no base imagery layer at all.
  baseLayer: netProbe ? Cesium.ImageryLayer.fromWorldImagery() : false,
  // Ion-backed or ion-listing widgets. Each would reach api.cesium.com when used.
  baseLayerPicker: false,
  geocoder: false,
  // Default terrain is EllipsoidTerrainProvider, which is computed, not fetched.
  // Slice 2b Task 4 replaces it with a CustomHeightmapTerrainProvider over the
  // generator; it stays local either way.
  animation: false,
  timeline: false,
  fullscreenButton: false,
  navigationHelpButton: false,
  homeButton: false,
  sceneModePicker: false,
  infoBox: false,
  selectionIndicator: false,
  // See HIDE_CREDITS above.
  creditContainer: HIDE_CREDITS
    ? Object.assign(document.createElement("div"), { style: "display:none" })
    : undefined,
});

brandTheCorner(document);
viewer.scene.globe.baseColor = Cesium.Color.fromCssColorString("#10243a");

document.getElementById("status").textContent =
  `Cesium ${Cesium.VERSION} | base=${window.CESIUM_BASE_URL} | ` +
  `imageryLayers=${viewer.imageryLayers.length} | ` +
  `terrain=${viewer.terrainProvider.constructor.name} | ` +
  `net-probe=${netProbe ? "ON (expect an outbound request)" : "off"}`;

// Machine-readable handles for the trace harness and for later slice-2b tasks.
window.viewer = viewer;
window.__viewerReady = {
  version: Cesium.VERSION,
  netProbe,
  imageryLayers: viewer.imageryLayers.length,
  terrain: viewer.terrainProvider.constructor.name,
};
