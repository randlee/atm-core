"""Single owner for the reviewed benchmark-baseline document."""
from __future__ import annotations

from pathlib import Path

from scripts.smoke.benchmark_schema import BaselineSet


class BenchmarkBaselineError(ValueError):
    """The reviewed baseline document cannot be loaded."""


def load_baselines(path: Path) -> BaselineSet:
    """Load and validate one baseline document without making policy decisions."""
    try:
        return BaselineSet.model_validate_json(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise BenchmarkBaselineError(
            f"could not load benchmark baselines {path}: {error}"
        ) from error
