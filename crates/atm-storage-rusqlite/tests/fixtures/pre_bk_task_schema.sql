
CREATE TABLE IF NOT EXISTS tasks (
    team TEXT NOT NULL,
    task_id TEXT NOT NULL,
    assignee TEXT NOT NULL,
    assigner TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('assigned', 'active', 'complete')),
    close_outcome TEXT NULL CHECK(close_outcome IN ('completed', 'refused', 'cancelled')),
    position INTEGER NULL CHECK(position IS NULL OR position >= 1),
    assignment_message_id TEXT NOT NULL,
    description TEXT NOT NULL,
    assigned_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_reminded_at TEXT NULL,
    reminder_count INTEGER NOT NULL DEFAULT 0,
    lead_notified_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (team, task_id),
    CHECK ((state = 'complete') = (close_outcome IS NOT NULL)),
    CHECK ((state = 'complete') = (position IS NULL))
);

CREATE TABLE IF NOT EXISTS task_events (
    team TEXT NOT NULL,
    task_id TEXT NOT NULL,
    assignee TEXT NOT NULL,
    seq INTEGER NOT NULL,
    at TEXT NOT NULL,
    event TEXT NOT NULL,
    from_state TEXT NULL,
    to_state TEXT NULL,
    close_outcome TEXT NULL CHECK(close_outcome IN ('completed', 'refused', 'cancelled')),
    actor TEXT NOT NULL,
    message_id TEXT NULL,
    outcome TEXT NULL CHECK(outcome IN ('emitted', 'unrenderable', 'blocked')),
    marker TEXT NULL CHECK(marker IN ('resend', 'assignment_missing')),
    detail TEXT NULL,
    PRIMARY KEY (team, task_id, seq)
);

CREATE TABLE IF NOT EXISTS prompt_handoffs (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key TEXT NOT NULL,
    kind TEXT NOT NULL,
    task_id TEXT NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 0 CHECK(attempt >= 0),
    trigger TEXT NOT NULL CHECK(trigger IN ('steer', 'task_pass')),
    at TEXT NOT NULL,
    UNIQUE (team, agent, message_key, kind, attempt)
);
CREATE INDEX IF NOT EXISTS prompt_handoffs_task ON prompt_handoffs(team, task_id, at);

CREATE UNIQUE INDEX IF NOT EXISTS one_active_task_per_agent
    ON tasks(team, assignee) WHERE state = 'active';

CREATE UNIQUE INDEX IF NOT EXISTS tasks_position_per_member
    ON tasks(team, assignee, position) WHERE state <> 'complete';

CREATE INDEX IF NOT EXISTS tasks_open_by_member
    ON tasks(team, assignee, assigned_at) WHERE state <> 'complete';

CREATE TABLE IF NOT EXISTS escalation_recipients (
    scope_key TEXT NOT NULL,
    address TEXT NOT NULL,
    added_at TEXT NOT NULL,
    PRIMARY KEY (scope_key, address)
);
