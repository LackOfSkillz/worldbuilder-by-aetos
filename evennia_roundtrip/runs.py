"""Every generation run, kept, so any of them can be gone back to.

A populate run makes thousands of rooms from a handful of inputs, and the first ones will
be wrong. The difference between a cheap experiment and an expensive one is entirely
whether the wrong ones can be taken back, so this exists before the generator does.

**Runs are immutable and additive; rolling back never deletes.** "Undo" moves a pointer,
it does not destroy work - so a run rolled back at noon can be rolled forward at three when
it turns out the replacement was worse. A rollback that threw the old output away would
make every experiment one-way, and people stop experimenting when the undo is expensive.

**A run records its inputs, not just its outputs.** A worldfile nobody can regenerate is a
dead end: you can look at it and you cannot ask it a different question. So the region, the
quotas, the culture table, the seeds and the engine's own source fingerprint all go in the
manifest, and a run can be replayed exactly or replayed with one number changed.

**A run that crashes is marked, not silently half-written.** The status starts as `running`
and only becomes `complete` when the generator says so, because the failure that matters is
a run that died at area sixty and looks like a finished world of sixty areas.
"""

import datetime
import json
import os
import shutil

#: Where runs live under the project root.
RUNS_DIR = "runs"

#: The file naming the run currently in force.
CURRENT = "current.json"

RUNNING = "running"
COMPLETE = "complete"
FAILED = "failed"
ABANDONED = "abandoned"


def _now():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def _root(project_root):
    path = os.path.join(project_root, RUNS_DIR)
    os.makedirs(path, exist_ok=True)
    return path


class Run:
    """One generation run and everything it produced."""

    def __init__(self, project_root, run_id, manifest):
        self.project_root = project_root
        self.run_id = run_id
        self.manifest = manifest
        self.dir = os.path.join(_root(project_root), run_id)

    # -- writing -------------------------------------------------------------------
    def path(self, name):
        return os.path.join(self.dir, name)

    def write_json(self, name, payload):
        """Store one artefact as JSON and note it in the manifest."""
        with open(self.path(name), "w", encoding="utf-8") as handle:
            json.dump(payload, handle, indent=1, sort_keys=True)
        self._note(name)
        return self.path(name)

    def write_text(self, name, text):
        """Store one artefact as text - the batchcode, a report, a log."""
        with open(self.path(name), "w", encoding="utf-8") as handle:
            handle.write(text)
        self._note(name)
        return self.path(name)

    def _note(self, name):
        size = os.path.getsize(self.path(name))
        files = self.manifest.setdefault("files", {})
        files[name] = {"bytes": size}
        self._save()

    def _save(self):
        with open(self.path("manifest.json"), "w", encoding="utf-8") as handle:
            json.dump(self.manifest, handle, indent=1, sort_keys=True)

    # -- finishing -----------------------------------------------------------------
    def finish(self, status=COMPLETE, summary=None, gates=None):
        """Close the run, and only now make it eligible to become current.

        Notes:
            A run is not `complete` because it stopped. It is complete because the
            generator said the work finished - which is the distinction between a world
            of sixty areas and a run that died at area sixty.
        """
        self.manifest["status"] = status
        self.manifest["finished_at"] = _now()
        if summary is not None:
            self.manifest["summary"] = summary
        if gates is not None:
            self.manifest["gates"] = gates
        self._save()
        return self

    def fail(self, why):
        self.manifest["error"] = str(why)
        return self.finish(status=FAILED)


def begin(project_root, label, inputs, engine_fingerprint=None):
    """
    Open a run.

    Args:
        project_root (str): The repository root.
        label (str): A short human name, e.g. `"inland-sea-100"`.
        inputs (dict): Everything needed to replay this exactly - region, quotas, seeds,
            culture table, thresholds. Recorded verbatim.
        engine_fingerprint (str, optional): The generator's own source fingerprint, so a
            run made by a different engine build is recognisable as such.

    Returns:
        run (Run): Open, with status `running`.
    """
    started = _now()
    safe = "".join(c if c.isalnum() or c in "-_" else "-" for c in label)[:40]
    run_id = "%s-%s" % (started, safe)
    directory = os.path.join(_root(project_root), run_id)
    os.makedirs(directory, exist_ok=True)
    manifest = {
        "run_id": run_id, "label": label, "started_at": started,
        "status": RUNNING, "inputs": inputs, "files": {},
        "engine_fingerprint": engine_fingerprint,
    }
    run = Run(project_root, run_id, manifest)
    run._save()
    return run


def all_runs(project_root):
    """Every run, newest first, as manifests."""
    root = _root(project_root)
    found = []
    for name in sorted(os.listdir(root), reverse=True):
        manifest = os.path.join(root, name, "manifest.json")
        if os.path.isfile(manifest):
            with open(manifest, encoding="utf-8") as handle:
                found.append(json.load(handle))
    return found


def current(project_root):
    """The run currently in force, or None."""
    pointer = os.path.join(_root(project_root), CURRENT)
    if not os.path.isfile(pointer):
        return None
    with open(pointer, encoding="utf-8") as handle:
        return json.load(handle)


def adopt(project_root, run_id, worldfile_path, artefact="worldfile.json"):
    """
    Make a run the one in force: copy its worldfile into place and move the pointer.

    Args:
        project_root (str): The repository root.
        run_id (str): The run to adopt.
        worldfile_path (str): Where the live worldfile lives.
        artefact (str, optional): Which file in the run is the worldfile.

    Raises:
        ValueError: If the run did not complete. **A failed run cannot be adopted**, which
            is the whole point of tracking status: the moment a half-written world can
            become the live one, the record stops being a safety net.
    """
    root = _root(project_root)
    manifest_path = os.path.join(root, run_id, "manifest.json")
    if not os.path.isfile(manifest_path):
        raise ValueError("no run %r" % run_id)
    with open(manifest_path, encoding="utf-8") as handle:
        manifest = json.load(handle)
    if manifest.get("status") != COMPLETE:
        raise ValueError("run %r is %r, not complete; it cannot be adopted"
                         % (run_id, manifest.get("status")))
    source = os.path.join(root, run_id, artefact)
    if not os.path.isfile(source):
        raise ValueError("run %r has no %s" % (run_id, artefact))

    # The world being replaced is kept beside the run that replaces it, so an adopt is
    # itself undoable without needing the run that produced the old one.
    if os.path.isfile(worldfile_path):
        shutil.copyfile(worldfile_path, os.path.join(root, run_id, "replaced.json"))
    shutil.copyfile(source, worldfile_path)

    with open(os.path.join(root, CURRENT), "w", encoding="utf-8") as handle:
        json.dump({"run_id": run_id, "adopted_at": _now(),
                   "worldfile": os.path.abspath(worldfile_path)}, handle, indent=1)
    return manifest


def rollback(project_root, worldfile_path, to=None):
    """
    Go back to an earlier run.

    Args:
        project_root (str): The repository root.
        worldfile_path (str): Where the live worldfile lives.
        to (str, optional): The run to return to. Defaults to the most recent complete
            run that is not the one in force.

    Returns:
        manifest (dict): The run now in force.

    Notes:
        Rolling back is `adopt` pointed backwards, and it destroys nothing: the run being
        left is still on disk and can be adopted again. That is what makes trying
        something reversible rather than merely regrettable.
    """
    if to is None:
        live = (current(project_root) or {}).get("run_id")
        candidates = [m for m in all_runs(project_root)
                      if m.get("status") == COMPLETE and m["run_id"] != live]
        if not candidates:
            raise ValueError("no earlier complete run to roll back to")
        to = candidates[0]["run_id"]
    return adopt(project_root, to, worldfile_path)


def report(project_root):
    """The run history, as lines for a terminal."""
    live = (current(project_root) or {}).get("run_id")
    lines = []
    for manifest in all_runs(project_root):
        mark = "->" if manifest["run_id"] == live else "  "
        summary = manifest.get("summary") or {}
        detail = ", ".join("%s %s" % (k, v) for k, v in sorted(summary.items())[:4])
        lines.append("%s %-46s %-9s %s" % (mark, manifest["run_id"],
                                           manifest.get("status", "?"), detail))
    return "\n".join(lines) or "no runs yet"
