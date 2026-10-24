#!/usr/bin/env python3
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

CLIENT = Path(__file__).resolve().parent / "jev_client.py"
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "atm-beads" / "scripts"))
from sanity_shared import valid_elapsed_s  # noqa: E402

FENCED = re.compile(r"\s*```(?:json)?[ \t]*\n(.*?)\n```\s*", re.S)
CHECKER_KIND = "skipped"
RERUN = ("Rerun `python3 .claude/skills/atm-bd-orchestration/scripts/jev_client.py --assignment <file>`, save its stdout "
         "unchanged to a file and pass that file as --client-output; never write or edit client output.")


class Refused(Exception):
    def __init__(self, check, message, fix):
        super().__init__(f"{check}: {message}. Fix: {fix}")


def load_client():
    spec = importlib.util.spec_from_file_location("jev_client", CLIENT)
    if spec is None or spec.loader is None:
        raise Refused("client_load", f"cannot load Jev client {CLIENT}", RERUN)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def read_json(path, what, fix):
    try:
        text = Path(path).read_text()
        fenced = FENCED.fullmatch(text)
        return json.loads(fenced.group(1) if fenced else text)
    except (OSError, ValueError) as exc:
        raise Refused(f"{what}_unreadable", f"cannot read {what} {path}: {exc}", fix)


def assignment_fields(assignment):
    fix = "Pass the assignment you received, unchanged, as --assignment."
    try:
        fields = {"sanity_bead": assignment["sanity_bead"], "dev_bead": assignment["dev_bead"]["id"],
                  "deliverable": assignment["deliverable"]["number"], "commit_checked": assignment["commit"]}
        total = assignment["deliverables_total"]
    except (KeyError, TypeError) as exc:
        raise Refused("assignment_fields", f"assignment lacks {exc}", fix)
    number = fields["deliverable"]
    if not isinstance(number, int) or isinstance(number, bool) or not isinstance(total, int) or not 1 <= number <= total:
        raise Refused("assignment_fields", f"deliverable.number {number!r} is not in 1..{total!r}", fix)
    return fields, total


def client_receipts(jev, client, key):
    """The receipts carried by `jev` (one receipt, or a split request's part list), each verified."""
    if isinstance(jev, list):
        receipts = []
        for i, part in enumerate(jev, 1):
            label = f"{i} of {len(jev)}"
            if not isinstance(part, dict) or part.get("part") != label:
                raise Refused("part_order", f"Jev part {label} is missing or mislabelled", RERUN)
            receipt = part.get("receipt")
            if not isinstance(receipt, dict) or not client.verify_receipt(receipt, key):
                raise Refused("receipt_mac", f"no valid Jev receipt on part {label}", RERUN)
            response = part.get("response")
            answer = response.get("answers", {}).get("written") if isinstance(response, dict) else None
            if not isinstance(answer, dict) or (answer.get("choice"), answer.get("probabilities")) != (
                    receipt["choice"], receipt["probabilities"]):
                raise Refused("part_response", f"Jev part {label}'s response does not match its receipt", RERUN)
            receipts.append(receipt)
        if not receipts:
            raise Refused("part_order", "the client's part list is empty", RERUN)
    else:
        if not client.verify_receipt(jev, key):
            raise Refused("receipt_mac", "no valid Jev receipt: it was not written by a jev_client.py call", RERUN)
        receipts = [jev]
    for receipt in receipts:
        if receipt["question"] != "written" or receipt["choice"] not in ("yes", "no"):
            raise Refused("receipt_question", f"Jev receipt answers {receipt['question']!r} with "
                          f"{receipt['choice']!r}, not written: yes or no", RERUN)
    return receipts


def result_choice(jev, receipts, final, for_triage=False):
    """The result: the receipts' common choice, or dev-sanity's triaged `final` when they disagree; `no` (its one
    skipped finding) for a reply sent to triage."""
    choices = {receipt["choice"] for receipt in receipts}
    if final is not None and (not isinstance(final, dict) or final.get("choice") not in choices):
        raise Refused("choice_in_receipts", f"final selection {final!r} names no choice any receipt holds "
                      f"({sorted(choices)})", "Write --final as {choice, reason, parts} with a choice a part answered.")
    if len(choices) == 1:
        return next(iter(choices))
    labels = [part["part"] for part in jev]
    if final is None and for_triage:
        return "no"
    if final is None:
        raise Refused("final_selection", f"Jev parts disagree ({sorted(choices)}) and no final selection was given",
                      "sc-sanity-jev: rerun with --for-triage and the one skipped finding. dev-sanity: triage the "
                      "parts, then rerun with --reply <reply> --final <file> holding {choice, reason, parts}.")
    if not isinstance(final.get("reason"), str) or not final["reason"].strip():
        raise Refused("final_reason", "the final selection has no reason",
                      "Add a nonempty `reason` citing every part's result to the --final file.")
    if final.get("parts") != labels:
        raise Refused("final_parts", f"final selection parts {final.get('parts')!r} are not {labels!r}",
                      f"Set `parts` in the --final file to {json.dumps(labels)}.")
    return final["choice"]


def check_findings(findings, choice, total):
    fix = ("Write --findings as [] when the result is yes, or as exactly one "
           "{\"kind\": \"skipped\", \"file\": <committed path>, \"line\": <n >= 1>, \"issue\": <one sentence>} when no.")
    if not isinstance(findings, list):
        raise Refused("findings_shape", "findings must be a JSON array", fix)
    expected = 1 if choice == "no" else 0
    if len(findings) != expected:
        raise Refused("findings_shape", f"result {choice!r} needs {expected} finding(s), got {len(findings)}", fix)
    for finding in findings:
        if (not isinstance(finding, dict) or finding.get("kind") != CHECKER_KIND
                or not isinstance(finding.get("file"), str) or not finding["file"]
                or not isinstance(finding.get("line"), int) or isinstance(finding.get("line"), bool)
                or finding["line"] < 1 or not isinstance(finding.get("issue"), str) or not finding["issue"]):
            raise Refused("findings_shape", f"finding must be {{kind: skipped, file, line >= 1, issue}}: {finding!r}", fix)
        dependencies = finding.get("depends_on", [])
        if not isinstance(dependencies, list) or any(
                not isinstance(d, dict) or not isinstance(d.get("deliverable"), int) or isinstance(d["deliverable"], bool)
                or not 1 <= d["deliverable"] <= total or not isinstance(d.get("file"), str) or not d["file"]
                or not isinstance(d.get("line"), int) or isinstance(d["line"], bool) or d["line"] < 1
                for d in dependencies):
            raise Refused("findings_shape", f"depends_on must list {{deliverable: 1..{total}, file, line >= 1}}: "
                          f"{dependencies!r}", fix)


def outer_elapsed(envelope, what, fix):
    """The envelope's outer `elapsed_s` as {"elapsed_s": value}, or {} when it carries none."""
    if "elapsed_s" not in envelope:
        return {}
    if not valid_elapsed_s(envelope["elapsed_s"]):
        raise Refused(f"{what}_elapsed_s", f"elapsed_s {envelope['elapsed_s']!r} is not a finite nonnegative number", fix)
    return {"elapsed_s": envelope["elapsed_s"]}


def wrap(args, client):
    fields, total = assignment_fields(read_json(args.assignment, "assignment",
                                                "Pass the assignment you received, unchanged, as --assignment."))
    findings = read_json(args.findings, "findings", "Write --findings as a JSON array.") if args.findings else None
    final = read_json(args.final, "final", "Write --final as a JSON object {choice, reason, parts}.") \
        if args.final else None
    if args.reply:
        reply = read_json(args.reply, "reply", "Pass the reply this script printed, unchanged, as --reply.")
        data = reply.get("data") if isinstance(reply, dict) and reply.get("success") is True else None
        if not isinstance(data, dict) or "jev" not in data:
            raise Refused("reply_shape", "--reply is not a success reply holding data.jev",
                          "Pass the reply this script printed, unchanged, as --reply.")
        jev = data["jev"]
        elapsed = outer_elapsed(reply, "reply", "Pass the reply this script printed, unchanged, as --reply.")
        if findings is None:
            findings = data.get("findings")
    else:
        output = read_json(args.client_output, "client_output", RERUN)
        if not isinstance(output, dict) or set(output) - {"elapsed_s"} != {"success", "data", "error"} \
                or not isinstance(output["success"], bool):
            raise Refused("client_envelope", "--client-output is not a jev_client.py envelope", RERUN)
        elapsed = outer_elapsed(output, "client", RERUN)
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
    receipts = client_receipts(jev, client, key)
    if args.for_triage and len({receipt["choice"] for receipt in receipts}) == 1:
        raise Refused("for_triage", "every Jev part answers the same, so there is nothing to triage",
                      "Drop --for-triage; the common answer sets the findings.")
    choice = result_choice(jev, receipts, final, args.for_triage)
    findings = [] if findings is None else findings
    check_findings(findings, choice, total)
    out = {**fields, "findings": findings, "jev": jev}
    if final is not None:
        out["jev_final"] = final
    return {"success": True, "data": out, "error": None, **elapsed}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--assignment", required=True)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--client-output", help="the raw stdout of one jev_client.py --assignment call")
    source.add_argument("--reply", help="a reply this script printed, to add dev-sanity's --final")
    parser.add_argument("--findings")
    parser.add_argument("--final")
    parser.add_argument("--for-triage", action="store_true", help="split parts disagree: emit the reply for dev-sanity")
    args = parser.parse_args(argv)
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
