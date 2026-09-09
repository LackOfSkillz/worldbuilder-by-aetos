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

/// How long the queue may sit at the SAME non-zero number before the picture is called done.
///
/// **A queue that never empties is not the same as a load that never finishes.** Measured on
/// this world: the terrain provider hands out all thirty tiles, the worker pool goes to zero
/// outstanding, sixteen tiles are drawn - and Cesium's load queue sits at eighteen and stays
/// there, so `tilesLoaded` never latches and the queue rule never fires. The reveal then
/// waited the full forty-five second timeout on every single load, which is the blank screen
/// somebody actually sees.
///
/// A queue that has not moved for this long, with nothing outstanding behind it, is a picture
/// that is as finished as it is going to get. Longer than `SETTLE_HOLD_MS`, because standing
/// still briefly is normal and standing still for two seconds is not.
const STALL_HOLD_MS = 2000;

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
    setTitle(text) { title.textContent = text; },
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
  const { waitFor = null, timeoutMs = 45000, document_ = document, overlay: given = null,
    title = null, label = null } = options;
  // **An overlay may be handed in already on screen, and usually should be.** The costly part
  // of a boot happens before there is a world to hold back - loading the wasm, spinning up
  // the workers, generating the plates - so a bar built here starts after the wait it is
  // meant to describe. `main.js` puts one up before any of that and passes it in; building
  // one is the fallback for callers with nothing to adopt.
  const overlay = given || buildOverlay(document_);
  const canvas = viewer && viewer.canvas;
  const container = (canvas && canvas.parentElement) || document_.body;
  // `.element`, not the record. Appending the record appended the string "[object Object]"
  // and the card was never in the DOM at all - which is why a boot showed a blank canvas and
  // no bar, and why the only visible symptom was the forty-five second timeout it always hit.
  if (!given) container.append(overlay.element);
  // A rebuild is not a boot and should not claim to be starting an engine that has been
  // running for ten minutes.
  if (title) overlay.setTitle(title);
  if (label) overlay.update(0.02, label);
  // The globe is hidden by opacity rather than `display`, because Cesium does not render a
  // zero-size canvas and would never finish loading the tiles we are waiting for.
  if (canvas) canvas.classList.add("wb-canvas-loading");

  const started = performance.now();
  const progress = makeProgress();
  let sideDone = waitFor === null;
  let tilesDone = false;
  let finished = false;
  let emptySince = null;
  //: The queue as the last event reported it. The poll needs it because the event STOPS
  //: firing once the queue empties - the last event sets `emptySince` and cannot come back
  //: seven hundred milliseconds later to say the hold has elapsed.
  let outstandingNow = null;
  //: Whether the renderer has asked for anything yet.
  //:
  //: **An empty queue means "finished" only after it has meant "busy".** Before the terrain
  //: provider is installed the queue is legitimately zero, and treating that as settled
  //: revealed the globe seven hundred milliseconds into the boot with nothing drawn on it -
  //: a starfield where a planet goes. The timeout is still the backstop for a world that
  //: never requests a tile at all.
  let sawWork = false;
  //: The queue value the stall timer is watching, and when it stopped moving.
  let stalledAt = null;
  let stalledSince = null;

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
      outstandingNow = outstanding;
      if (outstanding > SETTLED_QUEUE) sawWork = true;
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
      if (tilesDone && sideDone && sawWork) done("rendered");
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
      // **Either signal will do, and the queue is the one that works.** `tilesLoaded`
      // flickers - the relief layer streams behind the terrain, so it can stay false long
      // after the terrain queue has emptied and the card says "ready". Waiting on it alone
      // is what made every boot sit at a full bar until the forty-five second timeout, which
      // is the same hang this file's own doc warns about, reached by the other door.
      const settled = sawWork
        && (viewer.scene.globe.tilesLoaded
            || (outstandingNow !== null && outstandingNow <= SETTLED_QUEUE));
      if (settled) {
        if (emptySince === null) emptySince = performance.now();
        if (performance.now() - emptySince >= SETTLE_HOLD_MS) done("rendered");
        return;
      }
      emptySince = null;
      // The queue is not empty. Is it moving? See `STALL_HOLD_MS`.
      if (!sawWork || outstandingNow === null) return;
      // **A hidden page is not a stalled one.** Browsers stop the render loop for a
      // backgrounded tab, so the queue stops moving for a reason that has nothing to do with
      // the picture being finished - and revealing then uncovers a globe with nothing drawn
      // on it. The timer restarts when the page comes back.
      if (document_.hidden) {
        stalledSince = null;
        stalledAt = null;
        return;
      }
      if (outstandingNow !== stalledAt || stalledSince === null) {
        stalledAt = outstandingNow;
        stalledSince = performance.now();
        return;
      }
      if (performance.now() - stalledSince >= STALL_HOLD_MS) done("stalled");
    }, 250);
  });
}
