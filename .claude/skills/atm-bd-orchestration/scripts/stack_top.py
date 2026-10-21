"""Select a phase's current stack top from its canonical bead ledger."""


def stack_bead_id(root: str) -> str:
    """Return the stack ledger bead ID for a phase root."""
    return f"{root}.stack"


def select_stack_top(bead, pr_target, resolve_head):
    metadata = bead.get("metadata")
    if metadata is not None and not isinstance(metadata, dict):
        raise ValueError("stack bead metadata must be an object or null")
    layers = (metadata or {}).get("layers", [])
    if not isinstance(layers, list):
        raise ValueError("stack bead metadata.layers must be an array")
    if layers:
        layer = layers[-1]
        if not isinstance(layer, dict) or any(
            not isinstance(layer.get(name), str) or not layer[name].strip()
            for name in ("branch", "head")
        ):
            raise ValueError("last stack layer must have non-empty branch and head strings")
        return layer["branch"], layer["head"]
    if not isinstance(pr_target, str) or not pr_target.strip():
        raise ValueError("pr_target must be a non-empty string")
    head = resolve_head(pr_target)
    if not isinstance(head, str) or not head.strip():
        raise ValueError(f"origin/{pr_target} has no resolvable head")
    return pr_target, head
