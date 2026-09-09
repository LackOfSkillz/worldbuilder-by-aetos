// The viewer used to stop rendering, permanently, with no message on the page.
//
// Cesium's `DataSourceDisplay.update` runs every clock tick and reads, for each source in
// the collection, `source._visualizers.length`. A source is one OBJECT and the collection
// can hold two ENTRIES naming it; removing one entry tears down that object's visualizers
// while the other entry still points at it, so the next tick reads `undefined.length`.
// Cesium answers a throw from the render loop by stopping the render loop.
//
// The duplicate came from `dataSources.add()` being a promise: a `remove()` issued before
// the add resolved removed nothing, and the add landed afterwards. Both halves are tested
// here against a stand-in collection with the same asynchronous contract.

import assert from "node:assert/strict";
import test from "node:test";

import { showLayer } from "../public/app/globe-layers.js";

/// A collection that behaves like Cesium's: `add` resolves on a later turn, and the
/// display hands out visualizers on add and takes them away on remove.
function fakeViewer() {
  const entries = [];
  return {
    dataSources: {
      get length() { return entries.length; },
      entries,
      contains: (source) => entries.includes(source),
      add(source) {
        return Promise.resolve().then(() => {
          entries.push(source);
          source._visualizers = ["one"];
          return source;
        });
      },
      remove(source) {
        const at = entries.indexOf(source);
        if (at === -1) return false;
        entries.splice(at, 1);
        // What the display does: the visualizers belong to the object, not the entry.
        source._visualizers = undefined;
        return true;
      },
    },
    /// One clock tick, as `DataSourceDisplay.update` performs it.
    tick() {
      for (const source of entries) {
        const visualizers = source._visualizers;
        if (visualizers.length === undefined) throw new Error("unreachable");
      }
      return true;
    },
  };
}

test("a layer shown twice is only added once", async () => {
  const viewer = fakeViewer();
  const source = { name: "areas" };
  await showLayer(viewer, source).ready;
  await showLayer(viewer, source).ready;
  assert.equal(viewer.dataSources.entries.filter((s) => s === source).length, 1);
});

test("removing a layer shown twice does not wedge the next tick", async () => {
  const viewer = fakeViewer();
  const source = { name: "areas" };
  const first = showLayer(viewer, source);
  const second = showLayer(viewer, source);
  await Promise.all([first.ready, second.ready]);
  await first.remove();
  assert.doesNotThrow(() => viewer.tick());
  await second.remove();
  assert.doesNotThrow(() => viewer.tick());
});

test("a remove issued before the add lands still removes it", async () => {
  const viewer = fakeViewer();
  const source = { name: "roads" };
  const layer = showLayer(viewer, source);
  // Deliberately not awaiting `ready` - this is the shape that leaked.
  await layer.remove();
  assert.equal(viewer.dataSources.length, 0, "the pending add must not land afterwards");
  assert.doesNotThrow(() => viewer.tick());
});

test("removing twice does not strip a source somebody else is showing", async () => {
  const viewer = fakeViewer();
  const source = { name: "route" };
  const layer = showLayer(viewer, source);
  await layer.remove();
  const again = showLayer(viewer, source);
  await again.ready;
  await layer.remove();  // the stale handle must do nothing
  assert.equal(viewer.dataSources.length, 1);
  assert.doesNotThrow(() => viewer.tick());
});

test("the fake reproduces the original crash, so the guard is what fixes it", async () => {
  const viewer = fakeViewer();
  const source = { name: "areas" };
  await viewer.dataSources.add(source);
  await viewer.dataSources.add(source);   // the unguarded double add
  viewer.dataSources.remove(source);
  assert.throws(() => viewer.tick(), TypeError);
});
