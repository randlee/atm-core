CREATE TABLE IF NOT EXISTS mail_messages (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key TEXT NOT NULL,
    envelope_json TEXT NOT NULL,
    from_agent TEXT NOT NULL,
    source_chat_id TEXT NULL,
    destination_chat_id TEXT NULL,
    message_text TEXT NULL,
    template_sha TEXT NULL,
    vars_json TEXT NULL,
    category TEXT NULL,
    content_format TEXT NULL,
    tags_json TEXT NOT NULL DEFAULT '[]',
    summary TEXT NULL,
    message_at TEXT NOT NULL,
    message_id TEXT NULL,
    parent_message_id TEXT NULL,
    thread_mode TEXT NULL CHECK(thread_mode IS NULL OR thread_mode IN ('add-details', 'supersede')),
    recorded_at TEXT,
    workflow_scope_kind TEXT NULL,
    workflow_scope_id TEXT NULL,
    workflow_state TEXT NULL,
    workflow_stage TEXT NULL,
    workflow_transition TEXT NULL,
    workflow_iteration TEXT NULL,
    applied_template_tags_json TEXT NULL,
    effective_tags_json TEXT NULL,
    CHECK(message_key GLOB 'atm:*' OR message_key GLOB 'ext:*'),
    PRIMARY KEY (team, agent, message_key)
);

CREATE TABLE IF NOT EXISTS mail_message_states (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key TEXT NOT NULL,
    read INTEGER NOT NULL DEFAULT 0 CHECK(read IN (0, 1)),
    pending_ack_at TEXT NULL,
    acknowledged_at TEXT NULL,
    expires_at TEXT NULL,
    deleted_at TEXT NULL,
    updated_at TEXT NULL,
    nudge_pending_at TEXT NULL,
    nudge_attempts INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (team, agent, message_key),
    FOREIGN KEY (team, agent, message_key)
        REFERENCES mail_messages(team, agent, message_key)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS mail_seen_watermarks (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    watermark TEXT NOT NULL,
    PRIMARY KEY (team, agent)
);

CREATE TABLE IF NOT EXISTS team_roster (
    team_name TEXT NOT NULL,
    agent_name TEXT NOT NULL,
    member_kind TEXT NOT NULL CHECK(member_kind IN ('permanent', 'ephemeral')),
    harness TEXT NOT NULL CHECK(harness IN ('claude-code', 'codex-cli', 'gemini-cli', 'opencode', 'hermes', 'python-graft')),
    agent_type TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL DEFAULT '',
    metadata_json TEXT NOT NULL DEFAULT '{}',
    source TEXT,
    recipient_pane_id TEXT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (team_name, agent_name)
);

CREATE TABLE IF NOT EXISTS team_nudge_template_overrides (
    team_name TEXT NOT NULL,
    template_kind TEXT NOT NULL,
    mode TEXT NOT NULL DEFAULT 'override'
        CHECK(mode IN ('override', 'disabled')),
    template_body TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (team_name, template_kind)
);

CREATE TABLE IF NOT EXISTS peer_https_interfaces (
    bind_addr TEXT NOT NULL PRIMARY KEY,
    advertise_host TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK(enabled IN (0, 1))
);

CREATE TABLE IF NOT EXISTS peer_local_certificate (
    singleton INTEGER NOT NULL PRIMARY KEY CHECK(singleton = 1),
    fingerprint TEXT NOT NULL,
    private_key_ref TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS peer_trusted_peers (
    host TEXT NOT NULL PRIMARY KEY,
    fingerprint TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK(enabled IN (0, 1)),
    https_port INTEGER NOT NULL DEFAULT 43101 CHECK(https_port BETWEEN 1 AND 65535)
);

CREATE INDEX IF NOT EXISTS idx_mail_message_states_mailbox
    ON mail_message_states(team, agent);

CREATE INDEX IF NOT EXISTS idx_team_roster_team_name
    ON team_roster(team_name);

CREATE INDEX IF NOT EXISTS idx_team_nudge_template_overrides_team_name
    ON team_nudge_template_overrides(team_name);

CREATE INDEX IF NOT EXISTS idx_peer_https_interfaces_enabled
    ON peer_https_interfaces(enabled);

CREATE INDEX IF NOT EXISTS idx_peer_trusted_peers_enabled
    ON peer_trusted_peers(enabled);

CREATE TABLE IF NOT EXISTS diagnostic_events (
    id INTEGER PRIMARY KEY,
    ts_unix_ms INTEGER NOT NULL,
    level TEXT NOT NULL,
    component TEXT NOT NULL,
    code TEXT NULL,
    correlation_id TEXT NULL,
    origin TEXT NOT NULL,
    message TEXT NOT NULL,
    detail TEXT NULL
);
CREATE INDEX IF NOT EXISTS idx_diagnostic_events_ts_unix_ms
    ON diagnostic_events(ts_unix_ms);
CREATE INDEX IF NOT EXISTS idx_diagnostic_events_component_ts_unix_ms
    ON diagnostic_events(component, ts_unix_ms);

-- AK.2 retires the worker-only reconciliation policy.  `IF EXISTS` makes the
-- migration safe for both historical databases and fresh installs.
DROP TABLE IF EXISTS peer_sync_policies;

CREATE UNIQUE INDEX IF NOT EXISTS uq_mail_messages_single_successor
    ON mail_messages(team, agent, parent_message_id)
    WHERE parent_message_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS uq_mail_messages_message_id
    ON mail_messages(team, agent, message_id)
    WHERE message_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_mail_messages_mailbox
    ON mail_messages(team, agent);

-- Post-commit received-hook dispatch reloads an admitted record by its
-- immutable message key.  The mailbox primary key begins with team and agent,
-- so it cannot serve that global-key lookup without scanning a growing table.
CREATE INDEX IF NOT EXISTS idx_mail_messages_message_key
    ON mail_messages(message_key);
