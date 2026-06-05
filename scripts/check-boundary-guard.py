#!/usr/bin/env python3
"""CLI wrapper for the boundary-guard repository check."""

from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from scripts.check_boundary_guard import main


if __name__ == "__main__":
    raise SystemExit(main())
