// Saving a world, reopening it, and showing where its areas are.
//
// **The planet half of a save is already solved and the mechanism is determinism.** A generated
// world is entirely described by its seed and its parameters, which is the whole point of the
// point-evaluable design - the same query string reproduces the same planet, bit for bit, which
// the parity corpus checks on 127,659 values every run. The address bar has therefore been a
// complete save format all along.
//
// What was missing is not persistence but ergonomics, and it is a short list:
//
//   - a NAME, so a world is "Aerthos" rather than four hundred characters of query string
//   - a LIST to reopen from, rather than browser history
//   - a FILE that can be checked into a repository, mailed, or handed to somebody
//   - a GENERATOR VERSION stamped in it, so a saved world opened against a newer engine either
//     reproduces or REFUSES. A planet that quietly renders differently after an engine change is
//     worse than one that will not open.
//
// **The authored half is the part that genuinely needs storage.** Placed areas, their anchors and
// bearings, port mappings and builder overrides are nobody's function of a seed - they are
// somebody's work, and losing them loses the only copy. So a worldfile is a small deterministic
// header plus everything a human decided, and the two halves cost very different amounts.
//
// The file this writes is the same shape `evennia_roundtrip/worldfile.py` reads, so a world saved
// here can be handed straight to the importer and come back with its areas filled in.

/// Bumped when a field changes meaning. Adding an optional field does not bump it.
export const WORLDFILE_VERSION = 1;

/// This build. A worldfile stamped with anything else is refused rather than reopened.
export const GENERATOR_VERSION = "0.1.0-roundtrip";

/// Where the reopen list lives. Per browser, and deliberately not the file - the file is the
/// thing you keep, this is the thing that saves you retyping a name.
const STORE_KEY = "wb.worlds";

/// Query parameters that describe the VIEW rather than the planet. Everything else is saved.
///
/// **This was an allowlist and the allowlist was wrong, one save later.** The first version
/// named the twenty-four parameters it believed described a planet, reasoning that a stray
/// presentation key would make two identical worlds compare unequal. The owner's first real
/// save dropped `mtnCount` and `mtnWanderWave` on the floor, silently, because they were not on
/// the list - so the file described a DIFFERENT planet from the one on screen, and the version
/// check that exists to prevent exactly that could not see it, because the file was perfectly
/// well-formed.
///
/// An allowlist fails closed on the thing it knows and open on the thing it does not, which is
/// backwards for a save format: a new engine parameter appears and every file written after it
/// is quietly incomplete. A denylist fails the other way - a new presentation key gets saved
/// harmlessly until somebody notices. **When the two failure modes are "loses the world" and
/// "carries a spare field", the spare field wins.**
const VIEW_ONLY_KEYS = new Set([
  "fly", "trace", "loading", "net-probe", "bench", "verify", "digest", "shoot",
]);

/// Values that cannot be what they claim to be.
///
/// The same save carried `gully: '65"'` - a trailing quote the owner picked up copying a URL
/// out of a shell command. The engine treated it as unparseable and fell back to canonical, so
/// the world on screen was NOT the world the parameter named, and the file recorded the
/// parameter rather than the world. A save that records an input the engine rejected is a save
/// of a planet nobody has seen.
export function suspectValues(planet) {
  const suspect = [];
  for (const [key, value] of Object.entries(planet)) {
    if (key === "seed") continue;
    if (value !== "" && !Number.isFinite(Number(value))) suspect.push([key, value]);
  }
  return suspect;
}

/// Read the planet out of a query string.
export function planetFromSearch(search) {
  const params = new URLSearchParams(search);
  const planet = {};
  for (const [key, value] of params.entries()) {
    if (VIEW_ONLY_KEYS.has(key)) continue;
    planet[key] = value;
  }
  return planet;
}

/// Turn a saved planet back into a query string. Sorted, so two saves of one world match.
export function searchFromPlanet(planet) {
  const params = new URLSearchParams();
  for (const key of Object.keys(planet).sort()) params.set(key, planet[key]);
  const query = params.toString();
  return query ? `?${query}` : "";
}

/// Build a worldfile from what is on screen.
///
/// `areas` is whatever the importer last produced for this world, or an empty list. A world with
/// no areas placed yet is a legitimate save - it is the planet somebody liked, which is exactly
/// what the owner asked to be able to keep.
/// Assemble a worldfile.
///
/// **`features` is not optional decoration and leaving it out lost work.** A painted
/// mountain is a feature record and nothing else; the planet block is twenty-four sliders
/// and cannot hold one. A save that wrote the planet and the areas and dropped the features
/// silently threw away every stroke somebody had painted - the file looked complete, opened
/// without complaint, and drew a world with no mountains in it.
export function buildWorldfile(name, search, areas = [], features = [], live = null) {
  const planet = planetFromSearch(search);
  // **The four the reader cannot do without are always written, even when the URL is
  // silent about them.** `planetFromSearch` records what the query string says, and a
  // query string that omits `radius` describes a planet drawn at the viewer's default -
  // so the file named a world it could not rebuild, and the Python reader refused it with
  // "planet block is missing radius". These are read off the world that is actually on
  // screen, which is the one being saved.
  if (live) {
    const required = { seed: live.seed, radius: live.radiusM,
                       plates: live.plateCount, land: live.landFraction };
    for (const [key, value] of Object.entries(required)) {
      if (planet[key] === undefined && value !== undefined && value !== null) {
        planet[key] = String(value);
      }
    }
  }
  return {
    worldfile_version: WORLDFILE_VERSION,
    generator: { name: "worldbuilder", version: GENERATOR_VERSION },
    name,
    saved_at: new Date().toISOString(),
    planet,
    features,
    areas,
  };
}

/// Refuse a worldfile rather than reinterpret it. No silent substitution.
export function checkVersion(document) {
  if (document.worldfile_version !== WORLDFILE_VERSION) {
    throw new Error(
      `worldfile schema ${document.worldfile_version}, this build reads ${WORLDFILE_VERSION}`,
    );
  }
  const stamped = document.generator && document.generator.version;
  if (stamped !== GENERATOR_VERSION) {
    throw new Error(
      `worldfile was generated by ${stamped}, this build is ${GENERATOR_VERSION}; the same ` +
      "seed would not reproduce the same planet, so it will not be opened",
    );
  }
  return document;
}

// ---------------------------------------------------------------------------------------------
// The list. localStorage, wrapped, because it throws in a private window and returns nothing in
// a fresh one - and a save button that explodes on a browser setting is worse than no button.

export function savedWorlds() {
  try {
    return JSON.parse(localStorage.getItem(STORE_KEY) || "[]");
  } catch {
    return [];
  }
}

export function remember(document) {
  const kept = savedWorlds().filter((entry) => entry.name !== document.name);
  kept.unshift({
    name: document.name,
    saved_at: document.saved_at,
    search: searchFromPlanet(document.planet),
    areas: (document.areas || []).length,
  });
  try {
    localStorage.setItem(STORE_KEY, JSON.stringify(kept.slice(0, 40)));
  } catch {
    // A browser that will not store is not a reason to lose the download.
  }
  return kept;
}

export function forget(name) {
  try {
    localStorage.setItem(
      STORE_KEY, JSON.stringify(savedWorlds().filter((entry) => entry.name !== name)),
    );
  } catch { /* as above */ }
}

// ---------------------------------------------------------------------------------------------
// Autosave.
//
// **This is not a convenience and the design record already said so:** *"An author who places
// forty areas and loses them to a crashed tab will not come back. Autosave belongs in the first
// version of the builder, not a later one."* It was written down and then not built, and the
// first thing that happened was somebody refreshing a browser on a world they liked.
//
// What they lost was not the planet - the seed and the parameters were in the address bar, so
// the planet came back identical. They lost the CAMERA, because `?fly=` is read at boot and
// never written. So autosave records both, on a timer and on the way out, and the recovery is
// one click rather than an afternoon of flying around looking for a coastline.

const TRAIL_KEY = "wb.trail";
const TRAIL_MAX = 24;

/// Record where we are and what we are looking at.
export function autosave(search, camera) {
  const entry = {
    at: new Date().toISOString(),
    search: searchFromPlanet(planetFromSearch(search)),
    camera: camera || null,
  };
  let trail;
  try {
    trail = JSON.parse(localStorage.getItem(TRAIL_KEY) || "[]");
  } catch {
    trail = [];
  }
  // One entry per planet-and-place. Returning to a view already recorded refreshes its time
  // rather than filling the trail with the same spot.
  const key = `${entry.search}|${camera ? camera.join(",") : ""}`;
  const kept = trail.filter((item) => `${item.search}|${item.camera ? item.camera.join(",") : ""}` !== key);
  kept.unshift(entry);
  try {
    localStorage.setItem(TRAIL_KEY, JSON.stringify(kept.slice(0, TRAIL_MAX)));
  } catch {
    // A browser that will not store cannot be made to. Say nothing and carry on.
  }
  return entry;
}

//: Where painted work waits out a refresh.
const PAINTED_KEY = "wb.paintedWork";

/// Keep the painted features for this planet, so closing the tab does not lose them.
///
/// **Painting was the only work in this tool that nothing caught.** The camera trail is
/// autosaved every fifteen seconds and on `pagehide`; areas cross a reload in
/// `sessionStorage`; features were held in one page's memory and in nothing else, so a
/// refresh - or a save, which until now dropped them - threw away every stroke. That is the
/// one kind of work here that cannot be regenerated from a seed.
///
/// Keyed by the query string, because features are metres on a particular planet and
/// restoring them onto a different one would put a mountain range in the sea.
export function keepPainted(search, features) {
  try {
    localStorage.setItem(PAINTED_KEY, JSON.stringify({
      search, saved_at: new Date().toISOString(), features,
    }));
  } catch {
    // A full or disabled store is not a reason to fail a commit.
  }
}

/// What was painted on this planet and never saved, or null.
export function paintedWork(search) {
  try {
    const kept = JSON.parse(localStorage.getItem(PAINTED_KEY) || "null");
    if (!kept || !Array.isArray(kept.features) || !kept.features.length) return null;
    const here = new URLSearchParams(search);
    const there = new URLSearchParams(kept.search || "");
    const same = [...there.keys()].every((key) => here.get(key) === there.get(key));
    return same ? kept : null;
  } catch {
    return null;
  }
}

/// Forget it, once it is somewhere safer.
export function clearPainted() {
  try {
    localStorage.removeItem(PAINTED_KEY);
  } catch { /* nothing to do */ }
}

/// Everything autosave has recorded, newest first.
export function trail() {
  try {
    return JSON.parse(localStorage.getItem(TRAIL_KEY) || "[]");
  } catch {
    return [];
  }
}

/// The URL that puts you back where an entry was taken.
export function urlFor(entry) {
  const params = new URLSearchParams(entry.search.replace(/^\?/, ""));
  if (entry.camera) params.set("fly", entry.camera.join(","));
  const query = params.toString();
  return query ? `?${query}` : location.pathname;
}

/// Wire autosave to a live viewer. Returns a stop function.
///
/// Three triggers, because each catches what the others miss: a timer catches a long look at one
/// place, `pagehide` catches the refresh and the close, and `visibilitychange` catches a tab
/// switched away from and never returned to - which no unload event reliably fires for.
export function startAutosave(viewer, everyMs = 15000) {
  const capture = () => {
    try {
      const camera = viewer && viewer.camera;
      if (!camera) return autosave(location.search, null);
      const position = camera.positionCartographic;
      return autosave(location.search, [
        Number(((position.latitude * 180) / Math.PI).toFixed(5)),
        Number(((position.longitude * 180) / Math.PI).toFixed(5)),
        Math.round(position.height),
        Number(((camera.heading * 180) / Math.PI).toFixed(1)),
        Number(((camera.pitch * 180) / Math.PI).toFixed(1)),
      ]);
    } catch {
      return null;
    }
  };
  const timer = setInterval(capture, everyMs);
  const onHide = () => capture();
  window.addEventListener("pagehide", onHide);
  document.addEventListener("visibilitychange", onHide);
  capture();
  return () => {
    clearInterval(timer);
    window.removeEventListener("pagehide", onHide);
    document.removeEventListener("visibilitychange", onHide);
  };
}

/// Hand a worldfile to the user as a file.
export function download(document) {
  const blob = new Blob([`${JSON.stringify(document, null, 2)}\n`], {
    type: "application/json",
  });
  const url = URL.createObjectURL(blob);
  const link = window.document.createElement("a");
  link.href = url;
  link.download = `${document.name.replace(/[^A-Za-z0-9_-]+/g, "-") || "world"}.json`;
  window.document.body.appendChild(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  return link.download;
}
