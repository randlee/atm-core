from __future__ import annotations

from pathlib import Path
import re
import tomllib
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
LINK = re.compile(r"\]\(([^)\s]+)\)")
ABSOLUTE = ("https://", "http://", "#", "mailto:")


def published_pyprojects() -> list[Path]:
    return sorted((REPO_ROOT / "crates").glob("*/pyproject.toml"))


class PypiPackageMetadataTests(unittest.TestCase):
    def test_every_published_package_has_a_pypi_description(self) -> None:
        pyprojects = published_pyprojects()
        self.assertTrue(pyprojects)
        for pyproject in pyprojects:
            with self.subTest(package=pyproject.parent.name):
                project = tomllib.loads(pyproject.read_text(encoding="utf-8"))["project"]
                self.assertTrue(project.get("description"))
                self.assertEqual(project.get("readme"), "README.md")
                readme = pyproject.parent / "README.md"
                self.assertTrue(readme.read_text(encoding="utf-8").strip())

    def test_pypi_readme_links_are_absolute(self) -> None:
        # README.md is the PyPI long description; a relative link resolves against pypi.org.
        for pyproject in published_pyprojects():
            with self.subTest(package=pyproject.parent.name):
                text = (pyproject.parent / "README.md").read_text(encoding="utf-8")
                relative = [target for target in LINK.findall(text) if not target.startswith(ABSOLUTE)]
                self.assertEqual(relative, [])


if __name__ == "__main__":
    unittest.main()
