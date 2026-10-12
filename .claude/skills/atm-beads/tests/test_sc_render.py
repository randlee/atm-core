"""Every render is strict: a variable the template references but was not given fails, naming it and the template."""
from __future__ import annotations

import inspect
import sys
import tempfile
import unittest
from pathlib import Path

SKILL = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SKILL / "scripts"))
import sc_render  # noqa: E402


class StrictRenderTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.dir = Path(scratch.name)

    def assert_rejected(self, template: Path, variables: dict, name: str):
        with self.assertRaises(sc_render.RenderError) as caught:
            sc_render.render(template, variables)
        error = sc_render.error_text(caught.exception)
        self.assertIn(name, error)
        self.assertIn(template.name, error)

    def test_an_undeclared_variable_is_rejected(self):
        template = self.dir / "undeclared.md.j2"
        template.write_text("given {{ given }}, missing {{ missing }}\n")
        self.assert_rejected(template, {"given": "x"}, "missing")

    def test_a_missing_required_variable_is_rejected(self):
        template = self.dir / "required.md.j2"
        template.write_text("---\nrequired_variables:\n  - needed\n---\n{{ needed }}\n")
        self.assert_rejected(template, {}, "needed")

    def test_a_json_template_that_renders_invalid_json_is_rejected(self):
        # The wheel has no checked render (sc-compose#787); sc_render's own JSON check is what refuses this.
        template = self.dir / "record.json.j2"
        template.write_text('---\nrequired_variables:\n  - a\n---\n{"a": {{ a }}, }\n')
        with self.assertRaises(sc_render.RenderError) as caught:
            sc_render.render(template, {"a": 1})
        self.assertIn("ERR_RENDER_JSON_MALFORMED", sc_render.error_text(caught.exception))

    def test_there_is_no_lenient_render(self):
        self.assertEqual(list(inspect.signature(sc_render.render).parameters), ["template", "variables"])


if __name__ == "__main__":
    unittest.main()
