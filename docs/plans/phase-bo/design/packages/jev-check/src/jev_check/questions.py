"""Plan-anchor question-set loader (schema plan-anchoring/questions/1): choice templates expanded per subject."""
import json
from dataclasses import dataclass
from pathlib import Path

from jev_check.client import JevCheckError, Question, as_object

SCHEMA = "plan-anchoring/questions/1"


@dataclass(frozen=True)
class QuestionSet:
    name: str
    templates: dict[str, Question]

    def expand(self, template: str, subjects: dict[str, str]) -> dict[str, Question]:
        """One question per subject, keyed '<template>:<subject id>'. The text replaces {subject} in the template."""
        if template not in self.templates:
            raise JevCheckError("INPUT_INVALID", f"question set {self.name} has no template {template}")
        q = self.templates[template]
        return {f"{template}:{sid}": {"type": q["type"], "instructions": q["instructions"].replace("{subject}", text),
                                      "criteria": dict(q["criteria"])} for sid, text in subjects.items()}


def load_question_set(path: Path) -> QuestionSet:
    try:
        raw = as_object(json.loads(path.read_text()))
    except (OSError, ValueError) as exc:
        raise JevCheckError("INPUT_INVALID", f"cannot read question set {path}: {exc}") from None
    questions = as_object(raw.get("questions")) if raw is not None and raw.get("schema") == SCHEMA else None
    if questions is None:
        raise JevCheckError("INPUT_INVALID", f"{path} is not a {SCHEMA} question set")
    templates: dict[str, Question] = {}
    for key, item in questions.items():
        q = as_object(item)
        criteria = as_object(q.get("criteria")) if q is not None else None
        instructions = q.get("instructions") if q is not None else None
        if (q is None or q.get("type") != "choice" or not isinstance(instructions, str) or criteria is None
                or len(criteria) < 2):
            raise JevCheckError("INPUT_INVALID", f"{path}: question {key} is not a choice question")
        templates[key] = {"type": "choice", "instructions": instructions,
                          "criteria": {k: str(v) for k, v in criteria.items()}}
    return QuestionSet(name=path.stem, templates=templates)
