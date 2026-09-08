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

/// What stands where the credits would: this project's own mark and name.
///
/// **The wordmark is real text, not part of the picture.** The image model draws the planet
/// beautifully and cannot spell - asked for "World Builder by Aetos" it produced "Avetos",
/// dropped a word, and stacked the line. It also has no way to stay crisp: a rasterised
/// wordmark twenty-four pixels tall is mush, while text at that size is simply text. So the
/// model draws the emblem, which is the part only it can do, and the browser sets the words,
/// which is the part it does badly and CSS does perfectly.
///
/// `mix-blend-mode: screen` is what makes the black square disappear. The emblem is glowing
/// artwork on black, so screen keeps every lit pixel and drops the background to nothing -
/// no alpha channel to cut, no halo where a matte was not quite right, and it composites
/// correctly over both the night sky and a bright limb.
function brandTheCorner(document) {
  const brand = document.createElement("div");
  brand.id = "wb-brand";
  brand.style.cssText = [
    "position:absolute", "left:8px", "bottom:6px", "z-index:5",
    "display:flex", "align-items:center", "gap:7px",
    "pointer-events:none", "user-select:none",
  ].join(";");

  const mark = document.createElement("img");
  mark.src = "/brand/worldbuilder-mark.png";
  mark.alt = "";
  mark.style.cssText = [
    "width:26px", "height:26px", "display:block",
    // See the note above: the emblem is drawn on black and screened onto the scene.
    "mix-blend-mode:screen",
  ].join(";");

  const words = document.createElement("span");
  words.style.cssText = [
    "font:500 13px/1 'Iowan Old Style', 'Palatino Linotype', Palatino, Georgia, serif",
    "letter-spacing:0.015em", "white-space:nowrap",
    "text-shadow:0 1px 3px rgba(0,0,0,0.9), 0 0 10px rgba(0,0,0,0.7)",
  ].join(";");
  const plain = document.createElement("span");
  plain.textContent = "World Builder by ";
  plain.style.color = "rgba(255,255,255,0.9)";
  const name = document.createElement("span");
  name.textContent = "Aetos";
  name.style.color = "#f0a850";
  words.append(plain, name);

  brand.append(mark, words);
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

// Fold the diagnostics wall into one line and split the controls into two columns. Runs
// after the panel exists; it observes for sections added later, so nothing has to be
// re-run when a world opens and the area list appears.
import("./chrome.js").then((chrome) => {
  const start = () => chrome.tidy(document);
  if (document.readyState === "complete") setTimeout(start, 250);
  else window.addEventListener("load", () => setTimeout(start, 250));
}).catch(() => {});
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
