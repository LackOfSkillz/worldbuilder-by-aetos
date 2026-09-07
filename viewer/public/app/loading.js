// A loading bar, and a globe that stays hidden until it is worth looking at.
//
// **The problem is not that loading is slow, it is that a half-loaded globe looks finished.**
// Cesium draws the coarsest tiles first and refines downward, so an unfinished world is a
// complete-looking one made of the wrong resolution - blurred coastlines, missing mountains,
// water at the wrong level. Somebody judging a world at that moment is judging a picture the
// engine does not agree with, and this project has already been bitten once by a picture that
// looked like a result and was an artefact.
//
// **What "fully rendered" means here is a measurement, not a guess.** Cesium publishes exactly
// the signal needed: `globe.tilesLoaded`, and `scene.globe.tileLoadProgressEvent` firing with
// the number of tiles still queued. The bar reads those rather than a timer, so it cannot claim
// progress the renderer has not made.
//
// **Two things had to be counted, not one.** The tiles are the visible half; the water solve is
// the other, and on the default world it takes 1.4 seconds during which the globe is drawable
// and the lakes are not. Revealing on tiles alone would show a world whose lakes appear
// afterwards, which is the same lie in a smaller costume.

/// Below this many outstanding tiles the globe is considered settled.
const SETTLED_QUEUE = 0;

/// How long the queue has to STAY empty before the globe is revealed.
///
/// **`globe.tilesLoaded` is not a terminal state and waiting for it hangs.** The first version
/// of this file used it as the condition and revealed on the 45-second timeout, having never
/// once seen it true: a globe you can keep flying around keeps requesting tiles, and the relief
/// layer streams behind the terrain, so the flag flickers rather than latching. Measured on the
/// default world, the reveal took the full timeout and the user got 45 seconds of grey.
///
/// What settles reliably is the QUEUE, and what makes it trustworthy is holding it - a queue
/// that touches zero for one frame is between requests, and one that stays there for most of a
/// second has finished the view.
const SETTLE_HOLD_MS = 700;

/// The largest queue seen this load, so the bar has a denominator that means something.
/// A bar whose maximum is whatever the queue happens to be right now runs backwards.
function makeProgress() {
  let peak = 0;
  return (outstanding) => {
    peak = Math.max(peak, outstanding);
    if (peak === 0) return 1;
    return Math.min(1, 1 - outstanding / peak);
  };
}

/// Build the overlay. Returns `{ element, update, finish }`.
export function buildOverlay(document_ = document) {
  const overlay = document_.createElement("div");
  overlay.id = "wb-loading";
  overlay.setAttribute("role", "status");
  overlay.setAttribute("aria-live", "polite");

  const card = document_.createElement("div");
  card.className = "wb-loading-card";

  const title = document_.createElement("div");
  title.className = "wb-loading-title";
  title.textContent = "building the world";

  const track = document_.createElement("div");
  track.className = "wb-loading-track";
  const fill = document_.createElement("div");
  fill.className = "wb-loading-fill";
  track.append(fill);

  const step = document_.createElement("div");
  step.className = "wb-loading-step";
  step.textContent = "starting the engine";

  card.append(title, track, step);
  overlay.append(card);

  return {
    element: overlay,
    update(fraction, label) {
      fill.style.width = `${Math.round(Math.max(0, Math.min(1, fraction)) * 100)}%`;
      if (label) step.textContent = label;
    },
    finish() {
      fill.style.width = "100%";
      step.textContent = "ready";
      overlay.classList.add("wb-loading-done");
      // Removed rather than hidden, so it cannot intercept a click on the globe beneath it.
      setTimeout(() => overlay.remove(), 420);
    },
  };
}

/// Hold the globe back until it is finished, showing progress meanwhile.
///
/// Args:
///   viewer: the Cesium viewer.
///   options.waitFor: a promise resolving when the non-tile work is done - the water solve. The
///     reveal waits for BOTH this and the tiles, because a globe with no lakes on it is not a
///     finished picture.
///   options.timeoutMs: reveal regardless after this long. **A loading screen that can hang
///     forever is worse than one that lies occasionally** - a tile server that never answers
///     would otherwise leave a permanent grey card and no way past it.
///
/// Returns a promise resolving `{ revealed, reason, ms }`.
export function holdUntilRendered(viewer, options = {}) {
  const { waitFor = null, timeoutMs = 45000, document_ = document } = options;
  const overlay = buildOverlay(document_);
  const canvas = viewer && viewer.canvas;
  const container = (canvas && canvas.parentElement) || document_.body;
  container.append(overlay);
  // The globe is hidden by opacity rather than `display`, because Cesium does not render a
  // zero-size canvas and would never finish loading the tiles we are waiting for.
  if (canvas) canvas.classList.add("wb-canvas-loading");

  const started = performance.now();
  const progress = makeProgress();
  let sideDone = waitFor === null;
  let tilesDone = false;
  let finished = false;
  let emptySince = null;

  if (waitFor) {
    Promise.resolve(waitFor).then(() => { sideDone = true; }, () => { sideDone = true; });
  }

  return new Promise((resolve) => {
    const done = (reason) => {
      if (finished) return;
      finished = true;
      if (remove) remove();
      clearInterval(poll);
      if (canvas) canvas.classList.remove("wb-canvas-loading");
      overlay.finish();
      resolve({ revealed: true, reason, ms: Math.round(performance.now() - started) });
    };

    const onProgress = (outstanding) => {
      const fraction = progress(outstanding);
      // Tiles are most of the wait but not all of it, so they own most of the bar. The last
      // tenth belongs to whatever `waitFor` is doing, which is why the bar does not sit at
      // 100% while the lakes are still resolving.
      overlay.update(fraction * 0.9 + (sideDone ? 0.1 : 0),
        outstanding > SETTLED_QUEUE
          ? `${outstanding} tiles to draw`
          : sideDone ? "ready" : "resolving water");
      if (outstanding <= SETTLED_QUEUE) {
        if (emptySince === null) emptySince = performance.now();
        tilesDone = performance.now() - emptySince >= SETTLE_HOLD_MS;
      } else {
        emptySince = null;
        tilesDone = false;
      }
      if (tilesDone && sideDone) done("rendered");
    };

    let remove = null;
    try {
      remove = viewer.scene.globe.tileLoadProgressEvent.addEventListener(onProgress);
    } catch {
      remove = null;
    }

    // Polled as well as evented. `tileLoadProgressEvent` does not fire when there was never
    // anything to load, so an already-cached world would wait forever on the event alone.
    const poll = setInterval(() => {
      if (performance.now() - started > timeoutMs) {
        done("timeout");
        return;
      }
      // Polled as well as evented, and on the same held-queue rule. The event does not fire
      // when there was never anything to load, so a cached world would otherwise wait for the
      // timeout it is meant to avoid.
      if (!sideDone) return;
      if (viewer.scene.globe.tilesLoaded) {
        if (emptySince === null) emptySince = performance.now();
        if (performance.now() - emptySince >= SETTLE_HOLD_MS) done("rendered");
      }
    }, 250);
  });
}
