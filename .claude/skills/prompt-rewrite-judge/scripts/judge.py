#!/usr/bin/env python3
# pyright: strict
"""Judge prompt rewrites with Jev: per-requirement keep/drop plus clarity.

judge.py --key key.json --original original.txt --out DIR [--client .claude/skills/atm-bd-orchestration/scripts/jev_client.py] CAND.txt...
Writes DIR/<cand>.req.json and DIR/<cand>.json (Jev answers) and prints one row per
candidate; each value is the choice and Jev's probability for it. A failed row carries the
client's exit code, JSON error, unparsed stdout and stderr verbatim.
"""
import argparse
import json
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from types import ModuleType
from typing import Literal, Mapping, NotRequired, TypedDict
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "atm-beads" / "scripts"))
from operation_models import matches_typeddict

MODEL = "jev-1.13.0"
MAX_REQUEST_BYTES = 96000  # jev_client.py refuses larger requests
KEEP = ("Requirement: {req}\nCompare state.candidate with state.original. Does state.candidate still "
        "explicitly require this, at least as clearly as state.original? Answer no if the candidate "
        "omits it, weakens it, or makes it optional.")
ExtraQuestion = Literal["changes_rule", "unnecessary", "ambiguous", "clearer_than_original"]
EXTRA: dict[ExtraQuestion, tuple[str, list[str]]] = {
    "changes_rule": ("Does state.candidate add a rule, or change the order or meaning of an action, compared with state.original?", ["no", "yes"]),
    "unnecessary": ("Does state.candidate contain words an agent does not need to carry out state.requirements?", ["none", "some", "mostly"]),
    "ambiguous": ("Could an agent misread what state.candidate requires or in what order?", ["no", "yes"]),
    "clearer_than_original": ("Is state.candidate clearer for an agent than state.original?", ["yes", "same", "no"]),
}
HEADER = "cand | words | dropped (keep=no) | changes_rule | unnecessary | ambiguous | clearer"


class Question(TypedDict):
    type: Literal["choice"]
    instructions: str
    criteria: dict[str, None]


class JevRequest(TypedDict):
    model: str
    state: dict[str, list[str] | str]
    questions: dict[str, Question]


class Answer(TypedDict):
    choice: str
    probabilities: dict[str, float]


JevProbability = int | float


class JevAnswer(TypedDict):
    choice: str
    probabilities: dict[str, JevProbability]


class JevResponseData(TypedDict):
    answers: NotRequired[dict[str, JevAnswer]]


class JevErrorRecord(TypedDict):
    code: NotRequired[str]
    message: NotRequired[str]
    recoverable: NotRequired[bool]
    suggested_action: NotRequired[str]


class JevResponse(TypedDict):
    success: bool
    data: NotRequired[JevResponseData | None]
    error: NotRequired[JevErrorRecord | None]
    model: NotRequired[str]


class JevKey(TypedDict):
    requirements: list[str]


class JevAnswers(TypedDict):
    answers: dict[str, JevAnswer]



class ErrorRecord(TypedDict):
    exit: int
    error: object
    stdout: str | None
    stderr: str


class Verdict(TypedDict):
    dropped: list[tuple[int, float]]
    changes_rule: tuple[str, float]
    unnecessary: tuple[str, float]
    ambiguous: tuple[str, float]
    clearer_than_original: tuple[str, float]


Answers = Mapping[str, JevAnswer]


_typed_dict_module = sys.modules.get(__name__)
if _typed_dict_module is None:
    _typed_dict_module = ModuleType(__name__)
    sys.modules[__name__] = _typed_dict_module
_typed_dict_module.__dict__.update(globals())


def parse_answers(value: JevResponseData) -> Answers | None:
    if "answers" not in value:
        return None
    answers: dict[str, JevAnswer] = {}
    for name, raw_answer in value["answers"].items():
        probabilities = {option: float(probability) for option, probability in raw_answer["probabilities"].items()}
        if raw_answer["choice"] not in probabilities:
            return None
        answers[name] = {"choice": raw_answer["choice"], "probabilities": probabilities}
    return answers


def request(key: list[str], original: str, cand: str) -> JevRequest:
    q: dict[str, Question] = {f"keep_{i}": {"type": "choice", "instructions": KEEP.format(req=r), "criteria": {"yes": None, "no": None}}
         for i, r in enumerate(key, 1)}
    for k, (text, opts) in EXTRA.items():
        q[k] = {"type": "choice", "instructions": text, "criteria": {o: None for o in opts}}
    return {"model": MODEL, "state": {"requirements": key, "original": original, "candidate": cand}, "questions": q}


def reduce(answer_data: object) -> Verdict:
    """Jev answers -> {'dropped': [(n, p_no)], question: (choice, p_choice)}."""
    document = {"answers": answer_data}
    if not matches_typeddict(document, JevAnswers):
        raise ValueError("invalid Jev answers")
    answers = document["answers"]

    def parsed_answer(name: str) -> Answer:
        answer = answers[name]
        choice = answer["choice"]
        probabilities = {option: float(probability) for option, probability in answer["probabilities"].items()}
        if choice not in probabilities:
            raise ValueError(f"missing Jev probability for selected choice {name}/{choice}")
        return {"choice": choice, "probabilities": probabilities}

    def pick(answer_name: str) -> tuple[str, float]:
        answer = parsed_answer(answer_name)
        choice = answer["choice"]
        return choice, answer["probabilities"][choice]

    keeps = sorted(((int(k.split("_")[1]), parsed_answer(k)) for k in answers if k.startswith("keep_")),
                   key=lambda item: item[0])
    return {
        "dropped": [(number, answer["probabilities"]["no"]) for number, answer in keeps if answer["choice"] == "no"],
        "changes_rule": pick("changes_rule"),
        "unnecessary": pick("unnecessary"),
        "ambiguous": pick("ambiguous"),
        "clearer_than_original": pick("clearer_than_original"),
    }


def row(name: str, words: int, verdict: Verdict | None, error: object = None) -> str:
    if verdict is None:
        return f"{name} | {words} | JEV FAILED {json.dumps(error)}"
    def cell(key: Literal["changes_rule", "unnecessary", "ambiguous", "clearer_than_original"]) -> str:
        value = verdict[key]
        return f"{value[0]}({value[1]:.2f})"
    drops = " ".join(f"R{n}({p:.2f})" for n, p in verdict["dropped"]) or "-"
    return " | ".join([name, str(words), drops] + [cell(k) for k in EXTRA])


def run(client: Path, path: Path) -> tuple[Answers | None, ErrorRecord | None]:
    """-> (answers, None) or (None, the last attempt's verbatim error)."""
    error: ErrorRecord | None = None
    for _ in range(2):  # one rerun on failure; never answer for Jev
        p = subprocess.run([sys.executable, str(client), "--request", str(path)], capture_output=True, text=True)
        try:
            decoded: object = json.loads(p.stdout)
        except json.JSONDecodeError:
            response = None
        else:
            response = decoded if matches_typeddict(decoded, JevResponse) else None
        if response is not None:
            data = response.get("data")
            if response["success"] and data is not None:
                parsed = parse_answers(data)
                if parsed is not None:
                    return parsed, None
                error = {"exit": p.returncode, "error": "success response has invalid answers",
                         "stdout": p.stdout, "stderr": p.stderr}
            else:
                response_error = response.get("error")
                error = {"exit": p.returncode, "error": response_error, "stdout": None, "stderr": p.stderr}
        else:
            error = {"exit": p.returncode, "error": None if p.returncode else "response must be a JSON object",
                     "stdout": p.stdout, "stderr": p.stderr}
    return None, error


class Arguments(argparse.Namespace):
    key: Path
    original: Path
    out: Path
    client: Path
    cands: list[Path]


def main(argv: list[str] | None = None) -> int:
    a = argparse.ArgumentParser(description=__doc__.splitlines()[0] if __doc__ else "")
    a.add_argument("--key", required=True, type=Path)
    a.add_argument("--original", required=True, type=Path)
    a.add_argument("--out", required=True, type=Path)
    a.add_argument("--client", type=Path, default=Path(__file__).resolve().parents[2] / "atm-bd-orchestration/scripts/jev_client.py")
    a.add_argument("cands", nargs="+", type=Path)
    o = a.parse_args(argv, namespace=Arguments())
    raw_key: object = json.loads(o.key.read_text())
    orig = o.original.read_text()
    key_document = {"requirements": raw_key}
    if not matches_typeddict(key_document, JevKey):
        sys.exit("key.json must be a nonempty JSON list of requirement strings")
    key = key_document["requirements"]
    if not key or any(not requirement for requirement in key):
        sys.exit("key.json must be a nonempty JSON list of requirement strings")
    o.out.mkdir(parents=True, exist_ok=True)
    jobs: list[tuple[str, Path, int]] = []
    for c in o.cands:
        text = c.read_text()
        body = json.dumps(request(key, orig, text))
        if len(body.encode()) > MAX_REQUEST_BYTES:
            sys.exit(f"{c.stem}: request {len(body.encode())} bytes > {MAX_REQUEST_BYTES}")
        rp = o.out / f"{c.stem}.req.json"
        rp.write_text(body)
        jobs.append((c.stem, rp, len(text.split())))
    def run_job(job: tuple[str, Path, int]) -> tuple[Answers | None, ErrorRecord | None]:
        return run(o.client, job[1])

    with ThreadPoolExecutor(8) as ex:
        answers = list(ex.map(run_job, jobs))
    print(HEADER)
    for (name, _, words), (ans, error) in zip(jobs, answers):
        if ans is not None:
            (o.out / f"{name}.json").write_text(json.dumps(ans, indent=1))
        print(row(name, words, None if ans is None else reduce(ans), error))
    return 0 if all(ans for ans, _ in answers) else 2


if __name__ == "__main__":
    sys.exit(main())
