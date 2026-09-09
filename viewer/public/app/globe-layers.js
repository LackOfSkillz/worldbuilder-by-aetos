//! Putting a data source on the globe and taking it off again, without wedging the viewer.
//
// **`viewer.dataSources.add()` returns a promise, and forgetting that costs more than a
// leak.** The known symptom was a duplicate: a `remove()` issued before the matching `add()`
// resolved removed nothing, the add landed afterwards, and the layer was on the globe twice.
// The unknown one stops the viewer dead.
//
// Cesium's `DataSourceDisplay.update` runs once per clock tick and, for every source in the
// collection, reads:
//
//     o = source._visualizers, r = o.length
//
// A source is one OBJECT and the collection can hold two ENTRIES pointing at it. Removing
// one entry makes the display tear down that object's visualizers - `_visualizers` becomes
// undefined - while the second entry still names it. The next tick reads `undefined.length`
// and throws, and Cesium answers a throw from the render loop by stopping the render loop.
// The globe freezes, the panels go on working, and nothing on the page says why.
//
// Reproduced exactly: add one source twice, remove it once, tick. Same TypeError, same
// function. Found after a reload with a hundred and thirty generated areas on screen, which
// is just the state with enough layers going up and down for the race to be won.
//
// So: one place that knows the add is a promise, and a remove that waits for it.

//: Every source currently shown, and the handle that shows it. Weak, so a source nobody
//: holds any more is collectable; keyed by the source because that is what the collection
//: can end up holding twice.
const shown = new WeakMap();


/// Put a data source on the globe and hand back the way to take it off.
///
/// Args:
///   viewer: the Cesium viewer.
///   source: the data source to show.
///
/// Returns:
///   handle: `{ source, ready, remove }` - `ready` settles when the add has landed.
///
/// **Adding one that is already shown is refused rather than repeated**, because two
/// entries for one source is the state the crash needs and no caller ever wants it.
export function showLayer(viewer, source) {
  // **`contains` alone is not enough, and the test that says so is in the suite.** Two
  // calls made before either add has landed both see a collection that does not hold the
  // source yet, and both add it - which is precisely the double entry this file exists to
  // prevent. What has to be remembered is the SHOWING, not the collection's state, so a
  // second call while the first is in flight is handed the first.
  const open = shown.get(source);
  if (open) return open;

  const ready = viewer.dataSources.contains(source)
    ? Promise.resolve(source)
    : Promise.resolve(viewer.dataSources.add(source));
  let taken = false;

  const handle = {
    source,
    ready,
    /// Take the layer off, once the add it undoes has actually landed.
    ///
    /// The wait is the whole point: removing during a pending add is the no-op that leaves
    /// the duplicate behind. Guarded against a second call, because the second removal is
    /// what strips the visualizers a surviving entry is still being read for.
    async remove(destroy = true) {
      if (taken) return false;
      taken = true;
      try {
        await ready;
      } catch {
        return false; // An add that failed has nothing to undo.
      }
      if (shown.get(source) === handle) shown.delete(source);
      if (!viewer.dataSources.contains(source)) return false;
      return viewer.dataSources.remove(source, destroy);
    },
  };
  shown.set(source, handle);
  return handle;
}
