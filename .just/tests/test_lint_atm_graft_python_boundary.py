from pathlib import Path
import shutil, sys, tempfile, unittest

JUST = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(JUST))
from lint_atm_graft_python_boundary import collect_violations

class Tests(unittest.TestCase):
    def fixture(self, root):
        for path in ("boundaries/atm-graft-python/hermes-graft-binding.toml", "crates/atm-graft-python/pyproject.toml"):
            source = JUST.parent / path; target = root / path; target.parent.mkdir(parents=True, exist_ok=True); shutil.copy2(source, target)
    def test_undeclared_dependency_fails(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); self.fixture(root); path = root / "crates/atm-graft-python/pyproject.toml"
            path.write_text(path.read_text().replace('dependencies = ["pydantic>=2,<3"]', 'dependencies = ["pydantic>=2,<3", "other"]'))
            self.assertTrue(collect_violations(root))
