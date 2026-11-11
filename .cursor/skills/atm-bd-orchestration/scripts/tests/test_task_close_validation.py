"""The extracted validators remain the hook's public objects."""
import ast
import io
from pathlib import Path
import tokenize

import pytest

from task_close_world import hook
import task_close_validation as validation


def token_line_count(source):
    ignored = {tokenize.ENCODING, tokenize.ENDMARKER, tokenize.NEWLINE, tokenize.NL,
               tokenize.INDENT, tokenize.DEDENT, tokenize.COMMENT}
    docstrings = []
    lines = source.splitlines()

    def position(line, byte_column):
        return line, len(lines[line - 1].encode("utf-8")[:byte_column].decode("utf-8"))

    for node in ast.walk(ast.parse(source)):
        if isinstance(node, (ast.Module, ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)) and node.body:
            first = node.body[0]
            if isinstance(first, ast.Expr) and isinstance(first.value, ast.Constant) and isinstance(first.value.value, str):
                assert first.end_lineno is not None and first.end_col_offset is not None
                docstrings.append((position(first.lineno, first.col_offset), position(first.end_lineno, first.end_col_offset)))
    counted = set()
    for token in tokenize.generate_tokens(io.StringIO(source).readline):
        if token.type in ignored:
            continue
        if token.type == tokenize.STRING and any(start <= token.start and token.end <= end for start, end in docstrings):
            continue
        counted.update(line for line in range(token.start[0], token.end[0] + 1)
                       if line <= len(lines) and lines[line - 1].strip())
    return len(counted)


@pytest.mark.parametrize("body_lines, expected", [(998, 1000), (999, 1001)])
def test_rule_003_counts_multiline_literal_at_threshold(body_lines, expected):
    source = 'value = """\n' + 'content\n' * body_lines + '"""\n'
    assert token_line_count(source) == expected
    assert (token_line_count(source) > 1000) is (expected > 1000)


def test_rule_003_preserves_code_on_docstring_line_and_ignores_blanks():
    source = '"module doc"; value = """one\n\nthree"""\n'
    assert token_line_count(source) == 2
    assert token_line_count('def f(): """docs"""; return 1\n') == 1
    assert token_line_count('"""module\ndoc"""\nvalue = 1\n') == 1
    assert token_line_count('def café(): """first\nmiddle\nlast"""; return 1\n') == 2


def test_production_scripts_obey_rule_003():
    scripts = Path(__file__).resolve().parents[1]
    counts = {}
    for path in sorted(scripts.rglob("*")):
        if not path.is_file() or path.suffix not in ("", ".py") or "tests" in path.relative_to(scripts).parts or "__pycache__" in path.parts:
            continue
        source = path.read_text(encoding="utf-8")
        first_line = source.splitlines()[0] if source else ""
        if path.suffix != ".py" and not (first_line.startswith("#!") and ("python" in first_line or "uv run --script" in first_line)):
            continue
        counts[str(path.relative_to(scripts))] = token_line_count(source)
    assert counts
    assert {name: count for name, count in counts.items() if count > 1000} == {}


def test_hook_reexports_the_shared_validators_and_error():
    for name in ("CloseError", "assignment_details", "assignment_id_for_round", "sanity_round",
                 "sanity_round_key", "validate_command_json", "header", "reviewer_list", "validate",
                 "sanity_close_mode", "bd_value", "read_vars", "merge_result", "validate_outcome",
                 "read_toml_setting", "read_config"):
        assert getattr(hook, name) is getattr(validation, name)
    with pytest.raises(hook.CloseError) as error:
        validation.validate_command_json(["atm", "task", "events", "task", "--json"], {"events": "bad"})
    assert error.value.code == "COMMAND.INVALID"
    assert "atm task events task" in error.value.fix
