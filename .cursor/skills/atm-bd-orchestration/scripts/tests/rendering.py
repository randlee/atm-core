"""Template renders for tests, through the sc-compose wheel the scripts use (never the CLI)."""
import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "atm-beads" / "scripts"))
import sc_render  # noqa: E402


def render(template, variables) -> subprocess.CompletedProcess:
    """`sc-compose render --strict` as a completed process: returncode 0 and the text on stdout, or 1 and the
    sc-compose diagnostic on stderr. `variables` is a mapping or a JSON file."""
    if not isinstance(variables, dict):
        variables = json.loads(Path(variables).read_text())
    try:
        return subprocess.CompletedProcess([str(template)], 0, sc_render.render(template, variables), "")
    except sc_render.RenderError as exc:
        return subprocess.CompletedProcess([str(template)], 1, "", sc_render.error_text(exc))
