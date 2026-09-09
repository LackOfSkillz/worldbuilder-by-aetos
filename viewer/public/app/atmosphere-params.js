//! Atmosphere, limb and tone: what a query string asks the RENDERER for.
//
// This is the fourth module of its shape -- `relief-params.js`, `tectonic-params.js` and
// `coast-params.js` are the other three -- and it is here for the same reason they are: the
// parameter reading is a pure function of a `URLSearchParams` and a set of defaults, and a pure
// function can be asserted by `node --test` while a line inside `boot()` cannot.
//
// # The one difference from those three
//
// They read their defaults FROM THE ENGINE, across the FFI boundary, so the viewer holds no copy
// of a generator constant to drift from. This module does the same thing against **Cesium**:
// every default is read off the live `Scene` object rather than restated here. There is not one
// scattering constant written down in this file, which is why `?limb=` absent is byte-for-byte
// the picture Cesium draws with no parameter at all -- and why upgrading Cesium moves our
// defaults with it instead of silently pinning them to 1.145's.
//
// # What DOES change from Cesium's default, and it is exactly one thing
//
// `showGroundAtmosphere`. Cesium's own default is `true`; this viewer forced it `false` for a
// measured reason that the ocean retune invalidated, and the re-measurement is recorded in
// `main.js` beside the call. So this module's behaviour is: Cesium's defaults throughout, with
// the local override REMOVED rather than a new local value put in its place.

/// A numeric query parameter, or the fallback -- **and the fallback when the value is present but
/// not a finite number.**
///
/// `?limb=banana` is a typo in a shared link. `Number("banana")` is `NaN`, and a `NaN` scale
/// height reaches a shader as a uniform that turns the whole sky black. `tectonicFromParams` made
/// exactly this rule for exactly this reason ("a typo in a shared link... answering it with a
/// refused world would turn a typo into a blank page") and this is that rule, here.
function finiteNumber(params, name, fallback) {
  if (!params.has(name)) return fallback;
  const value = Number(params.get(name));
  return Number.isFinite(value) ? value : fallback;
}

/// The scattering fields this module forwards, as `[query parameter, property]` pairs, per host
/// object. A table rather than ten hand-written lines: the defect this viewer keeps producing is
/// a second copy of a number, and ten hand-written assignments is ten places for one to appear.
export const SKY_FIELDS = [
  ["limb", "atmosphereRayleighScaleHeight"],
  ["limbMie", "atmosphereMieScaleHeight"],
  ["limbLight", "atmosphereLightIntensity"],
];

export const GROUND_FIELDS = [
  ["atmosLight", "lightIntensity"],
  ["atmosScale", "rayleighScaleHeight"],
  ["atmosSaturation", "saturationShift"],
  ["atmosBrightness", "brightnessShift"],
];

/// Read a query string against a live `Scene` and apply the result to it.
///
/// Returns the settings it applied, so the status line can name them and a test can assert them
/// without reaching back into the scene it just wrote to.
///
/// `Cesium` is passed in rather than reached for as a global: this module is imported by
/// `node --test`, where there is no `window.Cesium`, and a module that only works inside a
/// browser is a module this project cannot assert anything about.
export function applyAtmosphere(scene, params, Cesium) {
  const flat = params.get("flat") === "1";

  // The SKY atmosphere: the blue limb outside the silhouette. It touches no ground fragment, so
  // it cannot wash anything.
  const sky = scene.skyAtmosphere;
  sky.show = !flat;
  for (const [name, property] of SKY_FIELDS) {
    sky[property] = finiteNumber(params, name, sky[property]);
  }
  // Per-fragment scattering. Cesium's default is per-vertex on a coarse shell, which bands
  // visibly once the limb is thickened. Off by default all the same, because turning it on
  // changes the picture and nothing in this task measured it as better.
  sky.perFragmentAtmosphere = params.get("limbSmooth") === "1";

  // The GROUND atmosphere. **This is the switch the task turned**, and `?atmosphere=0` restores
  // the previous picture exactly -- verified: both recorded digests come back byte-identical
  // under it.
  const groundAtmosphere = !flat && params.get("atmosphere") !== "0";
  scene.globe.showGroundAtmosphere = groundAtmosphere;

  // Its scattering, which since Cesium 1.116 lives on `scene.atmosphere` and is shared with the
  // sky's colour. `dynamicLighting` defaults to `NONE`, which hazes the whole disc regardless of
  // where the sun is -- a uniform haze over a whole disc being the shape of the wash this effect
  // was once switched off for, `?atmosSun=1` asks for `SUNLIGHT` instead. Measured at the orbital
  // camera it moved no ground pixel at all; it is offered rather than shipped for that reason.
  const atmosphere = scene.atmosphere;
  for (const [name, property] of GROUND_FIELDS) {
    atmosphere[property] = finiteNumber(params, name, atmosphere[property]);
  }
  const sunlit = params.get("atmosSun") === "1";
  if (sunlit) atmosphere.dynamicLighting = Cesium.DynamicAtmosphereLightingType.SUNLIGHT;

  // Tone. Cesium's tonemappers only run with `highDynamicRange` on, and a host without float
  // colour attachments cannot do it at all -- so the request and the capability are two different
  // facts and the caller is told both, rather than being handed an ungraded picture that looks
  // like the graded one was asked for and did nothing.
  const hdrAsked = params.get("hdr") === "1";
  const hdrSupported = scene.highDynamicRangeSupported === true;
  const tonemapperName = params.get("tonemap");
  // An unrecognised name is IGNORED rather than forwarded, for the `?limb=banana` reason: Cesium
  // throws on an unknown tonemapper, and a typo in a shared link must not be a blank page.
  const tonemapper = tonemapperName && Cesium.Tonemapper[tonemapperName]
    ? Cesium.Tonemapper[tonemapperName]
    : null;
  if (hdrAsked && hdrSupported) {
    scene.highDynamicRange = true;
    if (tonemapper) scene.postProcessStages.tonemapper = tonemapper;
    scene.postProcessStages.exposure = finiteNumber(params, "exposure", 1);
  }

  return {
    flat,
    groundAtmosphere,
    sunlit,
    sky: {
      show: sky.show,
      rayleighScaleHeightM: sky.atmosphereRayleighScaleHeight,
      mieScaleHeightM: sky.atmosphereMieScaleHeight,
      lightIntensity: sky.atmosphereLightIntensity,
      perFragment: sky.perFragmentAtmosphere,
    },
    ground: {
      lightIntensity: atmosphere.lightIntensity,
      rayleighScaleHeightM: atmosphere.rayleighScaleHeight,
      saturationShift: atmosphere.saturationShift,
      brightnessShift: atmosphere.brightnessShift,
    },
    hdr: {
      asked: hdrAsked,
      supported: hdrSupported,
      on: scene.highDynamicRange === true,
      tonemapper: tonemapperName ?? null,
      applied: hdrAsked && hdrSupported ? tonemapper : null,
      exposure: finiteNumber(params, "exposure", 1),
    },
  };
}

/// The atmosphere half of the status line. A screenshot carries that line as its own caption, and
/// "atmosphere=on" would say nothing about an effect whose whole argument is a set of scale
/// heights.
export function formatAtmosphere(applied) {
  const sky = applied.flat
    ? "off"
    : `${applied.sky.rayleighScaleHeightM} m ray / ${applied.sky.mieScaleHeightM} m mie light ${
      applied.sky.lightIntensity}${applied.sky.perFragment ? " per-fragment" : ""}`;
  const hdr = applied.hdr.on
    ? `on ${applied.hdr.applied ?? "default"} exp ${applied.hdr.exposure}`
    : applied.hdr.asked ? "asked for, UNSUPPORTED on this host" : "off";
  return `ground atmos=${applied.groundAtmosphere ? "on" : "off"}${
    applied.sunlit ? " sunlit" : ""} light ${applied.ground.lightIntensity} | ` +
    `limb=${sky} | hdr=${hdr}`;
}
