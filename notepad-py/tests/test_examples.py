"""Run every examples/*.py script (tests execute the examples)."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

EXAMPLES_DIR = Path(__file__).resolve().parent.parent / "examples"


def test_all_examples_run() -> None:
    scripts = sorted(EXAMPLES_DIR.glob("*.py"))
    assert scripts, f"no examples found in {EXAMPLES_DIR}"
    for script in scripts:
        result = subprocess.run(
            [sys.executable, str(script)],
            cwd=EXAMPLES_DIR.parent,
            capture_output=True,
            text=True,
            check=False,
        )
        assert result.returncode == 0, (
            f"{script.name} failed (exit {result.returncode}):\n"
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )
