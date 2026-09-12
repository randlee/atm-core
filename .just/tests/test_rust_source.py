from __future__ import annotations

import sys
from pathlib import Path


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from rust_source import code_mask, extract_fn_body


def test_code_mask_preserves_lifetimes_and_labels() -> None:
    source = "'_>> 'a 'static 'outer:"
    assert code_mask(source) == source


def test_code_mask_masks_char_literals() -> None:
    for literal in ["'x'", r"'\n'", r"'\u{1F600}'"]:
        masked = code_mask(literal)
        assert masked == " " * len(literal)


def test_extract_fn_body_preserves_the_async_lifetime_signature() -> None:
    source = (
        "type Handler = Pin<Box<dyn Future<Output = Result<(), Error>> + Send + '_>>;\n"
        'fn list_messages() { let value = "{"; call(value); }\n'
    )
    assert "+ Send + '_>>" in code_mask(source)
    assert extract_fn_body(source, "list_messages") == '{ let value = "{"; call(value); }'
