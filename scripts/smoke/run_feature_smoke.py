#!/usr/bin/env python3
"""Run one progressively stronger smoke feature against the selected daemon.

The runner never starts, stops, switches, or configures a daemon.  Use the
daemon-switch skill before invoking it.  Local identity comes from the normal
CLI environment: ``ATM_IDENTITY`` and ``ATM_TEAM``.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
from html import escape
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time
from typing import Any

from run_inbound_peer_smoke import PANE_TEMPLATE, compose, render_host_pane


ROOT = Path(__file__).resolve().parents[2]
FIXTURE_FEATURES = frozenset({"fast", "normal", "thorough"})
LOCALHOST = "localhost"
LOCAL_IP = "local-ip"
LOCAL_IP_ALIAS = "local-up"
CROSSHOST = "crosshost"


class SmokeError(RuntimeError):
    """A smoke prerequisite or assertion failed."""


def command(argv: list[str], timeout: float = 15.0) -> dict[str, Any]:
    try:
        completed = subprocess.run(argv, text=True, capture_output=True, check=False, timeout=timeout)
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"argv": argv, "exit_code": None, "stdout": "", "stderr": str(error)}
    return {"argv": argv, "exit_code": completed.returncode, "stdout": completed.stdout, "stderr": completed.stderr}


def require_environment() -> tuple[str, str, str]:
    identity = os.environ.get("ATM_IDENTITY", "").strip()
    team = os.environ.get("ATM_TEAM", "").strip()
    if not identity or not team:
        raise SmokeError("set ATM_IDENTITY and ATM_TEAM before running live daemon smoke")
    return os.environ.get("ATM_SMOKE_ATM", "atm"), identity, team


def parse_json(result: dict[str, Any], label: str) -> Any:
    if result["exit_code"] != 0:
        raise SmokeError(f"{label} failed: {result['stderr'].strip() or result['stdout'].strip()}")
    try:
        return json.loads(result["stdout"])
    except json.JSONDecodeError as error:
        raise SmokeError(f"{label} did not return JSON: {error}") from error


def branch_version() -> str:
    """Return the checked-out ATM release version, without trusting PATH."""
    metadata = parse_json(
        command(["cargo", "metadata", "--no-deps", "--format-version", "1"]),
        "cargo metadata",
    )
    packages = metadata.get("packages", []) if isinstance(metadata, dict) else []
    versions = {package.get("version") for package in packages if package.get("name") in {"atm", "atm-daemon"}}
    if len(versions) != 1 or not isinstance(next(iter(versions)), str):
        raise SmokeError("cargo metadata did not expose one shared atm/atm-daemon version")
    return next(iter(versions))


def message_id(value: Any) -> str:
    if isinstance(value, dict):
        for key in ("message_id", "messageId"):
            if isinstance(value.get(key), str) and value[key]:
                return value[key]
        for child in value.values():
            try:
                return message_id(child)
            except SmokeError:
                continue
    if isinstance(value, list):
        for child in value:
            try:
                return message_id(child)
            except SmokeError:
                continue
    raise SmokeError("send JSON did not contain a message ID")


def selected_message(value: Any, expected: str) -> dict[str, Any] | None:
    if isinstance(value, dict):
        for key in ("message", "selected_message"):
            child = value.get(key)
            if isinstance(child, dict) and child.get("message_id", child.get("messageId")) == expected:
                return child
        messages = value.get("messages")
        if isinstance(messages, list):
            for child in messages:
                if isinstance(child, dict) and child.get("message_id", child.get("messageId")) == expected:
                    return child
    return None


def wait_for_message(atm: str, team: str, expected: str, timeout: float = 12.0) -> dict[str, Any] | None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = command([atm, "read", "--team", team, "--message-id", expected, "--json"])
        if result["exit_code"] == 0:
            try:
                found = selected_message(json.loads(result["stdout"]), expected)
            except json.JSONDecodeError:
                found = None
            if found is not None:
                return found
        time.sleep(0.4)
    return None


def advertised_host(atm: str) -> str:
    override = os.environ.get("ATM_SMOKE_ADVERTISED_HOST", "").strip()
    if override:
        return override
    interfaces = parse_json(command([atm, "peer", "interface", "list", "--json"]), "peer interface list")
    stack = [interfaces]
    while stack:
        current = stack.pop()
        if isinstance(current, dict):
            host = current.get("advertise_host", current.get("advertised_host"))
            if current.get("enabled") is not False and isinstance(host, str) and host:
                return host
            stack.extend(current.values())
        elif isinstance(current, list):
            stack.extend(current)
    raise SmokeError("no enabled advertised host; set ATM_SMOKE_ADVERTISED_HOST")


def add_case(cases: list[dict[str, Any]], name: str, passed: bool, detail: str) -> None:
    cases.append({"name": name, "status": "PASS" if passed else "FAIL", "detail": detail})
    print(f"{'PASS' if passed else 'FAIL'} {name}: {detail}", flush=True)


def send_read_ack(cases: list[dict[str, Any]], atm: str, identity: str, team: str, host: str) -> None:
    target = f"{identity}@{team}.{host}"
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    sent = command([atm, "send", target, f"smoke-{host}-{stamp}", "--json"])
    try:
        sent_id = message_id(parse_json(sent, f"send to {host}"))
        visible = wait_for_message(atm, team, sent_id)
        add_case(cases, f"{host} send/read", visible is not None, sent_id if visible else "message not visible")
    except SmokeError as error:
        add_case(cases, f"{host} send/read", False, str(error))
    required = command([atm, "send", target, f"smoke-ack-{host}-{stamp}", "--requires-ack", "--json"])
    try:
        required_id = message_id(parse_json(required, f"ack-required send to {host}"))
        message = wait_for_message(atm, team, required_id)
        pending = bool(message and message.get("requires_ack", message.get("requiresAck")) is True)
        add_case(cases, f"{host} requires-ack/read", pending, required_id if pending else "pending acknowledgement message not visible")
        if pending:
            acknowledgement = command([atm, "ack", "--team", team, required_id, f"smoke acknowledgement {stamp}", "--json"])
            add_case(cases, f"{host} acknowledgement", acknowledgement["exit_code"] == 0, acknowledgement["stderr"].strip() or required_id)
    except SmokeError as error:
        add_case(cases, f"{host} requires-ack/read", False, str(error))


def remote_inbound(cases: list[dict[str, Any]], atm: str, identity: str, team: str, local_host: str, peers: list[str]) -> None:
    target = f"{identity}@{team}.{local_host}"
    for peer in peers:
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        remote = command(["ssh", peer, atm, "send", target, f"smoke-from-{peer}-{stamp}", "--json"], timeout=25.0)
        try:
            remote_id = message_id(parse_json(remote, f"remote send from {peer}"))
            visible = wait_for_message(atm, team, remote_id)
            add_case(cases, f"{peer} inbound send/read", visible is not None, remote_id if visible else "message not visible")
        except SmokeError as error:
            add_case(cases, f"{peer} inbound send/read", False, str(error))


def write_report(feature: str, cases: list[dict[str, Any]]) -> Path:
    directory = ROOT / "reports" / "smoke" / feature
    directory.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    report = directory / f"{stamp}.json"
    passed = all(case["status"] == "PASS" for case in cases)
    report.write_text(json.dumps({"feature": feature, "status": "PASS" if passed else "FAIL", "cases": cases}, indent=2) + "\n", encoding="utf-8")
    # Reuse the established AI.21-pre XHTML pane template instead of creating
    # a second report format for the same live-daemon evidence.
    rows = {
        case["name"]: ("pass" if case["status"] == "PASS" else "fail", case["detail"])
        for case in cases
    }
    records = [{"phase": case["name"], "passed": case["status"] == "PASS"} for case in cases]
    pane = report.with_suffix(".xhtml")
    compose(
        PANE_TEMPLATE,
        {
            "title": f"ATM smoke — {feature}",
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "host": platform.node(),
            "body_html": render_host_pane(platform.node(), None, rows, records),
        },
        pane,
    )
    compose(
        ROOT / "templates" / "smoke-report" / "inbound-peer-frame.html.j2",
        {
            "title": f"ATM smoke — {feature}",
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "pane_src": pane.name,
        },
        report.with_suffix(".html"),
    )
    panes = sorted(path for path in directory.parent.rglob("*.xhtml") if path.is_file())
    pane_html = "\n".join(
        "<section><h2>{label}</h2><iframe title=\"{title}\" src=\"{source}\"></iframe></section>".format(
            label=escape(str(path.relative_to(directory.parent))),
            title=escape(path.stem, quote=True),
            source=escape(str(path.relative_to(directory.parent)), quote=True),
        )
        for path in panes
    )
    compose(
        ROOT / "templates" / "smoke-report" / "inbound-peer-review.html.j2",
        {
            "title": "ATM smoke evidence",
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "pane_html": pane_html,
        },
        directory.parent / "index.html",
    )
    return report


def run_live(feature: str, peers: list[str]) -> int:
    atm, identity, team = require_environment()
    cases: list[dict[str, Any]] = []
    doctor = command([atm, "doctor", "--json"])
    try:
        report = parse_json(doctor, "doctor")
        expected_version = branch_version()
        client_version = report.get("client_context", {}).get("version")
        daemon_version = report.get("daemon_context", {}).get("version")
        healthy = (
            report.get("summary", {}).get("status") == "healthy"
            and report.get("runtime_status", {}).get("readiness") == "ready"
            and client_version == expected_version
            and daemon_version == expected_version
        )
        detail = (
            f"healthy/ready; expected={expected_version}, client={client_version}, daemon={daemon_version}"
            if healthy
            else f"expected={expected_version}, client={client_version}, daemon={daemon_version}"
        )
        add_case(cases, "doctor", healthy, detail)
    except SmokeError as error:
        add_case(cases, "doctor", False, str(error))
    if not all(case["status"] == "PASS" for case in cases):
        print(f"FAIL evidence: {write_report(feature, cases)}")
        return 1
    send_read_ack(cases, atm, identity, team, "localhost")
    if feature in {LOCAL_IP, CROSSHOST}:
        try:
            send_read_ack(cases, atm, identity, team, advertised_host(atm))
        except SmokeError as error:
            add_case(cases, "advertised-IP", False, str(error))
    if feature == CROSSHOST:
        if not peers:
            add_case(cases, "crosshost peers", False, "supply one or more SSH hostnames")
        else:
            try:
                remote_inbound(cases, atm, identity, team, advertised_host(atm), peers)
            except SmokeError as error:
                add_case(cases, "crosshost peers", False, str(error))
    report = write_report(feature, cases)
    passed = all(case["status"] == "PASS" for case in cases)
    print(f"{'PASS' if passed else 'FAIL'} evidence: {report}")
    return 0 if passed else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("feature", nargs="?", default="normal")
    parser.add_argument("peers", nargs="*")
    args = parser.parse_args()
    if args.feature in FIXTURE_FEATURES:
        if args.peers:
            raise SmokeError(f"fixture smoke `{args.feature}` does not accept hostnames")
        return subprocess.run([sys.executable, str(ROOT / "scripts" / "smoke" / "run.py"), args.feature, "--write-artifacts"], check=False).returncode
    feature = LOCAL_IP if args.feature == LOCAL_IP_ALIAS else args.feature
    if feature not in {LOCALHOST, LOCAL_IP, CROSSHOST}:
        raise SmokeError("supported smoke features: fast, normal, thorough, localhost, local-ip, crosshost")
    if args.peers and feature != CROSSHOST:
        raise SmokeError("hostnames are only valid with `just smoke crosshost <host...>`")
    return run_live(feature, args.peers)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except SmokeError as error:
        print(f"smoke error: {error}", file=sys.stderr)
        raise SystemExit(2)
