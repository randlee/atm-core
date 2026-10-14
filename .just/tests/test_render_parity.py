"""Local-only byte parity checks for the three repository render entry points.

The shipped binaries intentionally write a terminal newline when rendering to
stdout.  This test uses their structured/file outputs so it compares each
renderer body rather than that transport framing.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts.sc_compose_render import render_file


EXAMPLE_ROOTS = (
    REPO_ROOT / ".claude/skills/atm-bd-orchestration",
    REPO_ROOT / ".claude/skills/codex-orchestration",
)
ATM_BINARY = Path(os.environ.get("ATM_PARITY_BINARY", REPO_ROOT / "target/debug/atm"))
TEMPLATE_ALIASES = {
    "dev-sanity-assignment-context": "dev-sanity-assignment",
    "plan-review-template-fix-round": "plan-review-template",
    "qa-template-fix-round": "qa-template",
}


def example_pairs() -> list[tuple[Path, Path]]:
    """Return every template with a colocated example-variable document."""
    pairs: list[tuple[Path, Path]] = []
    for root in EXAMPLE_ROOTS:
        templates = root / ("templates" if root.name == "atm-bd-orchestration" else "")
        for variables in sorted((root / "examples").glob("*.json")):
            stem = variables.stem.removesuffix("-vars")
            template_stem = TEMPLATE_ALIASES.get(stem, stem)
            matches = list(templates.glob(f"{template_stem}.*.j2"))
            if len(matches) == 1:
                pairs.append((matches[0], variables))
    return pairs


@unittest.skipIf(os.environ.get("CI"), "local-only renderer parity coverage")
class RenderParityTests(unittest.TestCase):
    def test_every_template_with_example_vars_is_byte_identical(self) -> None:
        if not ATM_BINARY.is_file():
            self.skipTest(f"build the branch CLI first: cargo build -p agent-team-mail --bin atm ({ATM_BINARY})")
        pairs = example_pairs()
        self.assertGreaterEqual(len(pairs), 30)
        for template, variables_path in pairs:
            with self.subTest(template=template.relative_to(REPO_ROOT), vars=variables_path.name):
                variables = json.loads(variables_path.read_text(encoding="utf-8"))
                atm = subprocess.run(
                    [str(ATM_BINARY), "compose", "--json", "--template", str(template), "--vars", str(variables_path)],
                    cwd=REPO_ROOT,
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertEqual(atm.returncode, 0, atm.stderr)
                atm_rendered = json.loads(atm.stdout)["body"].encode("utf-8")

                with tempfile.TemporaryDirectory() as tempdir:
                    output = Path(tempdir) / "rendered.out"
                    cli = subprocess.run(
                        [
                            "sc-compose", "render", "--root", str(REPO_ROOT), "--file",
                            str(template.relative_to(REPO_ROOT)), "--var-file", str(variables_path),
                            "--output", str(output),
                        ],
                        cwd=REPO_ROOT,
                        capture_output=True,
                        text=True,
                        check=False,
                    )
                    self.assertEqual(cli.returncode, 0, cli.stderr or cli.stdout)
                    cli_rendered = output.read_bytes()

                wheel_rendered = render_file(template, variables, root=REPO_ROOT, strict=False).encode("utf-8")
                self.assertEqual(atm_rendered, cli_rendered)
                self.assertEqual(atm_rendered, wheel_rendered)


if __name__ == "__main__":
    unittest.main()
