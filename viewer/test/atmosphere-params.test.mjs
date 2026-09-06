// Node-native tests for the viewer's atmosphere, limb and tone channel: `atmosphere-params.js`.
//
// No framework: `node:test` + `node:assert/strict`, run with `npm test` from `viewer/`.
//
// # What is real here and what is a stand-in, stated rather than left to be discovered
//
// **The Cesium objects are REAL.** `SkyAtmosphere` and `Atmosphere` are constructed from the
// installed `cesium` package, so every "this is Cesium's default" assertion below is asking
// Cesium rather than a number transcribed from its documentation. That is the whole point of the
// module: it holds no scattering constant of its own, and a test that held one would put the
// second copy back in the place the module removed it from.
//
// The `Scene` itself is a stand-in -- a plain object with the four properties the module touches.
// A real `Scene` needs a WebGL context, and a headless renderer is what `scripts/probe.mjs` and
// `scripts/shoot.mjs` are for; every RENDERED figure quoted in this task's report comes from
// those, not from here. What is asserted here is the decision, not the picture.
//
// Host: node, this repository's `viewer/node_modules/cesium` at 1.145.0.

import test from "node:test";
import assert from "node:assert/strict";
import * as Cesium from "cesium";
import {
  applyAtmosphere, formatAtmosphere, GROUND_FIELDS, SKY_FIELDS,
} from "../public/app/atmosphere-params.js";

/// A stand-in `Scene` carrying real Cesium atmosphere objects.
///
/// `highDynamicRangeSupported` defaults to `true` here so the HDR path is reachable; the host
/// that cannot do it gets its own test.
function fakeScene({ hdrSupported = true } = {}) {
  return {
    globe: { showGroundAtmosphere: null },
    skyAtmosphere: new Cesium.SkyAtmosphere(),
    atmosphere: new Cesium.Atmosphere(),
    postProcessStages: { tonemapper: null, exposure: null },
    highDynamicRange: false,
    highDynamicRangeSupported: hdrSupported,
  };
}

const query = (string) => new URLSearchParams(string);

test("with no parameters every scattering value is the one already on the Scene", () => {
  // **Deliberately NOT Cesium's defaults.** If the module carried its own copy of a scale height
  // it would agree with Cesium's default and a scene built at Cesium's defaults could not tell
  // the difference. These values are chosen to be nothing Cesium would ever produce, so the only
  // way for them to survive is for the module to have read them.
  const scene = fakeScene();
  const witness = 12345.678;
  let mark = witness;
  const expected = { sky: {}, ground: {} };
  for (const [, property] of SKY_FIELDS) {
    scene.skyAtmosphere[property] = mark;
    expected.sky[property] = mark;
    mark += 1;
  }
  for (const [, property] of GROUND_FIELDS) {
    scene.atmosphere[property] = mark;
    expected.ground[property] = mark;
    mark += 1;
  }

  applyAtmosphere(scene, query(""), Cesium);

  for (const [, property] of SKY_FIELDS) {
    assert.equal(scene.skyAtmosphere[property], expected.sky[property],
      `sky.${property} was replaced by a value this module wrote down`);
  }
  for (const [, property] of GROUND_FIELDS) {
    assert.equal(scene.atmosphere[property], expected.ground[property],
      `atmosphere.${property} was replaced by a value this module wrote down`);
  }
});

test("every scattering parameter reaches the property it names, and only that one", () => {
  // A table-driven forward is only as good as the table.
  //
  // **The first two assertions exist because the third one alone is not enough, and a mutation
  // proved it.** Point `limbMie`'s row at `atmosphereRayleighScaleHeight` and the per-parameter
  // walk below still passes: setting `?limbMie=` does move the property its (wrong) row names,
  // and the only sibling it collides with is skipped as "the same property". A table with two
  // rows aimed at one field is invisible to a test that only checks rows against themselves.
  // So: the property names must be DISTINCT, and each must be a field the real Cesium object
  // actually has -- a row naming `atmosphereRaleighScaleHeight` would otherwise write a new
  // property onto the object and change nothing about the picture, silently.
  for (const [group, fields, host] of [
    ["sky", SKY_FIELDS, (scene) => scene.skyAtmosphere],
    ["ground", GROUND_FIELDS, (scene) => scene.atmosphere],
  ]) {
    const properties = fields.map(([, property]) => property);
    assert.equal(new Set(properties).size, properties.length,
      `${group} has two rows aimed at one property: ${properties.join(", ")}`);
    const names = fields.map(([name]) => name);
    assert.equal(new Set(names).size, names.length, `${group} has a duplicate query name`);
    const probe = fakeScene();
    for (const property of properties) {
      assert.notEqual(host(probe)[property], undefined,
        `${group}.${property} is not a field this Cesium object has`);
    }

    for (const [name, property] of fields) {
      const scene = fakeScene();
      const before = new Map(fields.map(([, other]) => [other, host(scene)[other]]));
      applyAtmosphere(scene, query(`${name}=4321`), Cesium);
      assert.equal(host(scene)[property], 4321, `${group}.${name} did not reach ${property}`);
      for (const [other, value] of before) {
        if (other === property) continue;
        assert.equal(host(scene)[other], value, `${group}.${name} also moved ${other}`);
      }
    }
  }
});

test("the ground atmosphere is ON by default, and both switches that turn it off do", () => {
  // **Cesium's own default is asked for rather than asserted from memory.** The change this task
  // shipped is the REMOVAL of a local override, not the addition of a local `true`, and the way to
  // say that in a test is to show the module now agrees with the class.
  assert.equal(new Cesium.Globe().showGroundAtmosphere, true,
    "Cesium's own default moved; this module's 'default' is no longer Cesium's");

  const on = fakeScene();
  assert.equal(applyAtmosphere(on, query(""), Cesium).groundAtmosphere, true);
  assert.equal(on.globe.showGroundAtmosphere, true);

  const off = fakeScene();
  assert.equal(applyAtmosphere(off, query("atmosphere=0"), Cesium).groundAtmosphere, false);
  assert.equal(off.globe.showGroundAtmosphere, false);

  const flat = fakeScene();
  assert.equal(applyAtmosphere(flat, query("flat=1"), Cesium).groundAtmosphere, false);
  assert.equal(flat.globe.showGroundAtmosphere, false);

  // `?atmosphere=1` was the OLD way to ask for it and links carrying it still exist. It must not
  // now mean the opposite of what it meant.
  const legacy = fakeScene();
  assert.equal(applyAtmosphere(legacy, query("atmosphere=1"), Cesium).groundAtmosphere, true);
});

test("the sky limb is on unless ?flat=1, independently of the ground switch", () => {
  // The two are different effects and the whole ground-atmosphere finding turned on not confusing
  // them. `?atmosphere=0` must leave the limb alone.
  const grounded = fakeScene();
  const applied = applyAtmosphere(grounded, query("atmosphere=0"), Cesium);
  assert.equal(grounded.skyAtmosphere.show, true);
  assert.equal(applied.groundAtmosphere, false);

  const flat = fakeScene();
  applyAtmosphere(flat, query("flat=1"), Cesium);
  assert.equal(flat.skyAtmosphere.show, false);
});

test("a parameter that is present but not a finite number is ignored, not forwarded", () => {
  // `?limb=banana` is a typo in a shared link. `Number("banana")` is NaN, and a NaN scale height
  // reaches a shader as a uniform that turns the sky black -- a typo becoming a blank page, which
  // is the failure `tectonicFromParams` wrote its own rule against.
  const scene = fakeScene();
  const defaults = SKY_FIELDS.map(([, property]) => scene.skyAtmosphere[property]);
  const groundDefaults = GROUND_FIELDS.map(([, property]) => scene.atmosphere[property]);
  const nonsense = SKY_FIELDS.concat(GROUND_FIELDS)
    .map(([name]) => `${name}=banana`).join("&");
  applyAtmosphere(scene, query(`${nonsense}&exposure=`), Cesium);
  SKY_FIELDS.forEach(([, property], index) => {
    assert.equal(scene.skyAtmosphere[property], defaults[index]);
    assert.ok(Number.isFinite(scene.skyAtmosphere[property]), `sky.${property} is not finite`);
  });
  GROUND_FIELDS.forEach(([, property], index) => {
    assert.equal(scene.atmosphere[property], groundDefaults[index]);
    assert.ok(Number.isFinite(scene.atmosphere[property]), `atmosphere.${property} is not finite`);
  });
});

test("an unknown tonemapper name is ignored rather than forwarded to Cesium", () => {
  // Cesium throws on an unknown tonemapper, and the setter runs inside `boot()`, so a bad name in
  // a shared link would be a failed boot rather than an ungraded picture.
  const bad = fakeScene();
  const applied = applyAtmosphere(bad, query("hdr=1&tonemap=SEPIA"), Cesium);
  assert.equal(bad.highDynamicRange, true);
  assert.equal(bad.postProcessStages.tonemapper, null, "an unknown name reached Cesium");
  assert.equal(applied.hdr.applied, null);

  const good = fakeScene();
  applyAtmosphere(good, query("hdr=1&tonemap=PBR_NEUTRAL&exposure=1.2"), Cesium);
  assert.equal(good.postProcessStages.tonemapper, Cesium.Tonemapper.PBR_NEUTRAL);
  assert.equal(good.postProcessStages.exposure, 1.2);
});

test("a host that cannot tonemap is not told to, and the status line says which it is", () => {
  // Three states, not two: off, on, and asked-for-but-impossible. Collapsing the third into the
  // first would let a screenshot of an ungraded picture be captioned as a graded one.
  const cannot = fakeScene({ hdrSupported: false });
  const applied = applyAtmosphere(cannot, query("hdr=1&tonemap=ACES"), Cesium);
  assert.equal(cannot.highDynamicRange, false);
  assert.equal(cannot.postProcessStages.tonemapper, null);
  assert.equal(applied.hdr.asked, true);
  assert.equal(applied.hdr.supported, false);
  assert.match(formatAtmosphere(applied), /UNSUPPORTED/);

  assert.match(formatAtmosphere(applyAtmosphere(fakeScene(), query(""), Cesium)), /hdr=off/);
});

test("?atmosSun=1 asks for SUNLIGHT and its absence leaves Cesium's own dynamic lighting", () => {
  const none = fakeScene();
  const before = none.atmosphere.dynamicLighting;
  applyAtmosphere(none, query(""), Cesium);
  assert.equal(none.atmosphere.dynamicLighting, before);

  const sunlit = fakeScene();
  const applied = applyAtmosphere(sunlit, query("atmosSun=1"), Cesium);
  assert.equal(sunlit.atmosphere.dynamicLighting, Cesium.DynamicAtmosphereLightingType.SUNLIGHT);
  assert.equal(applied.sunlit, true);
});

test("the status line names every number that decides the picture", () => {
  // A screenshot's caption IS this string. A caption that said "atmosphere=on" would say nothing
  // about an effect whose entire argument is a set of scale heights, so each one is asserted to
  // appear -- with a value chosen so it cannot be matched by accident.
  const scene = fakeScene();
  const line = formatAtmosphere(applyAtmosphere(
    scene,
    query("limb=24001&limbMie=3201&limbLight=51&atmosLight=11&limbSmooth=1&atmosSun=1"),
    Cesium,
  ));
  for (const token of ["24001", "3201", "51", "11", "per-fragment", "sunlit"]) {
    assert.ok(line.includes(token), `the status line does not name ${token}: ${line}`);
  }
  assert.match(line, /ground atmos=on/);

  assert.match(formatAtmosphere(applyAtmosphere(fakeScene(), query("flat=1"), Cesium)),
    /ground atmos=off .*limb=off/);
});
