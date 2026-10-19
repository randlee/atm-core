#!/usr/bin/env python3
"""Freeze the round-1 candidate pool for the testability rubric tournament."""
import hashlib, json, sys

req = json.load(open(sys.argv[1]))
out = sys.argv[2]

def q(instructions, yes, no, unclear):
    return {"type": "choice", "instructions": instructions,
            "criteria": {"yes": yes, "no": no, "unclear": unclear}}

base = req["current_questions"]

FAITHFUL_NORM = ("Allowed normalization: informal wording may become must or never, print may become emit to stdout or stderr, "
                 "synonyms may replace the user's words, one directive of a compound quote may become one anchor, and a concrete "
                 "value, identifier, path, or output channel may be named for something the quote already describes.")

cands = {}

# C0: the current rubric, unchanged (baseline).
cands["C0-baseline"] = json.loads(json.dumps(base))

# C1: concise rewording of every role.
cands["C1-concise"] = {
    "single_constraint": q(
        "Count the independently enforceable duties in anchor.statement. One duty applied to one subject under one trigger counts once even when it lists several outputs, surfaces, or values. Does the statement contain exactly one duty?",
        "Exactly one duty; every detail would be checked by the same test.",
        "Two or more duties that could each be satisfied or violated separately are joined.",
        "It cannot be told whether the parts are separate duties."),
    "observable": q(
        "Could a reviewer inspect a concrete artifact, output, location, or behavior and decide whether anchor.statement holds? A subjective quality (for example robust), an undefined label or code, or an approval with no named record gives nothing to inspect.",
        "A concrete artifact, output, path, value, or behavior settles compliance.",
        "Compliance rests on a subjective quality, an undefined label, or an approval with no named record.",
        "Something concrete is named, but which observation settles compliance is ambiguous."),
    "faithful": q(
        "Compare anchor.statement with anchor.quote. Does the statement keep the quote's meaning without inventing a requirement? " + FAITHFUL_NORM + " Vagueness is judged elsewhere; do not penalize it here.",
        "The statement is the quote's requirement after allowed normalization only.",
        "The statement adds a policy, algorithm, strength, threshold, trigger, or outcome the quote does not contain.",
        "It cannot be told whether the change goes beyond allowed normalization."),
    "scoped": q(
        "Do anchor.statement and anchor.scope together name the activity, component, artifact, or explicit all-items set the requirement governs? Judge only whether the area is identifiable, not whether the requirement is good.",
        "A reviewer can name the work area or explicit global set where the anchor applies.",
        "No work area or applicability set can be identified.",
        "An area is named but which work it covers cannot be determined."),
    "self_contained": q(
        "Can a reviewer apply anchor.statement with no private context? Disqualifiers: an undefined subjective quality (for example robust), a private label or code with no definition, or a human approval with no identified record. Ordinary technical terms, paths, flags, environment variable names, prohibitions, and explicit conditions are sufficient. Ignore grammar, bundling, fidelity, and scope.",
        "No disqualifier is present; the statement can be applied as written.",
        "At least one disqualifier decides compliance.",
        "A possible disqualifier is present but it is not clear whether it decides compliance."),
}

# C2: baseline instructions with example lists in the criteria.
c2 = json.loads(json.dumps(base))
c2["single_constraint"]["criteria"]["yes"] = ("All details enforce the same invariant and would be evaluated together, for example one prohibition covering several output streams, "
                                              "or one trigger with one required result.")
c2["single_constraint"]["criteria"]["no"] = ("At least two independently enforceable duties are joined in the statement, for example two components each given their own obligation, "
                                             "or one component given two unrelated obligations.")
c2["observable"]["criteria"]["no"] = ("It supplies only a subjective quality (for example robust or clean), undefined shorthand (for example an unexplained code or label), "
                                      "or an approval with no observable record.")
c2["faithful"]["instructions"] = ("Is anchor.statement a faithful formal requirement derived from anchor.quote? Preserve meaning, not wording. " + FAITHFUL_NORM +
                                  " Do not penalize vagueness here.")
c2["faithful"]["criteria"]["no"] = ("Even after allowed normalization, the statement requires a new policy, implementation choice, strength, trigger, or outcome, "
                                    "for example a specific algorithm or limit the quote never named.")
c2["self_contained"]["criteria"]["no"] = ("At least one decisive term is an undefined quality (for example robust) or private label (for example an unexplained code), "
                                          "or compliance depends on an approval with no identified record.")
cands["C2-examples"] = c2

# C3: decision-procedure framing.
cands["C3-procedure"] = {
    "single_constraint": q(
        "Step 1: list the duties anchor.statement imposes. Step 2: merge duties that share a subject and trigger and would always be checked together, even when they name several outputs or values. Step 3: answer whether one duty remains.",
        "One duty remains after merging.",
        "Two or more separately enforceable duties remain.",
        "The merge cannot be decided from the statement."),
    "observable": q(
        "Step 1: identify what a reviewer would look at to check anchor.statement (a file, path, output stream, value, command result, or behavior). Step 2: if the only thing to check is a subjective quality, an undefined label, or an approval with no named record, there is nothing to look at. Answer whether a determinate observation exists.",
        "A determinate observation exists.",
        "Only a subjective quality, an undefined label, or an unrecorded approval is offered.",
        "An observation is named but it would not settle compliance."),
    "faithful": q(
        "Step 1: read the requirement in anchor.quote. Step 2: read anchor.statement. Step 3: discard differences that are allowed normalization. " + FAITHFUL_NORM + " Step 4: answer whether any remaining difference adds a policy, algorithm, strength, trigger, or outcome. Vagueness is not a difference.",
        "No invented requirement remains after Step 3.",
        "An invented policy, algorithm, strength, trigger, or outcome remains.",
        "It cannot be decided whether a remaining difference is normalization."),
    "scoped": q(
        "Step 1: read anchor.scope and anchor.statement. Step 2: name the activity, component, artifact, or explicit global set they govern. Answer whether Step 2 succeeds, regardless of other defects in the requirement.",
        "The governed area or explicit global set can be named.",
        "Nothing governed can be named.",
        "An area is mentioned but its applicability cannot be determined."),
    "self_contained": q(
        "Step 1: find every term in anchor.statement that decides compliance. Step 2: flag a term only if it is an undefined subjective quality (for example robust), private shorthand with no definition, or a human approval with no identified record. Ordinary technical terms, paths, flags, environment variable names, prohibitions, and explicit conditions are not flagged. Ignore grammar, bundling, fidelity, and scope. Answer whether no term is flagged.",
        "No decisive term is flagged.",
        "At least one decisive term is flagged.",
        "A term might be flagged but it is unclear whether it decides compliance."),
}

# C4: baseline with only the faithful normalization list widened (minimal diff).
c4 = json.loads(json.dumps(base))
c4["faithful"]["instructions"] = ("Is anchor.statement a faithful formal requirement derived from anchor.quote? Preserve meaning, not wording: " + FAITHFUL_NORM[len("Allowed normalization: "):] +
                                  " Do not penalize vagueness here.")
cands["C4-baseline-faithful"] = c4

# C5: shortest wording.
cands["C5-minimal"] = {
    "single_constraint": q("Does anchor.statement impose exactly one independently enforceable duty? Several outputs of one duty count once.",
                           "Exactly one duty.", "Two or more separable duties.", "Cannot tell."),
    "observable": q("Can compliance with anchor.statement be checked by inspecting a concrete artifact, output, or behavior? Subjective qualities, undefined labels, and unrecorded approvals cannot be inspected.",
                    "Yes, a concrete check exists.", "No concrete check exists.", "A check is suggested but would not settle compliance."),
    "faithful": q("Does anchor.statement keep anchor.quote's meaning without adding a requirement? Changes of wording, strength words like must, output channel names, concrete values for described things, and choosing one directive of several are allowed. Ignore vagueness.",
                  "Meaning kept; nothing invented.", "A policy, algorithm, strength, trigger, or outcome was added.", "Cannot tell."),
    "scoped": q("Do anchor.statement and anchor.scope identify where the requirement applies (an activity, component, artifact, or explicit global set)? Ignore other defects.",
                "The area is identifiable.", "No area is identifiable.", "An area is named but its reach is unclear."),
    "self_contained": q("Is anchor.statement applicable without private context? Disqualified only by an undefined subjective quality, undefined private shorthand, or an approval with no identified record. Ignore grammar, bundling, fidelity, and scope.",
                        "No disqualifier.", "A disqualifier decides compliance.", "A possible disqualifier of uncertain weight."),
}

# C6: negative-first framing (look for the disqualifier first).
cands["C6-negative-first"] = {
    "single_constraint": q(
        "Look for a second duty in anchor.statement: an obligation on a different subject, or a second obligation on the same subject that could be met while the first is broken. Listing several outputs, surfaces, or values for one obligation is not a second duty. Is the statement free of a second duty?",
        "No second duty exists.",
        "A second, separately enforceable duty exists.",
        "Whether a part is a second duty cannot be decided."),
    "observable": q(
        "Look for what would make anchor.statement uncheckable: the outcome is only a subjective quality (for example robust), an undefined label or code, or an approval with no identified record. If none of these is the whole outcome, is a concrete artifact or behavior named that a reviewer can inspect?",
        "None of those is the whole outcome and a concrete artifact or behavior is named.",
        "The outcome is only a subjective quality, an undefined label, or an unrecorded approval.",
        "A concrete thing is named but whether it settles compliance is ambiguous."),
    "faithful": q(
        "Look for anything anchor.statement requires that anchor.quote does not: a new policy, algorithm, strength, threshold, trigger, or outcome. " + FAITHFUL_NORM + " Vagueness is not an addition. Is the statement free of additions?",
        "No addition beyond allowed normalization.",
        "At least one addition beyond allowed normalization.",
        "Whether a difference is an addition cannot be decided."),
    "scoped": q(
        "Look for whether anchor.statement and anchor.scope leave the governed area unknown. An explicit all-items set counts as an area. Other defects in the requirement do not matter here. Is the governed activity, component, artifact, or global set identifiable?",
        "The governed area is identifiable.",
        "The governed area is unknown.",
        "An area is named but its applicability cannot be determined."),
    "self_contained": q(
        "Look for a decisive term in anchor.statement that needs outside context: an undefined subjective quality (for example robust), private shorthand with no definition, or a human approval with no identified record. Ordinary technical terms, paths, flags, environment variable names, prohibitions, and explicit conditions need no outside context. Ignore grammar, bundling, fidelity, and scope. Is the statement free of such terms?",
        "No such term is present.",
        "At least one such term decides compliance.",
        "A possible such term is present but its weight is unclear."),
}

def canon(v):
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()

for cid, qs in cands.items():
    assert set(qs) == {"single_constraint", "observable", "faithful", "scoped", "self_contained"}, cid
pool = {"schema": "anchor-tuning/testability-candidate-pool/1", "round": 1,
        "candidates": {cid: {"questions": qs, "digest": hashlib.sha256(canon({"questions": qs})).hexdigest(),
                             "complexity_chars": len(canon(qs))} for cid, qs in cands.items()}}
pool["pool_sha256"] = hashlib.sha256(canon(pool["candidates"])).hexdigest()
open(out, "w").write(json.dumps(pool, indent=2, sort_keys=True) + "\n")
print(pool["pool_sha256"], {k: v["complexity_chars"] for k, v in pool["candidates"].items()})
