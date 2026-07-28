"""HTML rendering for progressive feature-smoke evidence."""
from __future__ import annotations

from html import escape
import re
from typing import Any


def render_feature_pane(feature: str, cases: list[dict[str, Any]], host: str) -> str:
    """Render one endpoint's local and cross-host evidence without prose."""
    local_cases = [case for case in cases if case["origin"] == host and case["destination"] == host]
    cross_cases = [
        case
        for case in cases
        if case["origin"] != case["destination"] and host in {case["origin"], case["destination"]}
    ]
    peer_hosts = list(
        dict.fromkeys(
            endpoint
            for case in cross_cases
            for endpoint in (case["origin"], case["destination"])
            if endpoint != host
        )
    )
    header = render_host_header(host, local_cases)
    local_ladder_cases = [
        case
        for case in local_cases
        if case["name"] != "doctor"
        and not case["name"].endswith("doctor/version")
        and not case["name"].endswith("advertised host")
    ]
    local_section = render_case_section(f"ONE-COMPUTER TEST — {host}", local_ladder_cases)
    cross_title = f"CROSS-HOST TEST — {host} ↔ {', '.join(peer_hosts)}" if peer_hosts else "CROSS-HOST TEST"
    cross_section = render_cross_host_section(cross_title, [host, *peer_hosts], cases, cross_cases)
    return f"<h1>{escape(host)} — ATM smoke: {escape(feature)}</h1>{header}{local_section}{cross_section}"


def render_host_header(host: str, cases: list[dict[str, Any]]) -> str:
    """Render the compact preflight facts that apply to one computer."""
    ip_address, version, doctor_detail = host_preflight_facts(host, cases)
    doctor_passed = doctor_detail.startswith("PASS")
    return (
        "<table><tbody>"
        f"<tr><th>Advertised IP</th><td>{escape(ip_address)}</td></tr>"
        f"<tr><th>ATM version</th><td>{escape(version)}</td></tr>"
        f"<tr class=\"{'pass' if doctor_passed else 'fail'}\"><th>Doctor</th><td>{escape(doctor_detail)}</td></tr>"
        "</tbody></table>"
    )


def host_preflight_facts(host: str, cases: list[dict[str, Any]]) -> tuple[str, str, str]:
    """Extract one daemon's advertised IP, version, and doctor result."""
    self_cases = [
        case
        for case in cases
        if case.get("origin", host) == host and case.get("destination", host) == host
    ]
    doctor_cases = [case for case in self_cases if case["name"] == "doctor" or case["name"].endswith("doctor/version")]
    doctor = doctor_cases[-1] if doctor_cases else None
    advertised = next((case for case in reversed(self_cases) if case["name"].endswith("advertised host")), None)
    ip_address = advertised["detail"] if advertised is not None else "unknown"
    if doctor is None:
        return ip_address, "unknown", "NOT RUN"
    match = re.search(r"(?:ATM |client=)([^,;\s]+)", doctor["detail"])
    version = match.group(1) if match is not None else "unknown"
    doctor_passes = sum(case["status"] == "PASS" for case in doctor_cases)
    doctor_result = (
        f"PASS {doctor_passes}/{len(doctor_cases)}"
        if doctor_passes == len(doctor_cases)
        else f"FAIL {doctor_passes}/{len(doctor_cases)}: {next(case['detail'] for case in doctor_cases if case['status'] != 'PASS')}"
    )
    return ip_address, version, doctor_result


def render_cross_host_section(title: str, hosts: list[str], all_cases: list[dict[str, Any]], cross_cases: list[dict[str, Any]]) -> str:
    """Show both link endpoints before listing directional checks."""
    preflight_rows = ""
    for endpoint in hosts:
        ip_address, version, doctor = host_preflight_facts(endpoint, all_cases)
        preflight_rows += "<tr><td>{host}</td><td>{ip_address}</td><td>{version}</td><td>{doctor}</td></tr>".format(
            host=escape(endpoint), ip_address=escape(ip_address), version=escape(version), doctor=escape(doctor)
        )
    return (
        f"<h2>{escape(title)}</h2>"
        "<table><thead><tr><th>Computer</th><th>IP address used</th><th>ATM version</th><th>Doctor</th></tr>"
        f"</thead><tbody>{preflight_rows}</tbody></table>"
        + render_case_section("Connection checks", cross_cases)
    )


def render_case_section(title: str, cases: list[dict[str, Any]]) -> str:
    """Render one concise ladder table; no narrative or duplicate logs."""
    rows = "".join(
        "<tr class=\"{status}\"><td>{marker}</td><td>{origin}</td><td>{destination}</td><td>{name}</td><td>{detail}</td></tr>".format(
            status="pass" if case["status"] == "PASS" else "fail",
            marker="✓" if case["status"] == "PASS" else "✗",
            origin=escape(case["origin"]), destination=escape(case["destination"]),
            name=escape(case["name"]), detail=escape(case["detail"]),
        )
        for case in summarize_cases(cases)
    )
    return (
        f"<h2>{escape(title)}</h2>"
        "<table><thead><tr><th>Status</th><th>Origin</th><th>Destination</th><th>Ladder step</th><th>Result / message ID</th></tr>"
        f"</thead><tbody>{rows}</tbody></table>"
    )


def summarize_cases(cases: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Collapse repeated attempts into one visible row without discarding failures."""
    grouped: dict[tuple[str, str, str], list[dict[str, Any]]] = {}
    for case in cases:
        grouped.setdefault((case["origin"], case["destination"], case["name"]), []).append(case)
    summarized: list[dict[str, Any]] = []
    for (origin, destination, name), attempts in grouped.items():
        passed = [case for case in attempts if case["status"] == "PASS"]
        total = len(attempts)
        all_passed = len(passed) == total
        evidence = attempts[-1]["detail"] if all_passed else next(
            case["detail"] for case in attempts if case["status"] != "PASS"
        )
        summarized.append({
            "origin": origin, "destination": destination, "name": name,
            "status": "PASS" if all_passed else "FAIL",
            "detail": f"{len(passed)}/{total} PASS · {evidence}",
        })
    return summarized
