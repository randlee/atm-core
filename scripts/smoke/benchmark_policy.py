"""Acceptance and baseline policy for durable admission benchmarks."""
from __future__ import annotations

from typing import Any, Literal

from scripts.smoke.smoke_common import SmokeError


BenchmarkStatus = Literal["PASS", "FAIL", "INCOMPLETE"]


def classify_status(
    *,
    lifecycle_complete: bool,
    messages_requested: int,
    messages_admitted: int,
    messages_durable: int,
    p50_admissions_per_second: float | None,
    baseline_p50_floor: float | None,
) -> BenchmarkStatus:
    """Apply the sole v4 benchmark acceptance decision.

    The caller records the reason a lifecycle is incomplete.  This function
    intentionally has no transport comparison, platform exception, or caller
    supplied status: a complete result passes only when every requested
    message is admitted and durable and its measured p50 meets its reviewed
    per-host, per-target floor.
    """
    if not lifecycle_complete or p50_admissions_per_second is None:
        return "INCOMPLETE"
    if baseline_p50_floor is None:
        raise SmokeError("benchmark baseline floor is required for a complete result")
    if (
        messages_requested == messages_admitted == messages_durable
        and p50_admissions_per_second >= baseline_p50_floor
    ):
        return "PASS"
    return "FAIL"


def profile_median_admissions_per_second(profile: dict[str, Any]) -> float:
    """Return the schema-consistent midpoint rate for a complete profile."""
    rates = sorted(float(item["admissions_per_second"]) for item in profile["intervals"])
    if not rates:
        return 0.0
    middle = len(rates) // 2
    return rates[middle] if len(rates) % 2 else (rates[middle - 1] + rates[middle]) / 2

