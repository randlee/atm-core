#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"

if command -v python3 >/dev/null 2>&1; then
  python_bin="python3"
else
  python_bin="python"
fi

"${python_bin}" - "${repo_root}" <<'PY'
from __future__ import annotations

from pathlib import Path
import re
import sys


repo_root = Path(sys.argv[1])
sys.path.insert(0, str(repo_root / ".just"))

from lint_common import iter_workspace_rust_files


PATTERN = re.compile(
    r"let\s*_\s*=\s*.*?\.(?:emit|emit_event|emit_subsystem_event)\s*\(",
    re.DOTALL,
)


def is_test_only_path(path: Path) -> bool:
    if "tests" in path.parts:
        return True
    name = path.name
    return (
        name == "tests.rs"
        or name.startswith("test_")
        or name.endswith("_test.rs")
        or "test_support" in name
    )


findings: list[str] = []
for rust_path in iter_workspace_rust_files(repo_root):
    relative_path = rust_path.relative_to(repo_root)
    if is_test_only_path(relative_path):
        continue
    text = rust_path.read_text(encoding="utf-8")
    for match in PATTERN.finditer(text):
        line = text.count("\n", 0, match.start()) + 1
        findings.append(f"{relative_path}:{line}: silent observability emit discard; use emit_or_warn/emit_event_or_warn")

if findings:
    print("silent-emit failed")
    for finding in findings:
        print(finding)
    raise SystemExit(1)

print("silent-emit passed")
PY
