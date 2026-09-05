"""
`worldbuilder` must be an installed package, not something that only imports because
pytest happens to put the repo root on `sys.path`.

The defect this guards against: `maturin develop` and `pip install -e .` both currently
target one distribution name (`worldbuilder-by-aetos`), and whichever runs last wins the
dist-info and evicts the other's files from it. When the engine wheel wins, the
`worldbuilder-by-aetos` dist-info lists `worldbuilder_engine/*` and not one `worldbuilder/`
file -- so `worldbuilder` is not installed at all. Every other test in this suite still
imports it successfully anyway, because they all run from the repo root and the CPython
"script's own directory" rule silently substitutes for a real install. A subprocess run
from a directory that is not inside the repository has no such crutch: it is the only way
to actually observe whether the package is installed.

Deliberately not a `sys.path` trick, a `conftest.py` insertion, or a `.pth` file -- any of
those would reproduce the exact bug this test exists to catch instead of detecting it.
"""

import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent


def test_worldbuilder_importable_outside_the_repo():
    """`import worldbuilder` must succeed, and resolve into this repo, from a cwd that
    is not this repo -- proof the package is installed rather than merely adjacent."""
    with tempfile.TemporaryDirectory() as outside_dir:
        assert not Path(outside_dir).resolve().is_relative_to(REPO_ROOT), (
            f"test bug: {outside_dir} is inside {REPO_ROOT}, so this would not exercise "
            "anything"
        )

        result = subprocess.run(
            [sys.executable, "-c", "import worldbuilder; print(worldbuilder.__file__)"],
            cwd=outside_dir,
            capture_output=True,
            text=True,
        )

        assert result.returncode == 0, (
            "`import worldbuilder` failed from outside the repository "
            f"(cwd={outside_dir}):\nstdout: {result.stdout}\nstderr: {result.stderr}"
        )

        imported_from = Path(result.stdout.strip()).resolve()
        assert imported_from.is_relative_to(REPO_ROOT.resolve()), (
            f"worldbuilder.__file__ ({imported_from}) does not resolve inside the repo "
            f"({REPO_ROOT.resolve()}); it was imported from somewhere else entirely"
        )
