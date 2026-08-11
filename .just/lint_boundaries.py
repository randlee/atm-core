#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import argparse
import re
import sys

from lint_common import build_report
from lint_common import discover_repo_root
from lint_common import is_comment_line
from lint_common import load_lint_config
from lint_common import monotonic_now
from lint_common import print_report
from lint_common import render_table
from lint_common import rust_file_test_scope
from lint_common import workspace_crate_section_lines
from lint_common import workspace_manifest_paths


LINT_NAME = "boundaries"
YAML_FENCE_START = "```yaml"
YAML_FENCE_END = "```"
REQUIRED_BOUNDARY_FIELDS = (
    ("boundary_id",),
    ("owner_package",),
    ("owner_crate_path",),
    ("name",),
    ("implementation", "visibility"),
    ("implementation", "constructor"),
    ("dependencies", "allowed_dependents"),
    ("dependencies", "allowed_dependencies"),
    ("dependencies", "forbidden_edges"),
    ("ownership", "io_owns"),
    ("ownership", "io_forbidden"),
    ("references", "scope"),
    ("references", "forbidden"),
    ("testing", "allowed_test_double_paths"),
    ("testing", "forbidden_test_bypasses"),
    ("enforcement", "lint_rules"),
    ("enforcement", "review_gates"),
    ("status", "state"),
)
VISIBILITY_VALUES = {"private", "pub(crate)", "public", "trait_only"}
CONSTRUCTOR_VALUES = {"private", "pub(crate)", "public", "none"}
REFERENCE_SCOPE_VALUES = {"global", "inside_owner_crate", "outside_owner_crate"}
STATE_VALUES = {
    "planned",
    "active",
    "deferred",
    "retired",
    "stub_landed",
    "concrete_landed",
    "unix_implemented_windows_pending",
}
PACKAGE_NAME_RE = re.compile(r"^[a-z0-9]+(?:[.-][a-z0-9]+)*$")
CRATE_PATH_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
RUST_PATH_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*$")
IDENTIFIER_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
FORBIDDEN_EDGE_RE = re.compile(
    r"^(?P<left>[a-z0-9]+(?:[.-][a-z0-9]+)*)\s*->\s*(?P<right>[a-z0-9]+(?:[.-][a-z0-9]+)*)$"
)
PUBLIC_TYPE_TEMPLATE = r"^\s*pub(?:\([^)]*\))?\s+(?:struct|enum|type)\s+{name}\b"
PUBLIC_REEXPORT_TEMPLATE = r"^\s*pub(?:\([^)]*\))?\s+use\b.*\b{name}\b"
PUBLIC_FUNCTION_RE = re.compile(r"^\s*pub(?:\([^)]*\))?\s+fn\s+[A-Za-z_][A-Za-z0-9_]*\b")

# ``ownership.io_forbidden`` is a source-level policy, not merely metadata.
# Keep the vocabulary explicit so adding a new tag cannot silently create an
# unenforced boundary declaration.  Patterns are intentionally scoped to
# concrete implementation modules below; they are not searched through the
# entire owner crate (which would conflate sibling boundaries).
IO_FORBIDDEN_SOURCE_PATTERNS: dict[str, tuple[str, ...]] = {
    "ambient_singleton_lookup": (
        r"\bdefault_runtime\s*\(",
        r"\bget_default_runtime\s*\(",
        r"\binstall_default_runtime(?:_factory|_instance)?\s*\(",
    ),
    "backend_specific_storage": (r"\b(?:rusqlite|sqlx|diesel)::", r"\b(?:Sqlite|SQLite)[A-Za-z0-9_]*\b"),
    "business_logic_dispatch": (
        r"\bbusiness_logic_dispatch\b",
        r"\bdispatch_(?:write|non_write)\s*\(",
        r"\broute_write\s*\(",
    ),
    "database_io": (
        r"\b(?:rusqlite|sqlx|diesel)::",
        r"\b(?:Sqlite|SQLite)(?:Connection|Transaction|Store|Database|Pool|Backend)\b",
        r"\bdatabase_io\b",
    ),
    "compatibility_jsonl_append": (
        r"\bappend_compat_inbox_message\s*\(",
        r"\bcompatibility_jsonl_append\b",
        r"\bcompatibility[^\n]*\.jsonl\b",
    ),
    "cursor": (r"\bcursor\b", r"\bCursor\b"),
    "catalog_persistence": (
        r"\b(?:TemplateCatalogStore|TemplateCatalogRecord|CatalogPersistence)\b",
        r"\b(?:persist|store|load)_[A-Za-z0-9_]*catalog[A-Za-z0-9_]*\s*\(",
        r"\bcatalog_persistence\b",
    ),
    "cli_surface": (
        r"\bclap::",
        r"\b(?:Args|Subcommand|Parser)::",
        r"\bcli_surface\b",
    ),
    "daemon_lifecycle": (r"\b(?:start|stop|shutdown|restart)_daemon\s*\(", r"\bdaemon_lifecycle\b"),
    "daemon_private_graft_api": (r"\bdaemon_private_graft_api\b", r"\bDaemonGraft[A-Za-z0-9_]*\b", r"\bdaemon::graft\b"),
    "daemon_request_dispatch": (r"\bdaemon_request_dispatch\b", r"\bdispatch_request\s*\(", r"\bDaemonRequestDispatcher\b"),
    "daemon_runtime_bootstrap": (r"\bdaemon_runtime_bootstrap\b", r"\bbootstrap_daemon_runtime\s*\(", r"\bDaemonRuntime::new\s*\("),
    "daemon_runtime_dispatch": (r"\bdaemon_runtime_dispatch\b", r"\bdispatch_runtime_request\s*\("),
    "daemon_transport": (r"\bdaemon_transport\b", r"\bDaemon(?:Http|Tcp|Ipc)Transport\b", r"\bdaemon::(?:http|tcp|ipc)_transport\b"),
    "delivery_plan_construction": (r"\b(?:Reply)?DeliveryPlan\b", r"\bexecute_(?:reply_)?delivery_plan\s*\(", r"\bdelivery_plan_construction\b"),
    "delivery_queue": (r"\bdelivery_queue\b", r"\bDeliveryQueue\b", r"\bqueue_delivery\s*\("),
    "delivery_state": (r"\bdelivery_state\b", r"\bDeliveryState\b"),
    "dns": (r"\b(?:lookup_host|to_socket_addrs|DnsResolver|resolve_peer_authority)\b",),
    "direct_socket_io": (
        r"\b(?:std|tokio)::net::",
        r"\b(?:Tcp|Udp|Unix)(?:Stream|Listener|Socket)\b",
        r"\bdirect_socket_io\b",
    ),
    "direct_sqlite_io": (r"\b(?:rusqlite|sqlx)::", r"\b(?:Sqlite|SQLite)[A-Za-z0-9_]*\b", r"\bdirect_sqlite_io\b"),
    "direct_rusqlite_calls": (
        r"\brusqlite::",
        r"\bSqlite(?:Connection|Transaction|Statement|Database)\b",
    ),
    "graft_crate_dependency": (r"\batm[_-]graft\b",),
    "graft_session_runtime": (r"\bgraft_session_runtime\b", r"\bGraftSession\b"),
    "inbox_jsonl": (r"\binbox[^\n]*\.jsonl\b", r"\b(?:append|write)_[A-Za-z0-9_]*inbox[A-Za-z0-9_]*\s*\("),
    "mailbox_storage_selection": (r"\bmailbox_storage_selection\b", r"\b(?:select|choose)_[A-Za-z0-9_]*mailbox[A-Za-z0-9_]*\s*\("),
    "message_delivery": (r"\bmessage_delivery\b", r"\b(?:deliver|send)_message\s*\(", r"\bMessageDelivery\b"),
    "production_delivery": (
        r"\bproduction_delivery\b",
        r"\b(?:deliver|send|route)_production(?:_message|_request)?\s*\(",
    ),
    "message_persistence": (r"\bmessage_persistence\b", r"\b(?:persist|store)_message\s*\(", r"\bMessageStore\b"),
    "local_path_heuristics": (
        r"\b(?:std::path::|Path(?:Buf)?::|canonicalize\s*\()",
        r"\b(?:is_absolute|starts_with)\s*\(",
        r"\blocal_path_heuristics\b",
    ),
    "named_pipe": (r"\bNamedPipe\b", r"\bnamed_pipe\b", r"\b(?:pipe|fifo)_(?:read|write|open)\s*\("),
    "nudge": (r"\bnudge\b", r"\bNudge\b"),
    "nudge_emission": (r"\bnudge_emission\b", r"\b(?:emit|send|deliver)_nudge\s*\(", r"\bNudgeEmitter\b"),
    "graft_delivery": (
        r"\b(?:deliver_graft_post_send|deliver_published_receiver_hook|GraftPostSendRequest)\b",
    ),
    "hook_execution": (r"\b(?:emit_post_send_effects|load_post_send_config_for_sender)\b",),
    "http_server": (
        r"\baxum::serve\s*\(",
        r"\bserve_(?:loopback|unix)_http1\s*\(",
        r"\bhyper::server\b",
    ),
    "http_runtime": (
        r"\b(?:axum|hyper|tower)::",
        r"\b(?:HttpRuntime|ApiRouter)\b",
        r"\bhttp_runtime\b",
    ),
    "process_spawn": (r"\bstd::process::Command\b", r"\bCommand::new\s*\(", r"\)\.spawn\s*\("),
    "process_spawn_for_notifications": (r"\bstd::process::Command\b", r"\bCommand::new\s*\(", r"\)\.spawn\s*\("),
    "process_spawn_outside_owned_runtime_path": (r"\bstd::process::Command\b", r"\bCommand::new\s*\(", r"\)\.spawn\s*\("),
    "raw_http_framing": (
        r"\bHttpFrameReader\b",
        r"\b(?:read|write)_http_(?:request|response)\s*\(",
        r"\bwrite_http_request_with_headers\s*\(",
    ),
    "replay_or_resend": (
        r"\b(?:Peer)?(?:Replay|Resend)[A-Za-z0-9_]*\b",
        r"\bPeerDrainCoordinator\b",
    ),
    "receipt": (r"\breceipt\b", r"\bReceipt\b"),
    "recipient_routing": (
        r"\brecipient_routing\b",
        r"\b(?:resolve|route|select)_recipient\s*\(",
        r"\b(?:RecipientRouting|RecipientRouter)\b",
        r"\bresolve_peer_authority\s*\(",
    ),
    "retry_queue": (r"\bretry_queue\b", r"\bRetryQueue\b", r"\bqueue_retry\s*\("),
    "retry_state": (r"\bretry_state\b", r"\bRetryState\b"),
    "background_work": (
        r"\bbackground_work\b",
        r"\bthread::spawn\s*\(",
    ),
    "router": (r"\brouter\b", r"\bRouter\b"),
    "storage_schema": (
        r"\b(?:CREATE|ALTER|DROP)\s+(?:VIRTUAL\s+)?TABLE\b",
        r"\b(?:schema_json|migration(?:s)?)\b",
        r"\bstorage_schema\b",
    ),
    "socket_io": (
        r"\b(?:std|tokio)::net::",
        r"\b(?:Tcp|Udp|Unix)(?:Stream|Listener|Socket)\b",
        r"\bsocket_io\b",
    ),
    "tls": (r"\b(?:TlsConnector|TlsAcceptor|rustls|ServerName)\b",),
    "tls_adapter": (r"\b(?:TlsConnector|TlsAcceptor|rustls|ServerName)\b",),
    "peer_only_ingress": (
        r"\bPeerMessageArray\b",
        r"\bpeer_(?:delivery|http_listener)\b",
        r"\bAuthenticatedConnector::peer\b",
    ),
    "peer_specific_dto_or_route": (
        r"\bPeerMessageArray\b",
        r"\bpeer_(?:delivery|http_listener)\b",
    ),
    "sqlite": (
        r"\b(?:rusqlite|sqlx)::",
        r"\b(?:Sqlite|SQLite)(?:Connection|Transaction|Store|Database|Pool|Backend)\b",
        r"\bsqlite_(?:open|connect|transaction|query|write)\s*\(",
    ),
    "storage_write": (r"\bstorage_write\b", r"\b(?:write|put|insert|update)_storage\s*\("),
    "task_changed_notifications": (r"\btask_changed_notifications\b", r"\bTaskChanged(?:Notification|Event)?\b", r"\btask_changed\b"),
    "template_rendering": (r"\btemplate_rendering\b", r"\b(?:render|render_template)\s*\(", r"\bTemplateRenderer\b"),
    "tls_handshake": (r"\b(?:rustls|native_tls)::", r"\b(?:Client|Server)Connection\b", r"\b(?:tls|TLS)[^\n]*handshake\b"),
    "tmux_nudge_delivery": (r"\btmux_nudge_delivery\b", r"\btmux[^\n]*nudge\b", r"\b(?:send|emit)_tmux_nudge\s*\("),
    "transport_dispatch": (r"\btransport_dispatch\b", r"\bdispatch_transport\s*\(", r"\bTransportDispatcher\b"),
    "transport": (
        r"\b(?:Tcp|Udp|Unix)(?:Stream|Listener|Socket)\b",
        r"\b(?:reqwest|hyper)::",
        r"\btransport\b",
    ),
}

# The runtime composition crate performs a deliberately short-lived listener
# bind as configuration preflight; it does not own a live transport.  Keep the
# exception explicit and narrow while still checking every other socket call
# in this implementation module.
IO_FORBIDDEN_SOURCE_EXCEPTIONS: dict[tuple[str, str], tuple[str, ...]] = {
    (
        "BOUNDARY-AtmRuntime-Composition",
        "direct_socket_io",
    ): (
        r"\bstd::net::TcpListener\b",
        r"\bTcpListener::bind\s*\(",
        r"\bstd::net::SocketAddr\b",
    ),
}
SCB_CONFIG_ALLOWLIST_PATH = Path(".just/allowlists/scb_config_allowlist.toml")
SCB_CONFIG_FIXTURE_PATH = Path(".just/fixtures/scb_config_known_bad.rs")
SCB_RETAINED_ALLOWLIST_PATH = Path(".just/allowlists/scb_retained_allowlist.toml")
SCB_RETAINED_FIXTURE_PATH = Path(".just/fixtures/scb_runtime_known_bad.rs")
SCB_WORKSPACE_ALLOWLIST_PATH = Path(".just/allowlists/scb_workspace_allowlist.toml")
SCB_WORKSPACE_FIXTURE_PATH = Path(".just/fixtures/scb_workspace_known_bad.rs")
SCB_SINGLETON_ALLOWLIST_PATH = Path(".just/allowlists/scb_singleton_allowlist.toml")
SCB_SINGLETON_FIXTURE_PATH = Path(".just/fixtures/scb_singleton_known_bad.rs")
SCB_OBSERVABILITY_ALLOWLIST_PATH = Path(".just/allowlists/scb_observability_allowlist.toml")
SCB_OBSERVABILITY_FIXTURE_PATH = Path(".just/fixtures/scb_observability_known_bad.rs")
SCB_CONFIG_DIRECT_PATTERNS = ("config::load_team_config(", "load_claude_team_config_document(")
SCB_CONFIG_GENERIC_HELPER_PATTERNS = (
    "fn load_workspace_config(",
    "crate::boundary_support::load_workspace_config(",
    "direct_boundaries::load_workspace_config(",
    "atm_core::direct_boundaries::load_workspace_config(",
)
SCB_CONFIG_SEND_PATTERNS = (
    "config::load_team_config(",
    "load_claude_team_config_document(",
    ".load_team_config(",
)
SCB_CONFIG_BOUNDARY_FILES = (
    Path("crates/atm-core/src/boundary_support.rs"),
    # Keep guarding this retired duplicate path if it is ever reintroduced.
    Path("crates/atm-core/src/direct_boundaries.rs"),
)
SCB_CONFIG_CANONICAL_HELPER_FILE = Path("crates/atm-core/src/boundary_support.rs")
# team_admin's sibling split files (restore.rs, filesystem.rs, projection.rs) were
# reviewed as of the member_mutation.rs split (PR #471) and found not to call
# service_runtime_store::default_runtime() or load_config() -- no gate entry needed
# for them today. Re-check if business logic migrates into those files later.
SCB_RETAINED_DIRECT_PATTERNS = ("service_runtime_store::default_runtime()",)
SCB_RETAINED_TARGET_FILES = (
    Path("crates/atm/src/commands/teams.rs"),
    Path("crates/atm/src/commands/members.rs"),
    Path("crates/atm-core/src/team_admin.rs"),
    Path("crates/atm-core/src/team_admin/member_mutation.rs"),
)
SCB_WORKSPACE_DIRECT_PATTERNS = ("load_config(",)
SCB_WORKSPACE_TARGET_FILES = (
    Path("crates/atm/src/commands/teams.rs"),
    Path("crates/atm/src/commands/members.rs"),
    Path("crates/atm-core/src/team_admin.rs"),
    Path("crates/atm-core/src/team_admin/member_mutation.rs"),
)
SCB_SINGLETON_ROOT_FORBIDDEN_PATTERNS = (
    "pub use service_runtime_store::install_default_runtime_factory",
    "pub use service_runtime_store::install_default_runtime_instance",
    "pub use crate::service_runtime_store::install_default_runtime_factory",
    "pub use crate::service_runtime_store::install_default_runtime_instance",
)
SCB_SINGLETON_HIDDEN_HOOK_PATTERNS = (
    "runtime_install_hooks::install_retained_runtime_factory_for_daemon_bootstrap(",
    "runtime_install_hooks::install_retained_runtime_factory_for_test_support(",
    "runtime_install_hooks::install_retained_runtime_instance_for_daemon(",
)
SCB_SINGLETON_TARGET_FILES = (
    Path("crates/atm-core/src/lib.rs"),
    Path("crates/atm-core/src/runtime_install_hooks.rs"),
    Path("crates/atm-daemon-bootstrap/src/lib.rs"),
    Path("crates/atm-runtime-test-support/src/lib.rs"),
    Path("crates/atm-daemon/src/composition.rs"),
    Path("crates/atm-daemon/src/tests.rs"),
    Path("crates/atm-storage-rusqlite/src/lib.rs"),
)
SCB_SINGLETON_ALLOWED_HOOK_CALLERS = {
    Path("crates/atm-daemon-bootstrap/src/lib.rs"),
    Path("crates/atm-runtime-test-support/src/lib.rs"),
    Path("crates/atm-daemon/src/composition.rs"),
}
SCB_OBSERVABILITY_ALLOWED_SRC_FILES = {
    Path("crates/atm-daemon/src/daemon_runtime_observability.rs"),
    Path("crates/atm-daemon/src/main.rs"),
}
SCB_OBSERVABILITY_DIRECT_PATTERNS = (
    "sc_observability_types::ActionName",
    "sc_observability_types::OutcomeLabel",
)


@dataclass(frozen=True)
class BoundaryViolation:
    location: str
    message: str

    def render(self) -> str:
        if not self.message:
            return self.location
        return f"{self.location}: {self.message}"


@dataclass(frozen=True)
class BoundaryRecord:
    boundary_id: str
    owner_package: str
    owner_crate_path: str
    name: str
    public_trait: str | None
    public_facade: str | None
    implementation_type: str | None
    implementation_module: str | None
    implementation_visibility: str
    implementation_constructor: str
    composition_roots: tuple[str, ...]
    io_owns: tuple[str, ...]
    io_forbidden: tuple[str, ...]
    allowed_dependents: tuple[str, ...]
    allowed_dependencies: tuple[str, ...]
    forbidden_edges: tuple[str, ...]
    references_scope: str
    forbidden_references: tuple[str, ...]
    allowed_test_double_paths: tuple[str, ...]
    forbidden_test_bypasses: tuple[str, ...]
    lint_rules: tuple[str, ...]
    review_gates: tuple[str, ...]
    status_state: str
    source_path: Path
    start_line: int
    raw: dict[str, object]

    @property
    def is_active(self) -> bool:
        return self.status_state in {
            "active",
            "stub_landed",
            "concrete_landed",
            "unix_implemented_windows_pending",
        }

    @property
    def location(self) -> str:
        return f"{self.source_path.as_posix()}:{self.start_line} [{self.boundary_id}]"


@dataclass(frozen=True)
class ManifestInfo:
    path: Path
    package_name: str
    crate_dir_name: str
    crate_path_name: str

    @property
    def aliases(self) -> tuple[str, ...]:
        aliases = {self.package_name, self.crate_dir_name}
        aliases.add(self.crate_path_name)
        return tuple(sorted(aliases))


@dataclass(frozen=True)
class DependencyOwnershipRule:
    dependency: str
    allowed_manifest_paths: tuple[Path, ...]
    allowed_source_roots: tuple[Path, ...]
    manifest_message: str
    source_message: str


@dataclass(frozen=True)
class ManifestSectionRule:
    owner_manifest_path: Path
    dependency_package: str
    allowed_sections: tuple[str, ...]
    message: str


@dataclass(frozen=True)
class ManifestDependencyAllowlist:
    owner_manifest_path: Path
    allowed_dependencies: tuple[str, ...]


@dataclass(frozen=True)
class ScbConfigAllowlistEntry:
    rule: str
    path: Path
    symbol: str
    why: str
    sunset_sprint: str


@dataclass(frozen=True)
class ScbRetainedAllowlistEntry:
    rule: str
    path: Path
    symbol: str
    why: str
    sunset_sprint: str


@dataclass(frozen=True)
class ScbWorkspaceAllowlistEntry:
    rule: str
    path: Path
    symbol: str
    why: str
    sunset_sprint: str


@dataclass(frozen=True)
class ScbSingletonAllowlistEntry:
    rule: str
    path: Path
    symbol: str
    why: str
    sunset_sprint: str


@dataclass(frozen=True)
class ScbObservabilityAllowlistEntry:
    rule: str
    path: Path
    symbol: str
    why: str
    sunset_sprint: str


def dependency_sections(manifest: dict) -> list[tuple[str, dict]]:
    sections: list[tuple[str, dict]] = []
    for section_name in ("dependencies", "dev-dependencies", "build-dependencies"):
        dependencies = manifest.get(section_name)
        if isinstance(dependencies, dict):
            sections.append((section_name, dependencies))

    targets = manifest.get("target", {})
    if isinstance(targets, dict):
        for target_name, target in targets.items():
            if not isinstance(target, dict):
                continue
            for section_name in ("dependencies", "dev-dependencies", "build-dependencies"):
                dependencies = target.get(section_name)
                if isinstance(dependencies, dict):
                    sections.append((f"target.{target_name}.{section_name}", dependencies))

    return sections


def dependency_package_name(dependency_name: str, dependency: object) -> str:
    if isinstance(dependency, str):
        return dependency_name
    if isinstance(dependency, dict):
        package_name = dependency.get("package")
        if isinstance(package_name, str):
            return package_name
    return dependency_name


def dependency_import_patterns(dependency: str) -> tuple[re.Pattern[str], ...]:
    crate_path = dependency.replace("-", "_")
    escaped = re.escape(crate_path)
    return (
        re.compile(rf"\b{escaped}::"),
        re.compile(rf"\buse\s+{escaped}\b"),
        re.compile(rf"\bextern\s+crate\s+{escaped}\b"),
    )


def boundary_config(repo_root: Path) -> dict:
    config = load_lint_config(repo_root).get("boundaries", {})
    if not isinstance(config, dict):
        raise SystemExit("[boundaries] must be a TOML table")
    return config


def workspace_manifests(repo_root: Path) -> list[Path]:
    return workspace_manifest_paths(repo_root)


def rust_sources(repo_root: Path) -> list[Path]:
    sources: list[Path] = []
    for manifest_path in workspace_manifest_paths(repo_root):
        src_root = manifest_path.parent / "src"
        if not src_root.exists():
            continue
        sources.extend(sorted(src_root.glob("**/*.rs")))
    return sorted(set(sources))


def rust_test_sources_for_crate(info: ManifestInfo) -> list[Path]:
    tests_root = info.path.parent / "tests"
    if not tests_root.exists():
        return []
    return sorted(tests_root.glob("**/*.rs"))


def boundary_docs(repo_root: Path) -> list[Path]:
    config = boundary_config(repo_root)
    doc_glob = config.get("doc_glob")
    if not isinstance(doc_glob, str) or not doc_glob.strip():
        raise SystemExit("[boundaries].doc_glob must be a non-empty string")
    return sorted(repo_root.glob(doc_glob))


def boundary_toml_files(repo_root: Path) -> list[Path]:
    config = boundary_config(repo_root)
    toml_glob = config.get("toml_glob")
    if toml_glob is None:
        return []
    if not isinstance(toml_glob, str) or not toml_glob.strip():
        raise SystemExit("[boundaries].toml_glob must be a non-empty string when provided")
    return sorted(repo_root.glob(toml_glob))


def dependency_ownership_rules(repo_root: Path) -> list[DependencyOwnershipRule]:
    config = boundary_config(repo_root)
    raw_rules = config.get("global_dependency_ownership", [])
    if not isinstance(raw_rules, list):
        raise SystemExit("[[boundaries.global_dependency_ownership]] entries must be an array of tables")

    rules: list[DependencyOwnershipRule] = []
    for index, raw_rule in enumerate(raw_rules):
        if not isinstance(raw_rule, dict):
            raise SystemExit(
                f"[boundaries.global_dependency_ownership][{index}] must be a TOML table"
            )
        dependency = raw_rule.get("dependency")
        allowed_manifest_paths = raw_rule.get("allowed_manifest_paths", [])
        allowed_source_roots = raw_rule.get("allowed_source_roots", [])
        manifest_message = raw_rule.get("manifest_message")
        source_message = raw_rule.get("source_message")
        if not isinstance(dependency, str) or not dependency:
            raise SystemExit(
                f"[boundaries.global_dependency_ownership][{index}].dependency must be a non-empty string"
            )
        if not isinstance(allowed_manifest_paths, list) or not all(isinstance(item, str) for item in allowed_manifest_paths):
            raise SystemExit(
                f"[boundaries.global_dependency_ownership][{index}].allowed_manifest_paths must be an array of strings"
            )
        if not isinstance(allowed_source_roots, list) or not all(isinstance(item, str) for item in allowed_source_roots):
            raise SystemExit(
                f"[boundaries.global_dependency_ownership][{index}].allowed_source_roots must be an array of strings"
            )
        if not isinstance(manifest_message, str) or not manifest_message:
            raise SystemExit(
                f"[boundaries.global_dependency_ownership][{index}].manifest_message must be a non-empty string"
            )
        if not isinstance(source_message, str) or not source_message:
            raise SystemExit(
                f"[boundaries.global_dependency_ownership][{index}].source_message must be a non-empty string"
            )
        rules.append(
            DependencyOwnershipRule(
                dependency=dependency,
                allowed_manifest_paths=tuple(Path(item) for item in allowed_manifest_paths),
                allowed_source_roots=tuple(Path(item) for item in allowed_source_roots),
                manifest_message=manifest_message,
                source_message=source_message,
            )
        )
    return rules


def manifest_section_rules(repo_root: Path) -> list[ManifestSectionRule]:
    config = boundary_config(repo_root)
    raw_rules = config.get("manifest_section_rules", [])
    if not isinstance(raw_rules, list):
        raise SystemExit("[[boundaries.manifest_section_rules]] entries must be an array of tables")

    rules: list[ManifestSectionRule] = []
    for index, raw_rule in enumerate(raw_rules):
        if not isinstance(raw_rule, dict):
            raise SystemExit(
                f"[boundaries.manifest_section_rules][{index}] must be a TOML table"
            )
        owner_manifest_path = raw_rule.get("owner_manifest_path")
        dependency_package = raw_rule.get("dependency_package")
        allowed_sections = raw_rule.get("allowed_sections", [])
        message = raw_rule.get("message")
        if not isinstance(owner_manifest_path, str) or not owner_manifest_path:
            raise SystemExit(
                f"[boundaries.manifest_section_rules][{index}].owner_manifest_path must be a non-empty string"
            )
        if not isinstance(dependency_package, str) or not dependency_package:
            raise SystemExit(
                f"[boundaries.manifest_section_rules][{index}].dependency_package must be a non-empty string"
            )
        if not isinstance(allowed_sections, list) or not all(isinstance(item, str) for item in allowed_sections):
            raise SystemExit(
                f"[boundaries.manifest_section_rules][{index}].allowed_sections must be an array of strings"
            )
        if not isinstance(message, str) or not message:
            raise SystemExit(
                f"[boundaries.manifest_section_rules][{index}].message must be a non-empty string"
            )
        rules.append(
            ManifestSectionRule(
                owner_manifest_path=Path(owner_manifest_path),
                dependency_package=dependency_package,
                allowed_sections=tuple(allowed_sections),
                message=message,
            )
        )
    return rules


def manifest_dependency_allowlists(repo_root: Path) -> list[ManifestDependencyAllowlist]:
    config = boundary_config(repo_root)
    raw_rules = config.get("manifest_dependency_allowlists", [])
    if not isinstance(raw_rules, list):
        raise SystemExit(
            "[[boundaries.manifest_dependency_allowlists]] entries must be an array of tables"
        )

    rules: list[ManifestDependencyAllowlist] = []
    for index, raw_rule in enumerate(raw_rules):
        if not isinstance(raw_rule, dict):
            raise SystemExit(
                f"[boundaries.manifest_dependency_allowlists][{index}] must be a TOML table"
            )
        owner_manifest_path = raw_rule.get("owner_manifest_path")
        allowed_dependencies = raw_rule.get("allowed_dependencies")
        if not isinstance(owner_manifest_path, str) or not owner_manifest_path:
            raise SystemExit(
                f"[boundaries.manifest_dependency_allowlists][{index}].owner_manifest_path must be a non-empty string"
            )
        if not isinstance(allowed_dependencies, list) or not all(
            isinstance(item, str) and item for item in allowed_dependencies
        ):
            raise SystemExit(
                f"[boundaries.manifest_dependency_allowlists][{index}].allowed_dependencies must be an array of non-empty strings"
            )
        rules.append(
            ManifestDependencyAllowlist(
                owner_manifest_path=Path(owner_manifest_path),
                allowed_dependencies=tuple(allowed_dependencies),
            )
        )
    return rules


def tomllib_load(path: Path) -> dict:
    import tomllib

    return tomllib.loads(path.read_text(encoding="utf-8"))


def scb_config_allowlist(repo_root: Path) -> list[ScbConfigAllowlistEntry]:
    allowlist_path = repo_root / SCB_CONFIG_ALLOWLIST_PATH
    if not allowlist_path.exists():
        raise SystemExit(f"[boundaries] missing required allowlist: {SCB_CONFIG_ALLOWLIST_PATH.as_posix()}")
    data = tomllib_load(allowlist_path)
    raw_entries = data.get("allow", [])
    if not isinstance(raw_entries, list):
        raise SystemExit("[boundaries.allow] must be an array of tables")

    entries: list[ScbConfigAllowlistEntry] = []
    for index, raw_entry in enumerate(raw_entries):
        if not isinstance(raw_entry, dict):
            raise SystemExit(f"[boundaries.allow][{index}] must be a TOML table")
        required = ("rule", "path", "symbol", "why", "sunset_sprint")
        for field in required:
            value = raw_entry.get(field)
            if not isinstance(value, str) or not value.strip():
                raise SystemExit(
                    f"[boundaries.allow][{index}].{field} must be a non-empty string"
                )
        entries.append(
            ScbConfigAllowlistEntry(
                rule=raw_entry["rule"],
                path=Path(raw_entry["path"]),
                symbol=raw_entry["symbol"],
                why=raw_entry["why"],
                sunset_sprint=raw_entry["sunset_sprint"],
            )
        )
    return entries


def scb_retained_allowlist(repo_root: Path) -> list[ScbRetainedAllowlistEntry]:
    allowlist_path = repo_root / SCB_RETAINED_ALLOWLIST_PATH
    if not allowlist_path.exists():
        raise SystemExit(
            f"[boundaries] missing required allowlist: {SCB_RETAINED_ALLOWLIST_PATH.as_posix()}"
        )
    data = tomllib_load(allowlist_path)
    raw_entries = data.get("allow", [])
    if not isinstance(raw_entries, list):
        raise SystemExit("[boundaries.allow] must be an array of tables")

    entries: list[ScbRetainedAllowlistEntry] = []
    for index, raw_entry in enumerate(raw_entries):
        if not isinstance(raw_entry, dict):
            raise SystemExit(f"[boundaries.allow][{index}] must be a TOML table")
        required = ("rule", "path", "symbol", "why", "sunset_sprint")
        for field in required:
            value = raw_entry.get(field)
            if not isinstance(value, str) or not value.strip():
                raise SystemExit(
                    f"[boundaries.allow][{index}].{field} must be a non-empty string"
                )
        entries.append(
            ScbRetainedAllowlistEntry(
                rule=raw_entry["rule"],
                path=Path(raw_entry["path"]),
                symbol=raw_entry["symbol"],
                why=raw_entry["why"],
                sunset_sprint=raw_entry["sunset_sprint"],
            )
        )
    return entries


def scb_workspace_allowlist(repo_root: Path) -> list[ScbWorkspaceAllowlistEntry]:
    allowlist_path = repo_root / SCB_WORKSPACE_ALLOWLIST_PATH
    if not allowlist_path.exists():
        raise SystemExit(
            f"[boundaries] missing required allowlist: {SCB_WORKSPACE_ALLOWLIST_PATH.as_posix()}"
        )
    data = tomllib_load(allowlist_path)
    raw_entries = data.get("allow", [])
    if not isinstance(raw_entries, list):
        raise SystemExit("[boundaries.allow] must be an array of tables")

    entries: list[ScbWorkspaceAllowlistEntry] = []
    for index, raw_entry in enumerate(raw_entries):
        if not isinstance(raw_entry, dict):
            raise SystemExit(f"[boundaries.allow][{index}] must be a TOML table")
        required = ("rule", "path", "symbol", "why", "sunset_sprint")
        for field in required:
            value = raw_entry.get(field)
            if not isinstance(value, str) or not value.strip():
                raise SystemExit(
                    f"[boundaries.allow][{index}].{field} must be a non-empty string"
                )
        entries.append(
            ScbWorkspaceAllowlistEntry(
                rule=raw_entry["rule"],
                path=Path(raw_entry["path"]),
                symbol=raw_entry["symbol"],
                why=raw_entry["why"],
                sunset_sprint=raw_entry["sunset_sprint"],
            )
        )
    return entries


def scb_singleton_allowlist(repo_root: Path) -> list[ScbSingletonAllowlistEntry]:
    allowlist_path = repo_root / SCB_SINGLETON_ALLOWLIST_PATH
    if not allowlist_path.exists():
        raise SystemExit(
            f"[boundaries] missing required allowlist: {SCB_SINGLETON_ALLOWLIST_PATH.as_posix()}"
        )
    data = tomllib_load(allowlist_path)
    raw_entries = data.get("allow", [])
    if not isinstance(raw_entries, list):
        raise SystemExit("[boundaries.allow] must be an array of tables")

    entries: list[ScbSingletonAllowlistEntry] = []
    for index, raw_entry in enumerate(raw_entries):
        if not isinstance(raw_entry, dict):
            raise SystemExit(f"[boundaries.allow][{index}] must be a TOML table")
        required = ("rule", "path", "symbol", "why", "sunset_sprint")
        for field in required:
            value = raw_entry.get(field)
            if not isinstance(value, str) or not value.strip():
                raise SystemExit(
                    f"[boundaries.allow][{index}].{field} must be a non-empty string"
                )
        entries.append(
            ScbSingletonAllowlistEntry(
                rule=raw_entry["rule"],
                path=Path(raw_entry["path"]),
                symbol=raw_entry["symbol"],
                why=raw_entry["why"],
                sunset_sprint=raw_entry["sunset_sprint"],
            )
        )
    return entries


def scb_observability_allowlist(repo_root: Path) -> list[ScbObservabilityAllowlistEntry]:
    allowlist_path = repo_root / SCB_OBSERVABILITY_ALLOWLIST_PATH
    if not allowlist_path.exists():
        raise SystemExit(
            f"[boundaries] missing required allowlist: {SCB_OBSERVABILITY_ALLOWLIST_PATH.as_posix()}"
        )
    data = tomllib_load(allowlist_path)
    raw_entries = data.get("allow", [])
    if not isinstance(raw_entries, list):
        raise SystemExit("[boundaries.allow] must be an array of tables")

    entries: list[ScbObservabilityAllowlistEntry] = []
    for index, raw_entry in enumerate(raw_entries):
        if not isinstance(raw_entry, dict):
            raise SystemExit(f"[boundaries.allow][{index}] must be a TOML table")
        required = ("rule", "path", "symbol", "why", "sunset_sprint")
        for field in required:
            value = raw_entry.get(field)
            if not isinstance(value, str) or not value.strip():
                raise SystemExit(
                    f"[boundaries.allow][{index}].{field} must be a non-empty string"
                )
        entries.append(
            ScbObservabilityAllowlistEntry(
                rule=raw_entry["rule"],
                path=Path(raw_entry["path"]),
                symbol=raw_entry["symbol"],
                why=raw_entry["why"],
                sunset_sprint=raw_entry["sunset_sprint"],
            )
        )
    return entries


def enclosing_function_name(lines: list[str], line_number: int) -> str | None:
    for index in range(line_number - 1, -1, -1):
        line = lines[index].strip()
        match = re.match(r"(?:pub(?:\([^)]*\))?\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(", line)
        if match is not None:
            return match.group(1)
    return None


def is_allowlisted_config_violation(
    *,
    entries: list[ScbConfigAllowlistEntry],
    rule: str,
    rel_path: Path,
    symbol: str | None,
) -> bool:
    for entry in entries:
        if entry.rule != rule:
            continue
        if entry.path != rel_path:
            continue
        if symbol is None or entry.symbol != symbol:
            continue
        return True
    return False


def is_allowlisted_retained_violation(
    *,
    entries: list[ScbRetainedAllowlistEntry],
    rule: str,
    rel_path: Path,
    symbol: str | None,
) -> bool:
    for entry in entries:
        if entry.rule != rule:
            continue
        if entry.path != rel_path:
            continue
        if symbol is None or entry.symbol != symbol:
            continue
        return True
    return False


def is_allowlisted_workspace_violation(
    *,
    entries: list[ScbWorkspaceAllowlistEntry],
    rule: str,
    rel_path: Path,
    symbol: str | None,
) -> bool:
    for entry in entries:
        if entry.rule != rule:
            continue
        if entry.path != rel_path:
            continue
        if symbol is None or entry.symbol != symbol:
            continue
        return True
    return False


def is_allowlisted_singleton_violation(
    *,
    entries: list[ScbSingletonAllowlistEntry],
    rule: str,
    rel_path: Path,
    symbol: str | None,
) -> bool:
    for entry in entries:
        if entry.rule != rule:
            continue
        if entry.path != rel_path:
            continue
        if symbol is None or entry.symbol != symbol:
            continue
        return True
    return False


def is_allowlisted_observability_violation(
    *,
    entries: list[ScbObservabilityAllowlistEntry],
    rule: str,
    rel_path: Path,
    symbol: str | None,
) -> bool:
    for entry in entries:
        if entry.rule != rule:
            continue
        if entry.path != rel_path:
            continue
        if symbol is None or entry.symbol != symbol:
            continue
        return True
    return False


def scb_config_fixture_violation(
    violations: list[BoundaryViolation],
    expected_rules: set[str],
) -> BoundaryViolation | None:
    observed_rules = {
        violation.location.split(" ", 1)[0]
        for violation in violations
        if violation.location.startswith("SCB-CONFIG-")
    }
    missing = sorted(expected_rules - observed_rules)
    if not missing:
        return None
    return BoundaryViolation(
        f"{SCB_CONFIG_FIXTURE_PATH.as_posix()}: fixture self-test did not reject {', '.join(missing)}",
        "",
    )


def scb_retained_fixture_violation(
    violations: list[BoundaryViolation],
    expected_rules: set[str],
) -> BoundaryViolation | None:
    observed_rules = {
        violation.location.split(" ", 1)[0]
        for violation in violations
        if violation.location.startswith("SCB-RETAINED-")
    }
    missing = sorted(expected_rules - observed_rules)
    if not missing:
        return None
    return BoundaryViolation(
        f"{SCB_RETAINED_FIXTURE_PATH.as_posix()}: fixture self-test did not reject {', '.join(missing)}",
        "",
    )


def scb_workspace_fixture_violation(
    violations: list[BoundaryViolation],
    expected_rules: set[str],
) -> BoundaryViolation | None:
    observed_rules = {
        violation.location.split(" ", 1)[0]
        for violation in violations
        if violation.location.startswith("SCB-WORKSPACE-")
    }
    missing = sorted(expected_rules - observed_rules)
    if not missing:
        return None
    return BoundaryViolation(
        f"{SCB_WORKSPACE_FIXTURE_PATH.as_posix()}: fixture self-test did not reject {', '.join(missing)}",
        "",
    )


def scb_singleton_fixture_violation(
    violations: list[BoundaryViolation],
    expected_rules: set[str],
) -> BoundaryViolation | None:
    observed_rules = {
        violation.location.split(" ", 1)[0]
        for violation in violations
        if violation.location.startswith("SCB-SINGLETON-")
    }
    missing = sorted(expected_rules - observed_rules)
    if not missing:
        return None
    return BoundaryViolation(
        f"{SCB_SINGLETON_FIXTURE_PATH.as_posix()}: fixture self-test did not reject {', '.join(missing)}",
        "",
    )


def scb_observability_fixture_violation(
    violations: list[BoundaryViolation],
    expected_rules: set[str],
) -> BoundaryViolation | None:
    observed_rules = {
        violation.location.split(" ", 1)[0]
        for violation in violations
        if violation.location.startswith("SCB-OBSERVABILITY-")
    }
    missing = sorted(expected_rules - observed_rules)
    if not missing:
        return None
    return BoundaryViolation(
        f"{SCB_OBSERVABILITY_FIXTURE_PATH.as_posix()}: fixture self-test did not reject {', '.join(missing)}",
        "",
    )


def yaml_scalar(value: str) -> object:
    stripped = value.strip()
    if stripped == "null":
        return None
    if stripped == "[]":
        return []
    if stripped.lower() == "true":
        return True
    if stripped.lower() == "false":
        return False
    return stripped


def leading_spaces(text: str) -> int:
    return len(text) - len(text.lstrip(" "))


def next_content_line(lines: list[tuple[int, str]], start_index: int) -> tuple[int, str] | None:
    for index in range(start_index, len(lines)):
        line_number, text = lines[index]
        if text.strip():
            return line_number, text
    return None


def parse_yaml_list(lines: list[tuple[int, str]], start_index: int, indent: int) -> tuple[list[object], int]:
    items: list[object] = []
    index = start_index
    while index < len(lines):
        line_number, text = lines[index]
        if not text.strip():
            index += 1
            continue
        current_indent = leading_spaces(text)
        if current_indent < indent:
            break
        if current_indent > indent:
            raise ValueError(f"unexpected indentation in list item at line {line_number}: {text!r}")
        stripped = text.strip()
        if not stripped.startswith("- "):
            break
        items.append(yaml_scalar(stripped[2:]))
        index += 1
    return items, index


def parse_yaml_mapping(lines: list[tuple[int, str]], start_index: int, indent: int) -> tuple[dict[str, object], int]:
    mapping: dict[str, object] = {}
    index = start_index
    while index < len(lines):
        line_number, text = lines[index]
        if not text.strip():
            index += 1
            continue

        current_indent = leading_spaces(text)
        if current_indent < indent:
            break
        if current_indent > indent:
            raise ValueError(f"unexpected indentation in mapping at line {line_number}: {text!r}")

        stripped = text.strip()
        if stripped.startswith("- "):
            raise ValueError(f"unexpected list item in mapping at line {line_number}: {text!r}")
        if ":" not in stripped:
            raise ValueError(f"expected key/value pair at line {line_number}: {text!r}")

        key, remainder = stripped.split(":", 1)
        remainder = remainder.strip()
        index += 1

        if remainder:
            mapping[key] = yaml_scalar(remainder)
            continue

        next_line = next_content_line(lines, index)
        if next_line is None:
            mapping[key] = {}
            continue

        next_line_number, next_text = next_line
        next_indent = leading_spaces(next_text)
        if next_indent <= current_indent:
            mapping[key] = {}
            continue

        if next_text.strip().startswith("- "):
            value, index = parse_yaml_list(lines, index, next_indent)
        else:
            value, index = parse_yaml_mapping(lines, index, next_indent)
        mapping[key] = value

    return mapping, index


def parse_simple_yaml_document(text: str) -> dict[str, object]:
    lines = [(line_number, line) for line_number, line in enumerate(text.splitlines(), start=1)]
    mapping, _ = parse_yaml_mapping(lines, 0, 0)
    return mapping


def extract_yaml_blocks(path: Path) -> list[tuple[int, str]]:
    blocks: list[tuple[int, str]] = []
    lines = path.read_text(encoding="utf-8").splitlines()
    in_block = False
    start_line = 0
    buffer: list[str] = []
    for line_number, line in enumerate(lines, start=1):
        stripped = line.strip()
        if not in_block and stripped == YAML_FENCE_START:
            in_block = True
            start_line = line_number + 1
            buffer = []
            continue
        if in_block and stripped == YAML_FENCE_END:
            blocks.append((start_line, "\n".join(buffer)))
            in_block = False
            buffer = []
            continue
        if in_block:
            buffer.append(line)
    return blocks


def nested_get(data: dict[str, object], path: tuple[str, ...]) -> object | None:
    current: object = data
    for segment in path:
        if not isinstance(current, dict):
            return None
        current = current.get(segment)
    return current


def as_string(value: object) -> str | None:
    if value is None:
        return None
    if isinstance(value, str):
        return value
    return None


def as_string_list(value: object) -> list[str] | None:
    if isinstance(value, list) and all(isinstance(item, str) for item in value):
        return [str(item) for item in value]
    return None


def validate_required_fields(data: dict[str, object]) -> list[str]:
    errors: list[str] = []
    for path in REQUIRED_BOUNDARY_FIELDS:
        value = nested_get(data, path)
        if value is None:
            errors.append(f"missing required field: {'.'.join(path)}")
            continue
        if isinstance(value, str) and not value.strip():
            errors.append(f"missing required field: {'.'.join(path)}")

    public_trait = nested_get(data, ("public", "trait"))
    public_facade = nested_get(data, ("public", "facade"))
    if public_trait is None and public_facade is None:
        errors.append("missing required field: public.(trait|facade)")
    return errors


def validate_package_name(value: str, *, field_name: str) -> str | None:
    if not PACKAGE_NAME_RE.match(value):
        return f"invalid {field_name}: {value!r}"
    return None


def validate_rust_path(value: str, *, field_name: str) -> str | None:
    if not RUST_PATH_RE.match(value):
        return f"invalid {field_name}: {value!r}"
    return None


def validate_identifier(value: str, *, field_name: str) -> str | None:
    if not IDENTIFIER_RE.match(value):
        return f"invalid {field_name}: {value!r}"
    return None


def validate_list_field(
    data: dict[str, object],
    path: tuple[str, ...],
    *,
    field_name: str,
    allow_empty: bool = True,
) -> tuple[list[str], list[str]]:
    value = nested_get(data, path)
    rendered = as_string_list(value)
    if rendered is None:
        return [], [f"{field_name} must be a list of strings"]
    if not allow_empty and not rendered:
        return rendered, [f"{field_name} must not be empty"]
    return rendered, []


def build_boundary_record(
    *,
    data: dict[str, object],
    source_path: Path,
    start_line: int,
) -> tuple[BoundaryRecord | None, list[BoundaryViolation]]:
    base_location = f"{source_path.as_posix()}:{start_line}"
    errors = validate_required_fields(data)

    boundary_id = as_string(nested_get(data, ("boundary_id",)))
    owner_package = as_string(nested_get(data, ("owner_package",)))
    owner_crate_path = as_string(nested_get(data, ("owner_crate_path",)))
    name = as_string(nested_get(data, ("name",)))
    public_trait = as_string(nested_get(data, ("public", "trait")))
    public_facade = as_string(nested_get(data, ("public", "facade")))
    implementation_type = as_string(nested_get(data, ("implementation", "type")))
    implementation_module = as_string(nested_get(data, ("implementation", "module")))
    implementation_visibility = as_string(nested_get(data, ("implementation", "visibility")))
    implementation_constructor = as_string(nested_get(data, ("implementation", "constructor")))
    references_scope = as_string(nested_get(data, ("references", "scope")))
    status_state = as_string(nested_get(data, ("status", "state")))

    composition_roots, composition_errors = validate_list_field(
        data,
        ("composition", "roots"),
        field_name="composition.roots",
    )
    io_owns, io_owns_errors = validate_list_field(
        data,
        ("ownership", "io_owns"),
        field_name="ownership.io_owns",
    )
    io_forbidden, io_forbidden_errors = validate_list_field(
        data,
        ("ownership", "io_forbidden"),
        field_name="ownership.io_forbidden",
    )
    allowed_dependents, allowed_dependent_errors = validate_list_field(
        data,
        ("dependencies", "allowed_dependents"),
        field_name="dependencies.allowed_dependents",
    )
    allowed_dependencies, allowed_dependency_errors = validate_list_field(
        data,
        ("dependencies", "allowed_dependencies"),
        field_name="dependencies.allowed_dependencies",
    )
    forbidden_edges, forbidden_edge_errors = validate_list_field(
        data,
        ("dependencies", "forbidden_edges"),
        field_name="dependencies.forbidden_edges",
    )
    forbidden_references, forbidden_reference_errors = validate_list_field(
        data,
        ("references", "forbidden"),
        field_name="references.forbidden",
    )
    allowed_test_double_paths, test_double_errors = validate_list_field(
        data,
        ("testing", "allowed_test_double_paths"),
        field_name="testing.allowed_test_double_paths",
    )
    forbidden_test_bypasses, test_bypass_errors = validate_list_field(
        data,
        ("testing", "forbidden_test_bypasses"),
        field_name="testing.forbidden_test_bypasses",
    )
    lint_rules, lint_rule_errors = validate_list_field(
        data,
        ("enforcement", "lint_rules"),
        field_name="enforcement.lint_rules",
        allow_empty=False,
    )
    review_gates, review_gate_errors = validate_list_field(
        data,
        ("enforcement", "review_gates"),
        field_name="enforcement.review_gates",
        allow_empty=False,
    )

    errors.extend(
        composition_errors
        + io_owns_errors
        + io_forbidden_errors
        + allowed_dependent_errors
        + allowed_dependency_errors
        + forbidden_edge_errors
        + forbidden_reference_errors
        + test_double_errors
        + test_bypass_errors
        + lint_rule_errors
        + review_gate_errors
    )

    for label, value in (
        ("boundary_id", boundary_id),
        ("owner_package", owner_package),
        ("name", name),
    ):
        if value is None:
            continue
        error = validate_package_name(value, field_name=label) if label == "owner_package" else None
        if error:
            errors.append(error)

    if owner_crate_path is not None:
        error = validate_identifier(owner_crate_path, field_name="owner_crate_path")
        if error:
            errors.append(error)
    if public_trait is not None:
        error = validate_identifier(public_trait, field_name="public.trait")
        if error:
            errors.append(error)
    if public_facade is not None:
        error = validate_identifier(public_facade, field_name="public.facade")
        if error:
            errors.append(error)
    if implementation_type is not None:
        error = validate_identifier(implementation_type, field_name="implementation.type")
        if error:
            errors.append(error)
    if implementation_module is not None:
        error = validate_rust_path(implementation_module, field_name="implementation.module")
        if error:
            errors.append(error)

    if implementation_visibility is not None and implementation_visibility not in VISIBILITY_VALUES:
        errors.append(
            "implementation.visibility must be one of: "
            + ", ".join(sorted(VISIBILITY_VALUES))
        )
    if implementation_constructor is not None and implementation_constructor not in CONSTRUCTOR_VALUES:
        errors.append(
            "implementation.constructor must be one of: "
            + ", ".join(sorted(CONSTRUCTOR_VALUES))
        )
    if references_scope is not None and references_scope not in REFERENCE_SCOPE_VALUES:
        errors.append("references.scope must be one of: " + ", ".join(sorted(REFERENCE_SCOPE_VALUES)))
    if status_state is not None and status_state not in STATE_VALUES:
        errors.append("status.state must be one of: " + ", ".join(sorted(STATE_VALUES)))

    if public_trait is None and public_facade is None:
        errors.append("one of public.trait or public.facade must be provided")

    if implementation_visibility == "trait_only":
        if implementation_type is not None:
            errors.append("implementation.type must be null when implementation.visibility is trait_only")
        if implementation_module is not None:
            errors.append("implementation.module must be null when implementation.visibility is trait_only")
        if implementation_constructor != "none":
            errors.append("implementation.constructor must be none when implementation.visibility is trait_only")
    else:
        if implementation_type is None:
            errors.append("implementation.type is required for concrete boundaries")
        if implementation_module is None:
            errors.append("implementation.module is required for concrete boundaries")

    for path_name in composition_roots:
        error = validate_rust_path(path_name, field_name="composition.roots entry")
        if error:
            errors.append(error)
    for path_name in allowed_test_double_paths:
        error = validate_rust_path(path_name, field_name="testing.allowed_test_double_paths entry")
        if error:
            errors.append(error)
    for path_name in forbidden_test_bypasses:
        error = validate_rust_path(path_name, field_name="testing.forbidden_test_bypasses entry")
        if error:
            errors.append(error)
    for ref_name in forbidden_references:
        if "::" not in ref_name:
            error = validate_identifier(ref_name, field_name="references.forbidden entry")
        else:
            error = validate_rust_path(ref_name, field_name="references.forbidden entry")
        if error:
            errors.append(error)
    for rule_name in lint_rules:
        error = validate_identifier(rule_name.replace("-", "_"), field_name="enforcement.lint_rules entry")
        if error:
            errors.append(f"invalid enforcement.lint_rules entry: {rule_name!r}")
    for gate_name in review_gates:
        error = validate_identifier(gate_name.replace("-", "_"), field_name="enforcement.review_gates entry")
        if error:
            errors.append(f"invalid enforcement.review_gates entry: {gate_name!r}")
    for edge in forbidden_edges:
        match = FORBIDDEN_EDGE_RE.match(edge)
        if match is None:
            errors.append(f"invalid dependencies.forbidden_edges entry: {edge!r}")
        else:
            left_alias = match.group("left")
            right_alias = match.group("right")
            if left_alias == right_alias:
                errors.append(f"invalid dependencies.forbidden_edges self-edge: {edge!r}")

    if owner_package:
        source_parts = source_path.parts
        if len(source_parts) >= 3 and source_parts[0] == "docs" and source_parts[-1] == "boundaries.md":
            doc_owner = source_path.parent.name
            if doc_owner != owner_package:
                errors.append(
                    f"document path owner mismatch: docs/{doc_owner}/boundaries.md declares owner_package {owner_package!r}"
                )
        elif len(source_parts) >= 3 and source_parts[0] == "boundaries" and source_path.suffix == ".toml":
            file_owner = source_parts[1]
            if file_owner != owner_package:
                errors.append(
                    f"document path owner mismatch: boundaries/{file_owner}/{source_path.name} declares owner_package {owner_package!r}"
                )

    if owner_package and composition_roots:
        owner_crate_prefix = owner_crate_path or ""
        allowed_dependent_set = set(allowed_dependents)
        for root in composition_roots:
            crate_prefix = root.split("::", 1)[0]
            if crate_prefix != owner_crate_prefix and crate_prefix.replace("_", "-") not in allowed_dependent_set:
                errors.append(
                    f"composition root {root!r} must also appear in dependencies.allowed_dependents"
                )

    if errors:
        identifier = boundary_id or name or "unknown-boundary"
        return None, [BoundaryViolation(base_location, f"[{identifier}] {error}") for error in errors]

    assert boundary_id is not None
    assert owner_package is not None
    assert owner_crate_path is not None
    assert name is not None
    assert implementation_visibility is not None
    assert implementation_constructor is not None
    assert references_scope is not None
    assert status_state is not None

    record = BoundaryRecord(
        boundary_id=boundary_id,
        owner_package=owner_package,
        owner_crate_path=owner_crate_path,
        name=name,
        public_trait=public_trait,
        public_facade=public_facade,
        implementation_type=implementation_type,
        implementation_module=implementation_module,
        implementation_visibility=implementation_visibility,
        implementation_constructor=implementation_constructor,
        composition_roots=tuple(composition_roots),
        io_owns=tuple(io_owns),
        io_forbidden=tuple(io_forbidden),
        allowed_dependents=tuple(allowed_dependents),
        allowed_dependencies=tuple(allowed_dependencies),
        forbidden_edges=tuple(forbidden_edges),
        references_scope=references_scope,
        forbidden_references=tuple(forbidden_references),
        allowed_test_double_paths=tuple(allowed_test_double_paths),
        forbidden_test_bypasses=tuple(forbidden_test_bypasses),
        lint_rules=tuple(lint_rules),
        review_gates=tuple(review_gates),
        status_state=status_state,
        source_path=source_path,
        start_line=start_line,
        raw=data,
    )
    return record, []


def parse_boundary_records(repo_root: Path) -> tuple[list[BoundaryRecord], list[BoundaryViolation]]:
    records: list[BoundaryRecord] = []
    violations: list[BoundaryViolation] = []
    for doc_path in boundary_docs(repo_root):
        rel_doc = doc_path.relative_to(repo_root)
        for start_line, yaml_text in extract_yaml_blocks(doc_path):
            try:
                data = parse_simple_yaml_document(yaml_text)
            except ValueError as error:
                violations.append(
                    BoundaryViolation(f"{rel_doc.as_posix()}:{start_line}", f"invalid boundary YAML: {error}")
                )
                continue
            record, record_errors = build_boundary_record(
                data=data,
                source_path=rel_doc,
                start_line=start_line,
            )
            violations.extend(record_errors)
            if record is not None:
                records.append(record)
    for toml_path in boundary_toml_files(repo_root):
        rel_toml = toml_path.relative_to(repo_root)
        try:
            data = tomllib_load(toml_path)
        except Exception as error:
            violations.append(
                BoundaryViolation(rel_toml.as_posix(), f"invalid boundary TOML: {error}")
            )
            continue
        if not isinstance(data, dict):
            violations.append(
                BoundaryViolation(rel_toml.as_posix(), "invalid boundary TOML: top-level document must be a table")
            )
            continue
        record, record_errors = build_boundary_record(
            data=data,
            source_path=rel_toml,
            start_line=1,
        )
        violations.extend(record_errors)
        if record is not None:
            records.append(record)
    return records, violations


def manifest_info(repo_root: Path) -> list[ManifestInfo]:
    infos: list[ManifestInfo] = []
    for manifest_path in workspace_manifests(repo_root):
        manifest = tomllib_load(manifest_path)
        package = manifest.get("package", {})
        package_name = package.get("name")
        if not isinstance(package_name, str):
            continue
        lib = manifest.get("lib", {})
        lib_name = lib.get("name") if isinstance(lib, dict) else None
        crate_path_name = lib_name if isinstance(lib_name, str) else manifest_path.parent.name.replace("-", "_")
        infos.append(
            ManifestInfo(
                path=manifest_path,
                package_name=package_name,
                crate_dir_name=manifest_path.parent.name,
                crate_path_name=crate_path_name,
            )
        )
    return infos


def manifest_by_alias(repo_root: Path) -> dict[str, ManifestInfo]:
    aliases: dict[str, ManifestInfo] = {}
    for info in manifest_info(repo_root):
        for alias in info.aliases:
            aliases[alias] = info
    return aliases


def source_files_for_crate(info: ManifestInfo) -> list[Path]:
    crate_root = info.path.parent / "src"
    if not crate_root.exists():
        return []
    return sorted(crate_root.glob("**/*.rs"))


def dedupe_violations(violations: list[BoundaryViolation]) -> list[BoundaryViolation]:
    unique: dict[tuple[str, str], BoundaryViolation] = {}
    for violation in violations:
        unique[(violation.location, violation.message)] = violation
    return sorted(unique.values(), key=lambda item: (item.location, item.message))


def collect_duplicate_record_violations(records: list[BoundaryRecord]) -> list[BoundaryViolation]:
    violations: list[BoundaryViolation] = []
    by_id: dict[str, BoundaryRecord] = {}
    by_owner_name: dict[tuple[str, str], BoundaryRecord] = {}
    for record in records:
        existing = by_id.get(record.boundary_id)
        if existing is not None:
            violations.append(
                BoundaryViolation(
                    record.location,
                    f"duplicate boundary_id {record.boundary_id!r}; first declared at {existing.location}",
                )
            )
        else:
            by_id[record.boundary_id] = record

        key = (record.owner_package, record.name)
        existing_owner_name = by_owner_name.get(key)
        if existing_owner_name is not None:
            violations.append(
                BoundaryViolation(
                    record.location,
                    f"duplicate boundary name {record.name!r} for owner_package {record.owner_package!r}; first declared at {existing_owner_name.location}",
                )
            )
        else:
            by_owner_name[key] = record
    return violations


def collect_manifest_consistency_violations(repo_root: Path, records: list[BoundaryRecord]) -> list[BoundaryViolation]:
    violations: list[BoundaryViolation] = []
    alias_map = manifest_by_alias(repo_root)
    for record in records:
        info = alias_map.get(record.owner_package)
        if info is None:
            continue
        if record.owner_crate_path != info.crate_path_name:
            violations.append(
                BoundaryViolation(
                    record.location,
                    f"owner_crate_path {record.owner_crate_path!r} does not match workspace crate path {info.crate_path_name!r}",
                )
            )
    return violations


def collect_allowed_dependent_violations(repo_root: Path, records: list[BoundaryRecord]) -> list[BoundaryViolation]:
    violations: list[BoundaryViolation] = []
    infos = manifest_info(repo_root)
    workspace_aliases = {alias for info in infos for alias in info.aliases}
    records_by_owner: dict[str, list[BoundaryRecord]] = {}
    for record in records:
        records_by_owner.setdefault(record.owner_package, []).append(record)

    for owner_package, owner_records in records_by_owner.items():
        owner_info = next((info for info in infos if owner_package in info.aliases), None)
        if owner_info is None:
            continue
        allowed = {alias for record in owner_records for alias in record.allowed_dependents}
        live_allowed_aliases: set[str] = set()
        for depender_info in infos:
            if depender_info.path == owner_info.path:
                continue
            manifest = tomllib_load(depender_info.path)
            for section_name, dependencies in dependency_sections(manifest):
                if "dev-dependencies" in section_name:
                    continue
                for dependency_name, dependency in dependencies.items():
                    package_name = dependency_package_name(dependency_name, dependency)
                    if package_name != owner_info.package_name:
                        continue
                    depender_aliases = set(depender_info.aliases)
                    live_allowed_aliases.update(depender_aliases)
                    if depender_aliases.isdisjoint(allowed):
                        rel_manifest = depender_info.path.relative_to(repo_root).as_posix()
                        violations.append(
                            BoundaryViolation(
                                f"{rel_manifest} [{section_name}]",
                                f"{owner_package} allows dependents {sorted(allowed)!r}; found unexpected dependent {depender_info.package_name!r}",
                            )
                        )
        if any(record.is_active for record in owner_records):
            for allowed_alias in sorted(allowed):
                if allowed_alias in live_allowed_aliases:
                    continue
                if allowed_alias not in workspace_aliases:
                    violations.append(
                        BoundaryViolation(
                            owner_records[0].location,
                            f"{owner_package} allows dependent {allowed_alias!r}, but no workspace crate exposes that alias",
                        )
                    )
                    continue
                violations.append(
                    BoundaryViolation(
                        owner_records[0].location,
                        f"{owner_package} allows dependents {sorted(allowed)!r}; stale allowed dependent {allowed_alias!r} has no live Cargo edge",
                    )
                )
    return violations


def collect_manifest_dependency_allowlist_violations(
    repo_root: Path,
    records: list[BoundaryRecord],
) -> list[BoundaryViolation]:
    """Require every active boundary owner to declare all direct Cargo dependencies.

    ``BoundaryRecord.allowed_dependencies`` remains per-seam documentation.  This
    separate crate-manifest policy intentionally includes dependencies from
    normal, dev, build, and target-specific sections so a new test dependency
    cannot silently bypass review.
    """
    allowlists = manifest_dependency_allowlists(repo_root)
    if not allowlists:
        return []

    violations: list[BoundaryViolation] = []
    infos = manifest_info(repo_root)
    alias_map = manifest_by_alias(repo_root)
    infos_by_path = {
        info.path.relative_to(repo_root): info
        for info in infos
    }
    active_owner_paths = {
        alias_map[record.owner_package].path.relative_to(repo_root)
        for record in records
        if record.is_active and record.owner_package in alias_map
    }
    allowlist_by_path: dict[Path, ManifestDependencyAllowlist] = {}
    for allowlist in allowlists:
        existing = allowlist_by_path.get(allowlist.owner_manifest_path)
        if existing is not None:
            violations.append(
                BoundaryViolation(
                    allowlist.owner_manifest_path.as_posix(),
                    "duplicate manifest dependency allowlist",
                )
            )
            continue
        allowlist_by_path[allowlist.owner_manifest_path] = allowlist

    for manifest_path in sorted(active_owner_paths):
        if manifest_path not in allowlist_by_path:
            violations.append(
                BoundaryViolation(
                    manifest_path.as_posix(),
                    "active boundary owner has no manifest dependency allowlist",
                )
            )

    for manifest_path, allowlist in sorted(allowlist_by_path.items()):
        info = infos_by_path.get(manifest_path)
        if info is None:
            violations.append(
                BoundaryViolation(
                    manifest_path.as_posix(),
                    "manifest dependency allowlist does not name a workspace crate manifest",
                )
            )
            continue
        if manifest_path not in active_owner_paths:
            violations.append(
                BoundaryViolation(
                    manifest_path.as_posix(),
                    "manifest dependency allowlist does not belong to an active boundary owner",
                )
            )
            continue

        manifest = tomllib_load(info.path)
        actual_dependencies: set[str] = set()
        for _section_name, dependencies in dependency_sections(manifest):
            for dependency_name, dependency in dependencies.items():
                package_name = dependency_package_name(dependency_name, dependency)
                dependency_info = alias_map.get(package_name) or alias_map.get(dependency_name)
                actual_dependencies.add(
                    dependency_info.crate_dir_name if dependency_info is not None else package_name
                )

        allowed_dependencies = set(allowlist.allowed_dependencies)
        location = f"{manifest_path.as_posix()} [manifest-dependency-allowlist]"
        for dependency in sorted(actual_dependencies - allowed_dependencies):
            violations.append(
                BoundaryViolation(
                    location,
                    f"Cargo dependency {dependency!r} is not allowlisted",
                )
            )
        for dependency in sorted(allowed_dependencies - actual_dependencies):
            violations.append(
                BoundaryViolation(
                    location,
                    f"allowlisted dependency {dependency!r} is not declared by Cargo.toml",
                )
            )
    return violations


def collect_forbidden_edge_violations(repo_root: Path, records: list[BoundaryRecord]) -> list[BoundaryViolation]:
    violations: list[BoundaryViolation] = []
    alias_map = manifest_by_alias(repo_root)
    for record in records:
        for edge in record.forbidden_edges:
            match = FORBIDDEN_EDGE_RE.match(edge)
            if match is None:
                continue
            left_alias = match.group("left")
            right_alias = match.group("right")
            left_manifest = alias_map.get(left_alias)
            if left_manifest is None:
                continue
            manifest = tomllib_load(left_manifest.path)
            rel_manifest = left_manifest.path.relative_to(repo_root).as_posix()
            for section_name, dependencies in dependency_sections(manifest):
                for dependency_name, dependency in dependencies.items():
                    package_name = dependency_package_name(dependency_name, dependency)
                    dependency_aliases = {dependency_name, package_name, package_name.replace("-", "_")}
                    if right_alias in dependency_aliases:
                        violations.append(
                            BoundaryViolation(
                                f"{rel_manifest} [{section_name}]",
                                f"{record.boundary_id} forbids edge {left_alias} -> {right_alias}",
                            )
                        )
    return violations


def compile_reference_pattern(reference: str) -> re.Pattern[str]:
    escaped = re.escape(reference)
    return re.compile(rf"(?<![A-Za-z0-9_]){escaped}(?![A-Za-z0-9_])")


def resolve_module_file(repo_root: Path, module_path: str) -> list[Path]:
    crate_prefix, *segments = module_path.split("::")
    info = manifest_by_alias(repo_root).get(crate_prefix)
    if info is None:
        return []
    src_root = info.path.parent / "src"
    if not src_root.exists():
        return []
    if not segments:
        candidates = [src_root / "lib.rs", src_root / "main.rs"]
    else:
        module_rel = Path(*segments)
        candidates = [src_root / f"{module_rel}.rs", src_root / module_rel / "mod.rs"]
    return [path for path in candidates if path.exists()]


def collect_io_forbidden_source_violations(
    repo_root: Path,
    records: list[BoundaryRecord],
) -> list[BoundaryViolation]:
    """Enforce ``ownership.io_forbidden`` against concrete implementation modules.

    A boundary record may describe a trait-only contract or a retired module;
    those records have no implementation region to inspect.  Concrete active
    records are deliberately checked only in their declared implementation
    module so sibling boundaries in the same crate do not contaminate one
    another.  Unknown tags are reported as policy errors instead of being
    silently ignored, which keeps the mapping table mechanically complete.
    """

    violations: list[BoundaryViolation] = []
    compiled_patterns = {
        tag: tuple(re.compile(pattern, re.IGNORECASE) for pattern in patterns)
        for tag, patterns in IO_FORBIDDEN_SOURCE_PATTERNS.items()
    }
    compiled_exceptions = {
        key: tuple(re.compile(pattern, re.IGNORECASE) for pattern in patterns)
        for key, patterns in IO_FORBIDDEN_SOURCE_EXCEPTIONS.items()
    }
    for record in records:
        for tag in record.io_forbidden:
            patterns = compiled_patterns.get(tag)
            if patterns is None:
                violations.append(
                    BoundaryViolation(
                        record.location,
                        f"{record.boundary_id} io_forbidden tag {tag!r} has no source-pattern mapping",
                    )
                )
                continue
            if not record.is_active or record.implementation_visibility == "trait_only":
                continue
            if record.implementation_module is None:
                continue
            source_paths = resolve_module_file(repo_root, record.implementation_module)
            for source_path in source_paths:
                rel_source = source_path.relative_to(repo_root).as_posix()
                source_lines = source_path.read_text(encoding="utf-8").splitlines()
                test_scope = rust_file_test_scope(source_path, source_lines)
                for line_number, line in enumerate(source_lines, start=1):
                    if is_comment_line(line):
                        continue
                    if tag == "background_work" and test_scope[line_number - 1]:
                        continue
                    if any(
                        pattern.search(line)
                        for pattern in compiled_exceptions.get(
                            (record.boundary_id, tag), ()
                        )
                    ):
                        continue
                    matched_pattern = next(
                        (pattern.pattern for pattern in patterns if pattern.search(line)),
                        None,
                    )
                    if matched_pattern is None:
                        continue
                    violations.append(
                        BoundaryViolation(
                            f"{rel_source}:{line_number}",
                            f"{record.boundary_id} forbids io {tag!r}; matched source pattern {matched_pattern!r}",
                        )
                    )
    return violations


def exempt_reference_files(repo_root: Path, record: BoundaryRecord) -> set[Path]:
    files: set[Path] = set()
    alias_map = manifest_by_alias(repo_root)
    owner_info = alias_map.get(record.owner_package)
    if owner_info is not None:
        files.update(path.resolve() for path in source_files_for_crate(owner_info))
    for root in record.composition_roots:
        files.update(path.resolve() for path in resolve_module_file(repo_root, root))
    return files


def collect_reference_violations(repo_root: Path, records: list[BoundaryRecord]) -> list[BoundaryViolation]:
    violations: list[BoundaryViolation] = []
    workspace_sources = rust_sources(repo_root)
    for record in records:
        if record.references_scope == "inside_owner_crate":
            owner = manifest_by_alias(repo_root).get(record.owner_package)
            source_paths = source_files_for_crate(owner) if owner is not None else []
            exempt_files: set[Path] = set()
            reference_scope = "owner-crate"
        else:
            source_paths = workspace_sources
            exempt_files = (
                exempt_reference_files(repo_root, record)
                if record.references_scope == "outside_owner_crate"
                else set()
            )
            reference_scope = "external" if record.references_scope == "outside_owner_crate" else "global"
        compiled_patterns = [(reference, compile_reference_pattern(reference)) for reference in record.forbidden_references]
        if not compiled_patterns:
            continue
        for source_path in source_paths:
            if source_path.resolve() in exempt_files:
                continue
            rel_source = source_path.relative_to(repo_root).as_posix()
            source_lines = source_path.read_text(encoding="utf-8").splitlines()
            test_scope = rust_file_test_scope(source_path, source_lines)
            for line_number, line in enumerate(source_lines, start=1):
                if is_comment_line(line):
                    continue
                if test_scope[line_number - 1]:
                    continue
                for reference, pattern in compiled_patterns:
                    if pattern.search(line):
                        violations.append(
                            BoundaryViolation(
                                f"{rel_source}:{line_number}",
                                f"{record.boundary_id} forbids {reference_scope} reference {reference!r}",
                            )
                        )
    return violations


def collect_test_bypass_violations(repo_root: Path, records: list[BoundaryRecord]) -> list[BoundaryViolation]:
    violations: list[BoundaryViolation] = []
    alias_map = manifest_by_alias(repo_root)
    for record in records:
        owner_info = alias_map.get(record.owner_package)
        if owner_info is None:
            continue
        test_sources = rust_test_sources_for_crate(owner_info)
        compiled_patterns = [
            (reference, compile_reference_pattern(reference)) for reference in record.forbidden_test_bypasses
        ]
        if not compiled_patterns:
            continue
        for source_path in test_sources:
            rel_source = source_path.relative_to(repo_root).as_posix()
            for line_number, line in enumerate(source_path.read_text(encoding="utf-8").splitlines(), start=1):
                if is_comment_line(line):
                    continue
                for reference, pattern in compiled_patterns:
                    if pattern.search(line):
                        violations.append(
                            BoundaryViolation(
                                f"{rel_source}:{line_number}",
                                f"{record.boundary_id} forbids test bypass reference {reference!r}",
                            )
                        )
    return violations


def find_public_type_violations(record: BoundaryRecord, source_path: Path, repo_root: Path) -> list[BoundaryViolation]:
    if record.implementation_type is None:
        return []
    pattern = re.compile(PUBLIC_TYPE_TEMPLATE.format(name=re.escape(record.implementation_type)))
    rel_source = source_path.relative_to(repo_root).as_posix()
    violations: list[BoundaryViolation] = []
    for line_number, line in enumerate(source_path.read_text(encoding="utf-8").splitlines(), start=1):
        if pattern.search(line):
            violations.append(
                BoundaryViolation(
                    f"{rel_source}:{line_number}",
                    f"{record.boundary_id} requires private implementation.type {record.implementation_type!r}",
                )
            )
    return violations


def find_public_reexport_violations(record: BoundaryRecord, source_path: Path, repo_root: Path) -> list[BoundaryViolation]:
    if record.implementation_type is None:
        return []
    pattern = re.compile(PUBLIC_REEXPORT_TEMPLATE.format(name=re.escape(record.implementation_type)))
    rel_source = source_path.relative_to(repo_root).as_posix()
    violations: list[BoundaryViolation] = []
    for line_number, line in enumerate(source_path.read_text(encoding="utf-8").splitlines(), start=1):
        if pattern.search(line):
            violations.append(
                BoundaryViolation(
                    f"{rel_source}:{line_number}",
                    f"{record.boundary_id} forbids public re-export of {record.implementation_type!r}",
                )
            )
    return violations


def find_public_constructor_violations(record: BoundaryRecord, source_path: Path, repo_root: Path) -> list[BoundaryViolation]:
    if record.implementation_type is None:
        return []
    lines = source_path.read_text(encoding="utf-8").splitlines()
    rel_source = source_path.relative_to(repo_root).as_posix()
    violations: list[BoundaryViolation] = []
    inside_impl = False
    brace_depth = 0
    impl_pattern = re.compile(rf"\bimpl(?:<[^>]+>)?(?:\s+[A-Za-z0-9_:<>, ]+)?\s+{re.escape(record.implementation_type)}\b")
    for line_number, line in enumerate(lines, start=1):
        if not inside_impl and impl_pattern.search(line):
            inside_impl = True
            brace_depth = line.count("{") - line.count("}")
        elif inside_impl:
            brace_depth += line.count("{") - line.count("}")
            if PUBLIC_FUNCTION_RE.search(line):
                violations.append(
                    BoundaryViolation(
                        f"{rel_source}:{line_number}",
                        f"{record.boundary_id} forbids public constructor/helper methods on {record.implementation_type!r}",
                    )
                )
            if brace_depth <= 0:
                inside_impl = False
                brace_depth = 0
    return violations


def source_module_path(info: ManifestInfo, source_path: Path) -> str:
    """Return the crate-qualified module path represented by a Rust source file."""

    relative = source_path.relative_to(info.path.parent / "src")
    parts = list(relative.with_suffix("").parts)
    if parts and parts[-1] in {"lib", "main", "mod"}:
        parts.pop()
    return "::".join(("crate", *parts))


def find_trait_only_test_double_violations(
    record: BoundaryRecord,
    owner_info: ManifestInfo,
    source_path: Path,
    repo_root: Path,
) -> list[BoundaryViolation]:
    """Require every owner-crate trait-only implementation to be an approved double.

    A trait-only boundary that declares approved test doubles must name every
    owner-crate implementation in ``testing.allowed_test_double_paths``. Keeping
    this check in the generic boundary linter prevents a second ad-hoc test
    emitter from silently bypassing the boundary manifest. Empty allowlists keep
    legacy trait-only records observational until their test-double policy is
    explicitly declared.
    """

    if record.public_trait is None:
        return []
    trait_pattern = re.compile(
        rf"\bimpl(?:<[^>]+>)?\s+(?:[A-Za-z_][A-Za-z0-9_:]*::)?"
        rf"{re.escape(record.public_trait)}(?:<[^>]+>)?\s+for\s+([A-Za-z_][A-Za-z0-9_]*)"
    )
    module_path = source_module_path(owner_info, source_path)
    allowed_paths = set(record.allowed_test_double_paths)
    violations: list[BoundaryViolation] = []
    rel_source = source_path.relative_to(repo_root).as_posix()
    for line_number, line in enumerate(source_path.read_text(encoding="utf-8").splitlines(), start=1):
        if is_comment_line(line):
            continue
        match = trait_pattern.search(line)
        if match is None:
            continue
        implementation_path = f"{module_path}::{match.group(1)}"
        if implementation_path in allowed_paths:
            continue
        violations.append(
            BoundaryViolation(
                f"{rel_source}:{line_number}",
                f"{record.boundary_id} trait-only implementation {implementation_path!r} "
                "is not listed in testing.allowed_test_double_paths",
            )
        )
    return violations


def collect_active_implementation_violations(repo_root: Path, records: list[BoundaryRecord]) -> list[BoundaryViolation]:
    violations: list[BoundaryViolation] = []
    alias_map = manifest_by_alias(repo_root)
    for record in records:
        if not record.is_active:
            continue
        owner_info = alias_map.get(record.owner_package)
        if owner_info is None:
            continue
        source_files = source_files_for_crate(owner_info)
        for source_path in source_files:
            if record.implementation_visibility == "trait_only" and record.allowed_test_double_paths:
                violations.extend(
                    find_trait_only_test_double_violations(record, owner_info, source_path, repo_root)
                )
                continue
            if record.implementation_visibility == "private":
                violations.extend(find_public_type_violations(record, source_path, repo_root))
                violations.extend(find_public_reexport_violations(record, source_path, repo_root))
            if record.implementation_constructor == "private":
                violations.extend(find_public_constructor_violations(record, source_path, repo_root))
    return violations


def collect_special_case_violations(repo_root: Path) -> list[BoundaryViolation]:
    violations: list[BoundaryViolation] = []
    ownership_rules = dependency_ownership_rules(repo_root)
    section_rules = manifest_section_rules(repo_root)

    for manifest_path in workspace_manifests(repo_root):
        manifest = tomllib_load(manifest_path)
        rel_manifest_path = manifest_path.relative_to(repo_root)
        rel_manifest = rel_manifest_path.as_posix()
        for section_name, dependencies in dependency_sections(manifest):
            for dependency_name, dependency in dependencies.items():
                package_name = dependency_package_name(dependency_name, dependency)
                for rule in ownership_rules:
                    if package_name == rule.dependency and rel_manifest_path not in rule.allowed_manifest_paths:
                        violations.append(
                            BoundaryViolation(
                                f"{rel_manifest} [{section_name}]",
                                rule.manifest_message,
                            )
                        )
                for rule in section_rules:
                    if (
                        rel_manifest_path == rule.owner_manifest_path
                        and package_name == rule.dependency_package
                        and section_name not in rule.allowed_sections
                    ):
                        violations.append(
                            BoundaryViolation(
                                f"{rel_manifest} [{section_name}]",
                                rule.message,
                            )
                        )

    for source_path in rust_sources(repo_root):
        rel_source = source_path.relative_to(repo_root).as_posix()
        text = source_path.read_text(encoding="utf-8")
        for line_number, line in enumerate(text.splitlines(), start=1):
            if is_comment_line(line):
                continue
            for rule in ownership_rules:
                if not any(pattern.search(line) for pattern in dependency_import_patterns(rule.dependency)):
                    continue
                if any((repo_root / allowed_root).resolve() in source_path.resolve().parents for allowed_root in rule.allowed_source_roots):
                    continue
                violations.append(
                    BoundaryViolation(
                        f"{rel_source}:{line_number}",
                        rule.source_message,
                    )
                )
    return violations


def collect_scb_config_rule_violations(
    repo_root: Path,
    source_paths: list[Path],
) -> list[BoundaryViolation]:
    violations: list[BoundaryViolation] = []
    allowlist = scb_config_allowlist(repo_root)

    for source_path in source_paths:
        rel_path = source_path.relative_to(repo_root)
        rel_source = rel_path.as_posix()
        lines = source_path.read_text(encoding="utf-8").splitlines()
        is_send_path = "crates/atm-core/src/send/" in rel_source or rel_path == SCB_CONFIG_FIXTURE_PATH
        is_boundary_file = rel_path in SCB_CONFIG_BOUNDARY_FILES or rel_path == SCB_CONFIG_FIXTURE_PATH

        for line_number, line in enumerate(lines, start=1):
            if is_comment_line(line):
                continue
            symbol = enclosing_function_name(lines, line_number)
            stripped = line.strip()

            if any(pattern in stripped for pattern in SCB_CONFIG_DIRECT_PATTERNS):
                if not is_allowlisted_config_violation(
                    entries=allowlist,
                    rule="SCB-CONFIG-001",
                    rel_path=rel_path,
                    symbol=symbol,
                ):
                    violations.append(
                        BoundaryViolation(
                            f"SCB-CONFIG-001 {rel_source}:{line_number} direct config.json roster read outside the explicit allowlist",
                            "",
                        )
                    )

            if (
                is_boundary_file
                and rel_path != SCB_CONFIG_CANONICAL_HELPER_FILE
                and any(pattern in stripped for pattern in SCB_CONFIG_GENERIC_HELPER_PATTERNS)
            ):
                violations.append(
                    BoundaryViolation(
                        f"SCB-CONFIG-002 {rel_source}:{line_number} generic load_workspace_config helper surface is forbidden",
                        "",
                    )
                )

            if is_send_path and any(pattern in stripped for pattern in SCB_CONFIG_SEND_PATTERNS):
                violations.append(
                    BoundaryViolation(
                        f"SCB-CONFIG-003 {rel_source}:{line_number} Claude send path must not consult config.json before durable ATM write completion",
                        "",
                    )
                )

    return violations


def collect_scb_retained_rule_violations(
    repo_root: Path,
    source_paths: list[Path],
) -> list[BoundaryViolation]:
    violations: list[BoundaryViolation] = []
    allowlist = scb_retained_allowlist(repo_root)

    for source_path in source_paths:
        rel_path = source_path.relative_to(repo_root)
        if rel_path not in SCB_RETAINED_TARGET_FILES and rel_path != SCB_RETAINED_FIXTURE_PATH:
            continue
        rel_source = rel_path.as_posix()
        lines = source_path.read_text(encoding="utf-8").splitlines()

        for line_number, line in enumerate(lines, start=1):
            if is_comment_line(line):
                continue
            stripped = line.strip()
            if not any(pattern in stripped for pattern in SCB_RETAINED_DIRECT_PATTERNS):
                continue
            symbol = enclosing_function_name(lines, line_number)
            if is_allowlisted_retained_violation(
                entries=allowlist,
                rule="SCB-RETAINED-001",
                rel_path=rel_path,
                symbol=symbol,
            ):
                continue
            violations.append(
                BoundaryViolation(
                    f"SCB-RETAINED-001 {rel_source}:{line_number} direct retained-runtime acquisition is forbidden outside the approved roster-store seam",
                    "",
                )
            )

    return violations


def collect_scb_workspace_rule_violations(
    repo_root: Path,
    source_paths: list[Path],
) -> list[BoundaryViolation]:
    violations: list[BoundaryViolation] = []
    allowlist = scb_workspace_allowlist(repo_root)

    for source_path in source_paths:
        rel_path = source_path.relative_to(repo_root)
        if rel_path not in SCB_WORKSPACE_TARGET_FILES and rel_path != SCB_WORKSPACE_FIXTURE_PATH:
            continue
        rel_source = rel_path.as_posix()
        lines = source_path.read_text(encoding="utf-8").splitlines()

        for line_number, line in enumerate(lines, start=1):
            if is_comment_line(line):
                continue
            stripped = line.strip()
            if not any(pattern in stripped for pattern in SCB_WORKSPACE_DIRECT_PATTERNS):
                continue
            symbol = enclosing_function_name(lines, line_number)
            if is_allowlisted_workspace_violation(
                entries=allowlist,
                rule="SCB-WORKSPACE-001",
                rel_path=rel_path,
                symbol=symbol,
            ):
                continue
            violations.append(
                BoundaryViolation(
                    f"SCB-WORKSPACE-001 {rel_source}:{line_number} direct workspace-config lookup is forbidden outside the approved ConfigIngress seam",
                    "",
                )
            )

    return violations


def collect_scb_singleton_rule_violations(
    repo_root: Path,
    source_paths: list[Path],
) -> list[BoundaryViolation]:
    violations: list[BoundaryViolation] = []
    allowlist = scb_singleton_allowlist(repo_root)

    for source_path in source_paths:
        rel_path = source_path.relative_to(repo_root)
        if rel_path not in SCB_SINGLETON_TARGET_FILES and rel_path != SCB_SINGLETON_FIXTURE_PATH:
            continue
        rel_source = rel_path.as_posix()
        lines = source_path.read_text(encoding="utf-8").splitlines()

        for line_number, line in enumerate(lines, start=1):
            if is_comment_line(line):
                continue
            stripped = line.strip()
            symbol = enclosing_function_name(lines, line_number)

            if any(pattern in stripped for pattern in SCB_SINGLETON_ROOT_FORBIDDEN_PATTERNS):
                if is_allowlisted_singleton_violation(
                    entries=allowlist,
                    rule="SCB-SINGLETON-001",
                    rel_path=rel_path,
                    symbol=symbol,
                ):
                    continue
                violations.append(
                    BoundaryViolation(
                        f"SCB-SINGLETON-001 {rel_source}:{line_number} public ambient runtime-install surface is forbidden; use approved bounded wrappers only",
                        "",
                    )
                )
                continue

            if any(pattern in stripped for pattern in SCB_SINGLETON_HIDDEN_HOOK_PATTERNS):
                if rel_path in SCB_SINGLETON_ALLOWED_HOOK_CALLERS:
                    continue
                if is_allowlisted_singleton_violation(
                    entries=allowlist,
                    rule="SCB-SINGLETON-001",
                    rel_path=rel_path,
                    symbol=symbol,
                ):
                    continue
                violations.append(
                    BoundaryViolation(
                        f"SCB-SINGLETON-001 {rel_source}:{line_number} hidden runtime-install hooks may only be called from approved bounded wrappers",
                        "",
                    )
                )

    return violations


def collect_scb_observability_rule_violations(
    repo_root: Path,
    source_paths: list[Path],
) -> list[BoundaryViolation]:
    violations: list[BoundaryViolation] = []
    allowlist = scb_observability_allowlist(repo_root)

    for source_path in source_paths:
        rel_path = source_path.relative_to(repo_root)
        rel_source = rel_path.as_posix()
        if (
            rel_path != SCB_OBSERVABILITY_FIXTURE_PATH
            and not rel_source.startswith("crates/atm-daemon/src/")
        ):
            continue
        if rel_path in SCB_OBSERVABILITY_ALLOWED_SRC_FILES:
            continue
        lines = source_path.read_text(encoding="utf-8").splitlines()
        for line_number, line in enumerate(lines, start=1):
            if is_comment_line(line):
                continue
            stripped = line.strip()
            if not any(pattern in stripped for pattern in SCB_OBSERVABILITY_DIRECT_PATTERNS):
                continue
            symbol = enclosing_function_name(lines, line_number) or "__module__"
            if is_allowlisted_observability_violation(
                entries=allowlist,
                rule="SCB-OBSERVABILITY-001",
                rel_path=rel_path,
                symbol=symbol,
            ):
                continue
            violations.append(
                BoundaryViolation(
                    f"SCB-OBSERVABILITY-001 {rel_source}:{line_number} direct sc_observability_types ActionName/OutcomeLabel imports are forbidden outside daemon_runtime_observability.rs and main.rs",
                    "",
                )
            )

    return violations


def collect_boundary_violations(repo_root: Path) -> list[BoundaryViolation]:
    records, parse_violations = parse_boundary_records(repo_root)
    violations: list[BoundaryViolation] = []
    violations.extend(parse_violations)
    violations.extend(collect_duplicate_record_violations(records))
    violations.extend(collect_manifest_consistency_violations(repo_root, records))
    violations.extend(collect_allowed_dependent_violations(repo_root, records))
    violations.extend(collect_manifest_dependency_allowlist_violations(repo_root, records))
    violations.extend(collect_forbidden_edge_violations(repo_root, records))
    violations.extend(collect_reference_violations(repo_root, records))
    violations.extend(collect_test_bypass_violations(repo_root, records))
    violations.extend(collect_active_implementation_violations(repo_root, records))
    violations.extend(collect_io_forbidden_source_violations(repo_root, records))
    violations.extend(collect_special_case_violations(repo_root))
    violations.extend(collect_scb_config_rule_violations(repo_root, rust_sources(repo_root)))
    violations.extend(collect_scb_retained_rule_violations(repo_root, rust_sources(repo_root)))
    violations.extend(collect_scb_workspace_rule_violations(repo_root, rust_sources(repo_root)))
    violations.extend(collect_scb_singleton_rule_violations(repo_root, rust_sources(repo_root)))
    violations.extend(collect_scb_observability_rule_violations(repo_root, rust_sources(repo_root)))
    fixture_path = repo_root / SCB_CONFIG_FIXTURE_PATH
    if not fixture_path.exists():
        violations.append(
            BoundaryViolation(
                SCB_CONFIG_FIXTURE_PATH.as_posix(),
                "missing required SCB-CONFIG known-bad fixture",
            )
        )
    else:
        fixture_violations = collect_scb_config_rule_violations(repo_root, [fixture_path])
        fixture_failure = scb_config_fixture_violation(
            fixture_violations,
            {"SCB-CONFIG-001", "SCB-CONFIG-002", "SCB-CONFIG-003"},
        )
        if fixture_failure is not None:
            violations.append(fixture_failure)
    retained_fixture_path = repo_root / SCB_RETAINED_FIXTURE_PATH
    if not retained_fixture_path.exists():
        violations.append(
            BoundaryViolation(
                SCB_RETAINED_FIXTURE_PATH.as_posix(),
                "missing required SCB-RETAINED known-bad fixture",
            )
        )
    else:
        fixture_violations = collect_scb_retained_rule_violations(repo_root, [retained_fixture_path])
        fixture_failure = scb_retained_fixture_violation(
            fixture_violations,
            {"SCB-RETAINED-001"},
        )
        if fixture_failure is not None:
            violations.append(fixture_failure)
    workspace_fixture_path = repo_root / SCB_WORKSPACE_FIXTURE_PATH
    if not workspace_fixture_path.exists():
        violations.append(
            BoundaryViolation(
                SCB_WORKSPACE_FIXTURE_PATH.as_posix(),
                "missing required SCB-WORKSPACE known-bad fixture",
            )
        )
    else:
        fixture_violations = collect_scb_workspace_rule_violations(repo_root, [workspace_fixture_path])
        fixture_failure = scb_workspace_fixture_violation(
            fixture_violations,
            {"SCB-WORKSPACE-001"},
        )
        if fixture_failure is not None:
            violations.append(fixture_failure)
    singleton_fixture_path = repo_root / SCB_SINGLETON_FIXTURE_PATH
    if not singleton_fixture_path.exists():
        violations.append(
            BoundaryViolation(
                SCB_SINGLETON_FIXTURE_PATH.as_posix(),
                "missing required SCB-SINGLETON known-bad fixture",
            )
        )
    else:
        fixture_violations = collect_scb_singleton_rule_violations(repo_root, [singleton_fixture_path])
        fixture_failure = scb_singleton_fixture_violation(
            fixture_violations,
            {"SCB-SINGLETON-001"},
        )
        if fixture_failure is not None:
            violations.append(fixture_failure)
    observability_fixture_path = repo_root / SCB_OBSERVABILITY_FIXTURE_PATH
    if not observability_fixture_path.exists():
        violations.append(
            BoundaryViolation(
                SCB_OBSERVABILITY_FIXTURE_PATH.as_posix(),
                "missing required SCB-OBSERVABILITY known-bad fixture",
            )
        )
    else:
        fixture_violations = collect_scb_observability_rule_violations(
            repo_root, [observability_fixture_path]
        )
        fixture_failure = scb_observability_fixture_violation(
            fixture_violations,
            {"SCB-OBSERVABILITY-001"},
        )
        if fixture_failure is not None:
            violations.append(fixture_failure)
    return dedupe_violations(violations)


def build_summary(violations: list[BoundaryViolation], record_count: int) -> str:
    if not violations:
        return f"boundary rules satisfied ({record_count} records)"
    return f"boundary rules violated ({len(violations)} findings across {record_count} records)"


def boundary_doc_section_lines(repo_root: Path, records: list[BoundaryRecord]) -> list[str]:
    record_counts_by_doc: dict[str, dict[str, int]] = {}
    for doc_path in boundary_docs(repo_root):
        record_counts_by_doc[doc_path.relative_to(repo_root).as_posix()] = {
            "records": 0,
            "active": 0,
            "planned": 0,
            "deferred": 0,
            "retired": 0,
        }
    for toml_path in boundary_toml_files(repo_root):
        record_counts_by_doc[toml_path.relative_to(repo_root).as_posix()] = {
            "records": 0,
            "active": 0,
            "planned": 0,
            "deferred": 0,
            "retired": 0,
        }

    for record in records:
        if record.source_path.is_absolute():
            doc_key = record.source_path.relative_to(repo_root).as_posix()
        else:
            doc_key = record.source_path.as_posix()
        counts = record_counts_by_doc.setdefault(
            doc_key,
            {"records": 0, "active": 0, "planned": 0, "deferred": 0, "retired": 0},
        )
        counts["records"] += 1
        if record.status_state == "unix_implemented_windows_pending":
            counts["active"] += 1
        elif record.status_state in counts:
            counts[record.status_state] += 1

    rows: list[dict[str, str]] = []
    for doc_display in sorted(record_counts_by_doc):
        counts = record_counts_by_doc[doc_display]
        rows.append(
            {
                "doc": doc_display,
                "records": str(counts["records"]),
                "active": str(counts["active"]),
                "planned": str(counts["planned"]),
                "deferred": str(counts["deferred"]),
                "retired": str(counts["retired"]),
            }
        )

    lines = ["boundary docs analyzed:"]
    lines.extend(
        render_table(
            rows,
            [
                ("doc", "doc"),
                ("records", "records"),
                ("active", "active"),
                ("planned", "planned"),
                ("deferred", "deferred"),
                ("retired", "retired"),
            ],
        )
    )
    lines.append("")
    lines.append(f"boundary doc count: {len(rows)}")
    lines.append(f"boundary records validated: {len(records)}")
    lines.append("")
    return lines


def run(repo_root: Path) -> int:
    started_at = datetime.now(timezone.utc)
    started_monotonic = monotonic_now()
    records, parse_violations = parse_boundary_records(repo_root)
    violations = parse_violations.copy()
    violations.extend(collect_duplicate_record_violations(records))
    violations.extend(collect_manifest_consistency_violations(repo_root, records))
    violations.extend(collect_allowed_dependent_violations(repo_root, records))
    violations.extend(collect_manifest_dependency_allowlist_violations(repo_root, records))
    violations.extend(collect_forbidden_edge_violations(repo_root, records))
    violations.extend(collect_reference_violations(repo_root, records))
    violations.extend(collect_test_bypass_violations(repo_root, records))
    violations.extend(collect_active_implementation_violations(repo_root, records))
    violations.extend(collect_io_forbidden_source_violations(repo_root, records))
    violations.extend(collect_special_case_violations(repo_root))
    violations.extend(collect_scb_config_rule_violations(repo_root, rust_sources(repo_root)))
    violations.extend(collect_scb_retained_rule_violations(repo_root, rust_sources(repo_root)))
    violations.extend(collect_scb_workspace_rule_violations(repo_root, rust_sources(repo_root)))
    violations.extend(collect_scb_singleton_rule_violations(repo_root, rust_sources(repo_root)))
    fixture_path = repo_root / SCB_CONFIG_FIXTURE_PATH
    if not fixture_path.exists():
        violations.append(
            BoundaryViolation(
                SCB_CONFIG_FIXTURE_PATH.as_posix(),
                "missing required SCB-CONFIG known-bad fixture",
            )
        )
    else:
        fixture_violations = collect_scb_config_rule_violations(repo_root, [fixture_path])
        fixture_failure = scb_config_fixture_violation(
            fixture_violations,
            {"SCB-CONFIG-001", "SCB-CONFIG-002", "SCB-CONFIG-003"},
        )
        if fixture_failure is not None:
            violations.append(fixture_failure)
    retained_fixture_path = repo_root / SCB_RETAINED_FIXTURE_PATH
    if not retained_fixture_path.exists():
        violations.append(
            BoundaryViolation(
                SCB_RETAINED_FIXTURE_PATH.as_posix(),
                "missing required SCB-RETAINED known-bad fixture",
            )
        )
    else:
        fixture_violations = collect_scb_retained_rule_violations(repo_root, [retained_fixture_path])
        fixture_failure = scb_retained_fixture_violation(
            fixture_violations,
            {"SCB-RETAINED-001"},
        )
        if fixture_failure is not None:
            violations.append(fixture_failure)
    workspace_fixture_path = repo_root / SCB_WORKSPACE_FIXTURE_PATH
    if not workspace_fixture_path.exists():
        violations.append(
            BoundaryViolation(
                SCB_WORKSPACE_FIXTURE_PATH.as_posix(),
                "missing required SCB-WORKSPACE known-bad fixture",
            )
        )
    else:
        fixture_violations = collect_scb_workspace_rule_violations(repo_root, [workspace_fixture_path])
        fixture_failure = scb_workspace_fixture_violation(
            fixture_violations,
            {"SCB-WORKSPACE-001"},
        )
        if fixture_failure is not None:
            violations.append(fixture_failure)
    singleton_fixture_path = repo_root / SCB_SINGLETON_FIXTURE_PATH
    if not singleton_fixture_path.exists():
        violations.append(
            BoundaryViolation(
                SCB_SINGLETON_FIXTURE_PATH.as_posix(),
                "missing required SCB-SINGLETON known-bad fixture",
            )
        )
    else:
        fixture_violations = collect_scb_singleton_rule_violations(repo_root, [singleton_fixture_path])
        fixture_failure = scb_singleton_fixture_violation(
            fixture_violations,
            {"SCB-SINGLETON-001"},
        )
        if fixture_failure is not None:
            violations.append(fixture_failure)
    violations = dedupe_violations(violations)

    duration_seconds = monotonic_now() - started_monotonic
    findings = [violation.render() for violation in violations]
    transcript_lines = workspace_crate_section_lines(repo_root)
    transcript_lines.extend(boundary_doc_section_lines(repo_root, records))
    transcript_lines.extend(findings or ["no boundary violations found"])
    report = build_report(
        lint_name=LINT_NAME,
        repo_root=repo_root,
        passed=not violations,
        summary=build_summary(violations, len(records)),
        findings=findings,
        transcript_lines=transcript_lines,
        started_at=started_at,
        duration_seconds=duration_seconds,
    )
    print_report(report, repo_root=repo_root, preview_limit=4, direct_threshold=4)
    return 0 if report.passed else 1


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Check boundary inventory schema and enforcement rules.")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)
    return run(repo_root)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
