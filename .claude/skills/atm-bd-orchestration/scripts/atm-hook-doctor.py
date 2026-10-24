#!/usr/bin/env python3
"""Check an installed atm-bd-orchestration copy against its install record, with no host-repository tooling.

    python3 atm-hook-doctor.py [--dest <.claude or .codex dir>]   one ok|modified|missing|extra line per path, PASS|FAIL
    python3 atm-hook-doctor.py doctor                           the doctor hook: request on stdin, reply on stdout

`atm-bd skill` writes the record (atm-bd-install.json) with the sc-sha of every file as installed; an install.py install
writes none, which fails closed. Only the standard library and sc_sha are imported.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

RECORD_NAME = "atm-bd-install.json"
SC_SHA_FIX = "uv pip install 'sc-sha>=1.7.0,<2'"
CODES = {"modified": "skill_file_modified", "missing": "skill_file_missing", "extra": "skill_file_extra"}


class RecordError(ValueError):
    """The install record exists but is not a record atm-bd skill writes."""

try:
    import sc_sha
except ImportError:
    sc_sha = None


def install_folder() -> Path:
    """<repo>/.claude (or .codex): this script is <install folder>/skills/atm-bd-orchestration/scripts/atm-hook-doctor.py."""
    return Path(__file__).resolve().parents[3]


def _sha(data: bytes) -> str:
    if sc_sha is None:
        raise RuntimeError(f"sc_sha is unavailable; {SC_SHA_FIX}")
    return sc_sha.calculate_hash({"utf8_file_bytes": data})["sha256"]


def _shipped(directory: Path) -> set[Path]:
    return {path for path in directory.rglob("*")
            if path.is_file() and "__pycache__" not in path.relative_to(directory).parts and path.suffix != ".pyc"}


def check(dest: Path) -> tuple[list[tuple[str, str]], dict | None]:
    """(status, repository-relative path) per recorded path and per extra file in a recorded skill directory, sorted
    by path; the record, or None when there is none."""
    record_path = dest / RECORD_NAME
    if not record_path.is_file():
        return [("missing", RECORD_NAME)], None
    try:
        record = json.loads(record_path.read_text(encoding="utf-8"))
        recorded = {entry["path"]: entry["sc_sha"] for entry in record["files"]}
    except (OSError, UnicodeDecodeError, json.JSONDecodeError, KeyError, TypeError) as error:
        raise RecordError(f"{record_path} is not an atm-bd install record: {error!r}") from error
    if not all(isinstance(path, str) and isinstance(sha, str) for path, sha in recorded.items()):
        raise RecordError(f"{record_path} is not an atm-bd install record: paths and sc_sha values must be strings")
    repo = dest.parent
    prefix = dest.name + "/skills/"
    skill_dirs = {path[len(prefix):].split("/", 1)[0] for path in recorded if path.startswith(prefix) and path.count("/") >= 3}
    present = {path.relative_to(repo).as_posix() for name in skill_dirs if (dest / "skills" / name).is_dir()
               for path in _shipped(dest / "skills" / name)}
    results = []
    for path in sorted(recorded.keys() | present):
        file = repo / path
        if path not in recorded:
            results.append(("extra", path))
        elif not file.is_file():
            results.append(("missing", path))
        else:
            results.append(("ok" if _sha(file.read_bytes()) == recorded[path] else "modified", path))
    return results, record


def remediation(dest: Path, record: dict | None) -> str:
    version = f" {record['version']}" if record and isinstance(record.get("version"), str) else ""
    codex = " --codex" if dest.name == ".codex" else ""
    return f"atm-bd skill{version} --dest {dest.parent}{codex} --overwrite"


def doctor(request: dict) -> dict:
    """The doctor hook contract reply: success, message, findings."""
    dest = install_folder()
    if sc_sha is None:
        return {"success": False, "message": "sc_sha is not installed; the skill files cannot be checked",
                "findings": [{"severity": "error", "code": "sc_sha_missing",
                              "message": "the sc_sha Python module is not installed", "remediation": SC_SHA_FIX}]}
    try:
        results, record = check(dest)
    except RecordError as error:
        return {"success": False, "message": str(error),
                "findings": [{"severity": "error", "code": "install_record_unreadable", "message": str(error),
                              "remediation": remediation(dest, None)}]}
    fix = remediation(dest, record)
    if record is None:
        return {"success": False, "message": f"{dest / RECORD_NAME} is missing; the install cannot be checked",
                "findings": [{"severity": "error", "code": "install_record_missing",
                              "message": f"missing {RECORD_NAME} in {dest}", "remediation": fix}]}
    findings = [{"severity": "error", "code": CODES[status], "message": f"{status} {path}", "remediation": fix}
                for status, path in results if status != "ok"]
    if findings:
        return {"success": False, "message": f"{len(findings)} skill file(s) differ from {RECORD_NAME}", "findings": findings}
    return {"success": True, "message": f"{len(results)} skill file(s) match {RECORD_NAME}", "findings": []}


def _doctor_mode() -> int:
    try:
        request = json.loads(sys.stdin.read())
    except json.JSONDecodeError:
        print(json.dumps({
            "success": False,
            "message": "doctor request must be valid JSON",
            "findings": [{"severity": "error", "code": "request_invalid",
                          "message": "doctor request must be valid JSON",
                          "remediation": 'send a JSON object with "hook": "doctor"'}],
        }))
        return 2
    if not isinstance(request, dict) or request.get("hook") != "doctor":
        message = 'doctor request must be an object with "hook": "doctor"'
        print(json.dumps({
            "success": False,
            "message": message,
            "findings": [{"severity": "error", "code": "request_invalid", "message": message,
                          "remediation": 'send a JSON object with "hook": "doctor"'}],
        }))
        return 2
    print(json.dumps(doctor(request)))
    return 2 if sc_sha is None else 0


def main(argv: list[str] | None = None) -> int:
    argv = sys.argv[1:] if argv is None else argv
    if argv[:1] == ["doctor"] and len(argv) == 1:
        return _doctor_mode()
    parser = argparse.ArgumentParser(prog="atm-hook-doctor.py", description=__doc__.splitlines()[0] if __doc__ else "")
    parser.add_argument("--dest", type=Path, help="the installed .claude or .codex directory (default: this install)")
    args = parser.parse_args(argv)
    if sc_sha is None:
        print(f"atm-hook-doctor: the sc_sha Python module is not installed; fix: {SC_SHA_FIX}", file=sys.stderr)
        return 2
    dest = (args.dest or install_folder()).expanduser().resolve()
    try:
        results, _record = check(dest)
    except RecordError as error:
        print(f"unreadable {RECORD_NAME}: {error}")
        print("FAIL")
        return 1
    for status, path in results:
        print(f"{status} {path}")
    passed = bool(results) and all(status == "ok" for status, _path in results)
    print("PASS" if passed else "FAIL")
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
