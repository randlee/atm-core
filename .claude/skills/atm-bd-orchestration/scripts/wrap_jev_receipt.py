#!/usr/bin/env python3
# pyright: strict
"""Wrap the raw stdout of one `jev_client.py --assignment` call into an sc-sanity-jev reply. Transport only.

Usage:
  wrap_jev_receipt.py --assignment <file> --client-output <file> [--findings <file>] [--final <file> | --for-triage]
  wrap_jev_receipt.py --assignment <file> --reply <file> [--findings <file>] --final <file>

`--assignment` is the assignment the checker received (fenced or plain JSON). `--client-output` is the client's
stdout, unchanged. `--reply` is a reply this script printed before; dev-sanity uses it to add its triaged
`--final` ({choice, reason, parts}) to a split reply whose parts disagree. Parts that disagree need `--final`, or
`--for-triage` from the checker: that reply carries the one skipped finding and no `jev_final`, which
`sanity-merge` rejects until dev-sanity re-wraps it with `--reply --final`. `--findings` is a JSON array: empty
(or absent) when the result is yes, exactly one {kind: skipped, file, line, issue} when it is no; with `--reply`
it defaults to the reply's findings.

On success stdout is the reply envelope: `data.jev` is the client's `data.receipt`, or for a split request its
`data.parts`, exactly as the client printed it, with every receipt verified with TYPESAFE_API_KEY by the sibling
jev_client.py. A client failure becomes the failure envelope with the client's error verbatim plus `deliverable`.
The client's outer `elapsed_s` (and, with `--reply`, the reply's) is copied unchanged into the outer result.
These are the checks `sanity-merge --reviewer sanity-jev` applies; a reply this script prints passes them.

Exit 1: a check failed; stderr names the check and the fix. This script writes nothing but stdout.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import sys
from typing import Protocol, TypedDict, runtime_checkable

CLIENT = Path(__file__).resolve().parent / "jev_client.py"
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "atm-beads" / "scripts"))
from sanity_shared import valid_elapsed_s  # noqa: E402
from operation_models import matches_typeddict  # noqa: E402
from sprint_index_common import JsonObject, JsonValue, is_json_value  # noqa: E402

FENCED = re.compile(r"\s*```(?:json)?[ \t]*\n(.*?)\n```\s*", re.S)
CHECKER_KIND = "skipped"
RERUN = ("Rerun `python3 .claude/skills/atm-bd-orchestration/scripts/jev_client.py --assignment <file>`, save its stdout "
         "unchanged to a file and pass that file as --client-output; never write or edit client output.")


class Refused(Exception):
    def __init__(self, check: str, message: str, fix: str) -> None:
        super().__init__(f"{check}: {message}. Fix: {fix}")


@runtime_checkable
class JevClient(Protocol):
    def verify_receipt(self, value: JsonValue, key: str) -> bool: ...

    def is_client_message(self, message: JsonValue) -> bool: ...


def load_client() -> JevClient:
    spec = importlib.util.spec_from_file_location("jev_client", CLIENT)
    if spec is None or spec.loader is None:
        raise Refused("client_load", f"cannot load Jev client {CLIENT}", RERUN)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    if not isinstance(module, JevClient):
        raise Refused("client_load", f"invalid Jev client {CLIENT}", RERUN)
    return module


def read_json(path: str, what: str, fix: str) -> JsonValue:
    try:
        text = Path(path).read_text()
        fenced = FENCED.fullmatch(text)
        value = json.loads(fenced.group(1) if fenced else text)
        if not is_json_value(value):
            raise ValueError("not a JSON value")
        return value
    except (OSError, ValueError) as exc:
        raise Refused(f"{what}_unreadable", f"cannot read {what} {path}: {exc}", fix)


class AssignmentIdentity(TypedDict):
    sanity_bead: str
    dev_bead: str
    commit_checked: str


class AssignmentFields(AssignmentIdentity):
    deliverable: int


class AssignmentCount(TypedDict):
    number: int
    total: int


class ReceiptFields(TypedDict):
    question: str
    choice: str


class FinalSelection(TypedDict):
    choice: str
    reason: JsonValue
    parts: JsonValue


class ParsedReceipts(TypedDict):
    receipts: list[ReceiptFields]
    labels: list[str]
    split: bool


def assignment_fields(assignment: JsonValue) -> tuple[AssignmentFields, int]:
    fix = "Pass the assignment you received, unchanged, as --assignment."
    if not isinstance(assignment, dict):
        raise Refused("assignment_fields", "assignment must be an object", fix)
    dev = assignment.get("dev_bead")
    deliverable = assignment.get("deliverable")
    if not isinstance(dev, dict) or not isinstance(deliverable, dict):
        raise Refused("assignment_fields", "assignment lacks dev_bead or deliverable", fix)
    number, total = deliverable.get("number"), assignment.get("deliverables_total")
    identity = {"sanity_bead": assignment.get("sanity_bead"), "dev_bead": dev.get("id"),
                "commit_checked": assignment.get("commit")}
    if not matches_typeddict(identity, AssignmentIdentity):
        raise Refused("assignment_fields", "assignment bead IDs and commit must be strings", fix)
    counts = {"number": number, "total": total}
    if not matches_typeddict(counts, AssignmentCount) or not 1 <= counts["number"] <= counts["total"]:
        raise Refused("assignment_fields", f"deliverable.number {number!r} is not in 1..{total!r}", fix)
    return {**identity, "deliverable": counts["number"]}, counts["total"]


def client_receipts(jev: JsonValue, client: JevClient, key: str) -> ParsedReceipts:
    """The receipts carried by `jev` (one receipt, or a split request's part list), each verified."""
    labels: list[str] = []
    if isinstance(jev, list):
        receipts: list[JsonObject] = []
        for i, part in enumerate(jev, 1):
            label = f"{i} of {len(jev)}"
            if not isinstance(part, dict) or part.get("part") != label:
                raise Refused("part_order", f"Jev part {label} is missing or mislabelled", RERUN)
            labels.append(label)
            receipt = part.get("receipt")
            if not isinstance(receipt, dict) or not client.verify_receipt(receipt, key):
                raise Refused("receipt_mac", f"no valid Jev receipt on part {label}", RERUN)
            response = part.get("response")
            answers = response.get("answers") if isinstance(response, dict) else None
            answer = answers.get("written") if isinstance(answers, dict) else None
            if not isinstance(answer, dict) or (answer.get("choice"), answer.get("probabilities")) != (
                    receipt["choice"], receipt["probabilities"]):
                raise Refused("part_response", f"Jev part {label}'s response does not match its receipt", RERUN)
            receipts.append(receipt)
        if not receipts:
            raise Refused("part_order", "the client's part list is empty", RERUN)
    else:
        if not isinstance(jev, dict) or not client.verify_receipt(jev, key):
            raise Refused("receipt_mac", "no valid Jev receipt: it was not written by a jev_client.py call", RERUN)
        receipts = [jev]
    parsed: list[ReceiptFields] = []
    for receipt in receipts:
        if receipt["question"] != "written" or receipt["choice"] not in ("yes", "no"):
            raise Refused("receipt_question", f"Jev receipt answers {receipt['question']!r} with "
                          f"{receipt['choice']!r}, not written: yes or no", RERUN)
        question, choice = receipt["question"], receipt["choice"]
        assert isinstance(question, str) and isinstance(choice, str)
        parsed.append({"question": question, "choice": choice})
    return {"receipts": parsed, "labels": labels, "split": isinstance(jev, list)}


def result_choice(parsed: ParsedReceipts, final: JsonValue,
                  for_triage: bool = False) -> str:
    """The result: the receipts' common choice, or dev-sanity's triaged `final` when they disagree; `no` (its one
    skipped finding) for a reply sent to triage."""
    choices = {receipt["choice"] for receipt in parsed["receipts"]}
    if final is not None and (not isinstance(final, dict) or final.get("choice") not in choices):
        raise Refused("choice_in_receipts", f"final selection {final!r} names no choice any receipt holds "
                      f"({sorted(choices)})", "Write --final as {choice, reason, parts} with a choice a part answered.")
    selection: FinalSelection | None = None
    if isinstance(final, dict):
        choice = final.get("choice")
        assert isinstance(choice, str)
        selection = {"choice": choice, "reason": final.get("reason"), "parts": final.get("parts")}
    if len(choices) == 1:
        return next(iter(choices))
    if not parsed["split"]:
        raise Refused("part_order", "the client's part list is missing", RERUN)
    labels = parsed["labels"]
    if selection is None and for_triage:
        return "no"
    if selection is None:
        raise Refused("final_selection", f"Jev parts disagree ({sorted(choices)}) and no final selection was given",
                      "sc-sanity-jev: rerun with --for-triage and the one skipped finding. dev-sanity: triage the "
                      "parts, then rerun with --reply <reply> --final <file> holding {choice, reason, parts}.")
    reason = selection["reason"]
    if not isinstance(reason, str) or not reason.strip():
        raise Refused("final_reason", "the final selection has no reason",
                      "Add a nonempty `reason` citing every part's result to the --final file.")
    if selection["parts"] != labels:
        raise Refused("final_parts", f"final selection parts {selection['parts']!r} are not {labels!r}",
                      f"Set `parts` in the --final file to {json.dumps(labels)}.")
    return selection["choice"]


class FindingDependency(TypedDict):
    deliverable: int
    file: str
    line: int


class FindingFields(TypedDict):
    kind: str
    file: str
    line: int
    issue: str


class Finding(FindingFields):
    dependencies: list[FindingDependency]
    raw: JsonObject


def check_findings(findings: JsonValue, choice: str, total: int) -> list[Finding]:
    fix = ("Write --findings as [] when the result is yes, or as exactly one "
           "{\"kind\": \"skipped\", \"file\": <committed path>, \"line\": <n >= 1>, \"issue\": <one sentence>} when no.")
    if not isinstance(findings, list):
        raise Refused("findings_shape", "findings must be a JSON array", fix)
    expected = 1 if choice == "no" else 0
    if len(findings) != expected:
        raise Refused("findings_shape", f"result {choice!r} needs {expected} finding(s), got {len(findings)}", fix)
    parsed_findings: list[Finding] = []
    for finding in findings:
        if not isinstance(finding, dict):
            raise Refused("findings_shape", f"finding must be {{kind: skipped, file, line >= 1, issue}}: {finding!r}", fix)
        raw_finding = finding
        if (not matches_typeddict(finding, FindingFields) or finding["kind"] != CHECKER_KIND
                or not finding["file"] or finding["line"] < 1 or not finding["issue"]):
            raise Refused("findings_shape", f"finding must be {{kind: skipped, file, line >= 1, issue}}: {finding!r}", fix)
        dependencies = raw_finding.get("depends_on", [])
        def parse_dependency(d: JsonValue) -> FindingDependency | None:
            if (not matches_typeddict(d, FindingDependency) or not 1 <= d["deliverable"] <= total
                    or not d["file"] or d["line"] < 1):
                return None
            return d

        parsed_dependencies = [parse_dependency(d) for d in dependencies] if isinstance(dependencies, list) else []
        if not isinstance(dependencies, list) or any(d is None for d in parsed_dependencies):
            raise Refused("findings_shape", f"depends_on must list {{deliverable: 1..{total}, file, line >= 1}}: "
                          f"{dependencies!r}", fix)
        parsed_findings.append({**finding, "dependencies": [d for d in parsed_dependencies if d is not None],
                                "raw": raw_finding})
    return parsed_findings


def outer_elapsed(envelope: JsonObject, what: str, fix: str) -> JsonObject:
    """The envelope's outer `elapsed_s` as {"elapsed_s": value}, or {} when it carries none."""
    if "elapsed_s" not in envelope:
        return {}
    if not valid_elapsed_s(envelope["elapsed_s"]):
        raise Refused(f"{what}_elapsed_s", f"elapsed_s {envelope['elapsed_s']!r} is not a finite nonnegative number", fix)
    return {"elapsed_s": envelope["elapsed_s"]}


class WrapArguments(argparse.Namespace):
    assignment: str
    client_output: str | None
    reply: str | None
    findings: str | None
    final: str | None
    for_triage: bool


class WrappedReply(TypedDict):
    jev: JsonValue
    findings: JsonValue
    elapsed: JsonObject


class ClientOutput(TypedDict):
    success: bool
    data: JsonValue
    error: JsonValue
    elapsed: JsonObject


class ClientEnvelope(TypedDict):
    success: bool
    data: JsonValue
    error: JsonValue


def parse_client_output(output: JsonValue) -> ClientOutput:
    raw = output
    if not isinstance(raw, dict) or not matches_typeddict(output, ClientEnvelope) \
            or set(raw) - {"elapsed_s"} != set(ClientEnvelope.__required_keys__):
        raise Refused("client_envelope", "--client-output is not a jev_client.py envelope", RERUN)
    return {"success": output["success"], "data": output["data"], "error": output["error"],
            "elapsed": outer_elapsed(raw, "client", RERUN)}


def parse_wrapped_reply(reply: JsonValue) -> WrappedReply:
    data = reply.get("data") if isinstance(reply, dict) and reply.get("success") is True else None
    if not isinstance(data, dict) or "jev" not in data:
        raise Refused("reply_shape", "--reply is not a success reply holding data.jev",
                      "Pass the reply this script printed, unchanged, as --reply.")
    assert isinstance(reply, dict)
    return {"jev": data["jev"], "findings": data.get("findings"),
            "elapsed": outer_elapsed(reply, "reply", "Pass the reply this script printed, unchanged, as --reply.")}


def wrap(args: WrapArguments, client: JevClient) -> JsonObject:
    fields, total = assignment_fields(read_json(args.assignment, "assignment",
                                                "Pass the assignment you received, unchanged, as --assignment."))
    findings = read_json(args.findings, "findings", "Write --findings as a JSON array.") if args.findings else None
    final = read_json(args.final, "final", "Write --final as a JSON object {choice, reason, parts}.") \
        if args.final else None
    if args.reply:
        reply = parse_wrapped_reply(read_json(args.reply, "reply", "Pass the reply this script printed, unchanged, as --reply."))
        jev, elapsed = reply["jev"], reply["elapsed"]
        if findings is None:
            findings = reply["findings"]
    else:
        assert args.client_output is not None
        output = parse_client_output(read_json(args.client_output, "client_output", RERUN))
        elapsed = output["elapsed"]
        if output["success"] is False:
            error = output["error"]
            if not isinstance(error, dict) or not client.is_client_message(error.get("message")):
                raise Refused("client_message", f"the failure message is not the client's own text: {error!r}", RERUN)
            if args.findings or args.final or args.for_triage:
                raise Refused("failure_inputs", "a client failure takes no --findings, --final or --for-triage",
                              "Drop them; the failure is reported as the client wrote it.")
            return {"success": False, "data": None, "error": {**error, "deliverable": fields["deliverable"]}, **elapsed}
        data = output["data"]
        if not isinstance(data, dict) or ("receipt" in data) == ("parts" in data):
            raise Refused("client_data", "the client's data holds neither one `receipt` nor a `parts` list", RERUN)
        jev = data["receipt"] if "receipt" in data else data["parts"]
    key = os.environ.get("TYPESAFE_API_KEY", "").strip()
    if not key:
        raise Refused("receipt_key", "TYPESAFE_API_KEY is not set, so the Jev receipt cannot be verified",
                      "Run with the same TYPESAFE_API_KEY the jev_client.py call used.")
    parsed_receipts = client_receipts(jev, client, key)
    if args.for_triage and len({receipt["choice"] for receipt in parsed_receipts["receipts"]}) == 1:
        raise Refused("for_triage", "every Jev part answers the same, so there is nothing to triage",
                      "Drop --for-triage; the common answer sets the findings.")
    choice = result_choice(parsed_receipts, final, args.for_triage)
    findings = [] if findings is None else findings
    parsed_findings = check_findings(findings, choice, total)
    out: JsonObject = {"sanity_bead": fields["sanity_bead"], "dev_bead": fields["dev_bead"],
                       "deliverable": fields["deliverable"], "commit_checked": fields["commit_checked"],
                       "findings": [finding["raw"] for finding in parsed_findings], "jev": jev}
    if final is not None:
        out["jev_final"] = final
    return {"success": True, "data": out, "error": None, **elapsed}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--assignment", required=True)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--client-output", help="the raw stdout of one jev_client.py --assignment call")
    source.add_argument("--reply", help="a reply this script printed, to add dev-sanity's --final")
    parser.add_argument("--findings")
    parser.add_argument("--final")
    parser.add_argument("--for-triage", action="store_true", help="split parts disagree: emit the reply for dev-sanity")
    args = parser.parse_args(argv, namespace=WrapArguments())
    if args.reply and not args.final:
        parser.error("--reply requires --final")
    if args.for_triage and (args.final or args.reply):
        parser.error("--for-triage takes neither --final nor --reply")
    try:
        result = wrap(args, load_client())
    except Refused as exc:
        print(exc, file=sys.stderr)
        return 1
    print(json.dumps(result, allow_nan=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
