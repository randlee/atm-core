"""Choose CI operating-system legs from the event and target branch."""

from __future__ import annotations

import argparse
import json


ALL_OS = ("ubuntu-latest", "macos-latest", "windows-latest")
PYTHON = {"ubuntu-latest": "3.14.7", "macos-latest": "3.14.7", "windows-latest": "3.12.10"}
RELEASE_ARTIFACT = {
    "ubuntu-latest": "linux-x86_64",
    "macos-latest": "macos-aarch64",
    "windows-latest": "windows-x86_64",
}


def select_matrices(event: str, base: str, ref: str, workflow_call: bool) -> dict[str, str]:
    full_os = (
        workflow_call
        or event == "workflow_call"
        or (event == "pull_request" and base in {"develop", "main"})
        or (event == "push" and ref in {"refs/heads/develop", "refs/heads/main"})
    )
    oses = ALL_OS if full_os else ALL_OS[:1]
    return {
        "full_os": str(full_os).lower(),
        "lint_matrix": json.dumps([{"os": os, "python": PYTHON[os]} for os in oses]),
        "test_matrix": json.dumps(
            [
                {"os": os, "python": PYTHON[os], "release_artifact": RELEASE_ARTIFACT[os]}
                for os in oses
            ]
        ),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--event", required=True)
    parser.add_argument("--base", default="")
    parser.add_argument("--ref", default="")
    parser.add_argument("--workflow-call", default="false")
    args = parser.parse_args()
    matrices = select_matrices(args.event, args.base, args.ref, args.workflow_call == "true")
    for key, value in matrices.items():
        print(f"{key}={value}")


if __name__ == "__main__":
    main()
