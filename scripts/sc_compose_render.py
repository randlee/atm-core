"""Root-confined Python-wheel rendering for repository sc-compose templates."""

from __future__ import annotations

from pathlib import Path
from typing import Any

import sc_compose


def render_file(template: Path, variables: dict[str, Any], *, root: Path, strict: bool = True) -> str:
    """Render one template through the wheel while confining it to ``root``."""

    resolved_root = root.resolve()
    template_path = template.resolve() if template.is_absolute() else (resolved_root / template).resolve()
    try:
        relative_template = template_path.relative_to(resolved_root)
    except ValueError as error:
        raise ValueError(f"template {template} is outside confining root {resolved_root}") from error
    request = sc_compose.ComposeRequest(
        root=resolved_root,
        mode=sc_compose.ComposeMode.file(str(relative_template)),
        vars_input=variables,
        policy=sc_compose.ComposePolicy(strict_undeclared_variables=strict),
    )
    return sc_compose.compose_file(request).rendered_text
