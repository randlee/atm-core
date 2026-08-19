"""Acceptance and baseline policy for durable admission benchmarks."""
from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from scripts.smoke.benchmark_schema import distribution
from scripts.smoke.smoke_common import SmokeError


def profile_median_admissions_per_second(profile: dict[str, Any]) -> float:
    """Return the schema-consistent midpoint rate for a complete profile."""
    rates = [float(item["admissions_per_second"]) for item in profile["intervals"]]
    return distribution(rates)["p50"] if rates else 0.0


def evaluate_profile_thresholds(
    profile: dict[str, Any], baseline_median: float | None,
    comparison_median: float | None = None,
    comparison_ratio: float = 1.0,
    comparison_strict: bool = False,
    comparison_required: bool = True,
) -> dict[str, Any]:
    """Make admission, baseline, and transport-comparison gates explicit."""
    median = profile_median_admissions_per_second(profile)
    admission_passed = all(item["passed"] for item in profile["intervals"])
    baseline_passed = baseline_median is None or median >= baseline_median
    comparison_target = None if comparison_median is None else comparison_median * comparison_ratio
    comparison_passed = (
        comparison_target is None
        or (median > comparison_target if comparison_strict else median >= comparison_target)
    )
    return {
        "admissions_per_second_minimum": 1_000,
        "median_admissions_per_second": median,
        "baseline_median_admissions_per_second": baseline_median,
        "admission_passed": admission_passed,
        "baseline_passed": baseline_passed,
        "comparison_median_admissions_per_second": comparison_median,
        "comparison_ratio": comparison_ratio if comparison_median is not None else None,
        "comparison_target_admissions_per_second": comparison_target,
        "comparison_strict": comparison_strict if comparison_median is not None else None,
        "comparison_required": comparison_required if comparison_median is not None else None,
        "comparison_passed": comparison_passed,
        "passed": admission_passed and baseline_passed and (
            comparison_passed if comparison_required else True
        ),
    }


def load_baseline_median(
    path: Path | None, transport: str, peer_wire_security: str, frames_per_connection: int,
) -> float | None:
    """Read a prior compatible one-profile evidence artifact when requested."""
    if path is None:
        return None
    try:
        baseline = json.loads(path.read_text(encoding="utf-8"))
        if baseline["transport"] != transport:
            raise SmokeError(
                f"capacity baseline transport {baseline['transport']!r} does not match {transport!r}"
            )
        if baseline.get("peer_wire_security") != peer_wire_security:
            raise SmokeError(
                "capacity baseline peer_wire_security does not match the selected profile"
            )
        if baseline["frames_per_connection"] != frames_per_connection:
            raise SmokeError(
                "capacity baseline frames_per_connection does not match the selected profile"
            )
        return validated_profile_median(baseline, "capacity baseline")
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise SmokeError(f"could not read admission-capacity baseline {path}: {error}") from error


def validated_profile_median(payload: dict[str, Any], label: str) -> float:
    """Return a median only from evidence that passed its own methodology gates."""
    if not payload.get("passed", False):
        raise SmokeError(f"{label} did not pass its own acceptance gates")
    if payload.get("sample_count", 0) < payload.get("minimum_sample_count", 10):
        raise SmokeError(f"{label} has fewer than its required samples")
    if payload.get("run_duration_s", 0.0) < payload.get("target_duration_s", 20.0):
        raise SmokeError(f"{label} did not run for its required duration")
    return recorded_profile_median(payload, label)


def recorded_profile_median(payload: dict[str, Any], label: str) -> float:
    """Read the recorded median without treating a failed run as a baseline."""
    try:
        if payload.get("schema_version") in {3, 4}:
            return float(payload["metrics"]["admissions_per_second"]["p50"])
        return profile_median_admissions_per_second(payload["runs"][0])
    except (KeyError, TypeError, ValueError, IndexError) as error:
        raise SmokeError(f"invalid {label}") from error


def baseline_reference(path: Path | None) -> dict[str, Any] | None:
    """Retain the comparison artifact identity alongside its measured median."""
    if path is None:
        return None
    try:
        baseline = json.loads(path.read_text(encoding="utf-8"))
        return {
            "source_revision": baseline.get("source_revision"),
            "generated_at": baseline.get("generated_at"),
            "run_duration_s": baseline.get("run_duration_s"),
            "passed": bool(baseline.get("passed", False)),
            "median_admissions_per_second": recorded_profile_median(baseline, "capacity baseline"),
        }
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise SmokeError(f"could not describe admission-capacity baseline {path}: {error}") from error
