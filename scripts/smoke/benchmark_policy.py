"""Acceptance and baseline policy for durable admission benchmarks."""
from __future__ import annotations

from collections.abc import Collection
from typing import Any, Literal

from scripts.smoke.smoke_common import SmokeError


BenchmarkStatus = Literal["PASS", "FAIL", "INCOMPLETE"]


def classify_status(
    *,
    lifecycle_complete: bool | None = None,
    messages_requested: int | None = None,
    messages_admitted: int | None = None,
    messages_durable: int | None = None,
    p50_admissions_per_second: float | None = None,
    baseline_p50_floor: float | None = None,
    required_targets: Collection[str] | None = None,
    observed_targets: Collection[str] | None = None,
    target_statuses: Collection[BenchmarkStatus] | None = None,
) -> BenchmarkStatus:
    """Apply the sole v4 benchmark acceptance decision for a target or campaign.

    A target supplies lifecycle, durability, and its reviewed floor.  A
    campaign supplies its required/observed target sets and the immutable
    target statuses.  Keeping both forms here prevents runner, schema, and
    reporting code from independently deciding PASS/FAIL/INCOMPLETE.
    """
    campaign_inputs = (required_targets, observed_targets, target_statuses)
    if any(value is not None for value in campaign_inputs):
        if not all(value is not None for value in campaign_inputs):
            raise ValueError("campaign classification requires targets and target statuses")
        if any(
            value is not None
            for value in (
                lifecycle_complete,
                messages_requested,
                messages_admitted,
                messages_durable,
                p50_admissions_per_second,
                baseline_p50_floor,
            )
        ):
            raise ValueError("target and campaign classification inputs cannot be combined")
        if set(observed_targets) != set(required_targets):
            return "INCOMPLETE"
        if "INCOMPLETE" in target_statuses:
            return "INCOMPLETE"
        if "FAIL" in target_statuses:
            return "FAIL"
        return "PASS"

    if any(
        value is None
        for value in (
            lifecycle_complete,
            messages_requested,
            messages_admitted,
            messages_durable,
            baseline_p50_floor,
        )
    ):
        raise ValueError("target classification requires lifecycle, counts, and baseline")
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
