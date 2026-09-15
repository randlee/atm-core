use atm_storage::SearchMetadataMatch;
use rusqlite::types::Value as SqlValue;

#[derive(Debug, Clone)]
pub(crate) struct SqlFilters {
    pub(crate) clause: String,
    pub(crate) parameters: Vec<SqlValue>,
}

pub(crate) fn compile_sql_filters(filters: &atm_storage::SearchFilters) -> SqlFilters {
    compile_sql_filters_for(filters, SqlFilterAliases::SEARCH)
}

/// Search has a document projection (`d`) plus its canonical message row
/// (`m`). Mailbox reads and counts deliberately use canonical `mail_messages`.
#[derive(Clone, Copy)]
pub(crate) struct SqlFilterAliases {
    document: &'static str,
    message: &'static str,
    state: &'static str,
}

impl SqlFilterAliases {
    const SEARCH: Self = Self {
        document: "d",
        message: "m",
        state: "ms",
    };

    pub(crate) const MAILBOX: Self = Self {
        document: "m",
        message: "m",
        state: "ms",
    };
}

pub(crate) fn compile_sql_filters_for(
    filters: &atm_storage::SearchFilters,
    aliases: SqlFilterAliases,
) -> SqlFilters {
    let mut clauses = Vec::new();
    let mut parameters = Vec::new();
    push_scalar_filters(filters, aliases, &mut clauses, &mut parameters);
    push_time_filters(filters, aliases, &mut clauses, &mut parameters);
    push_tag_filters(filters, aliases, &mut clauses, &mut parameters);
    push_json_filters(filters, aliases, &mut clauses, &mut parameters);
    push_message_state_filters(filters, aliases, &mut clauses);
    SqlFilters {
        clause: clauses
            .into_iter()
            .map(|clause| format!(" AND {clause}"))
            .collect(),
        parameters,
    }
}

fn push_message_state_filters(
    filters: &atm_storage::SearchFilters,
    aliases: SqlFilterAliases,
    clauses: &mut Vec<String>,
) {
    let state = aliases.state;
    let message = aliases.message;
    let read =
        format!("COALESCE({state}.read, json_extract({message}.envelope_json, '$.read'), 0)");
    let pending_ack_at = format!(
        "COALESCE({state}.pending_ack_at, json_extract({message}.envelope_json, '$.pendingAckAt'))"
    );
    let acknowledged_at = format!(
        "COALESCE({state}.acknowledged_at, json_extract({message}.envelope_json, '$.acknowledgedAt'))"
    );
    let expires_at = format!(
        "COALESCE({state}.expires_at, json_extract({message}.envelope_json, '$.expiresAt'))"
    );
    clauses.push(format!("{state}.deleted_at IS NULL"));
    clauses.push(format!(
        "({expires_at} IS NULL OR {expires_at} > strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))"
    ));
    match filters.read_state {
        Some(atm_storage::SearchReadState::Unread) => clauses.push(format!("{read} = 0")),
        Some(atm_storage::SearchReadState::Read) => clauses.push(format!("{read} != 0")),
        None => {}
    }
    match filters.ack_state {
        Some(atm_storage::SearchAckState::Pending) => clauses.push(format!(
            "{pending_ack_at} IS NOT NULL AND {acknowledged_at} IS NULL"
        )),
        Some(atm_storage::SearchAckState::Acknowledged) => {
            clauses.push(format!("{acknowledged_at} IS NOT NULL"));
        }
        Some(atm_storage::SearchAckState::NotRequired) => {
            clauses.push(format!("{pending_ack_at} IS NULL"));
        }
        None => {}
    }
    match filters.mailbox_selection {
        Some(atm_storage::SearchMailboxSelection::Actionable) => clauses.push(format!(
            "({pending_ack_at} IS NOT NULL AND {acknowledged_at} IS NULL OR ({pending_ack_at} IS NULL OR {acknowledged_at} IS NOT NULL) AND {read} = 0)"
        )),
        Some(atm_storage::SearchMailboxSelection::Unread) => clauses.push(format!(
            "({pending_ack_at} IS NULL OR {acknowledged_at} IS NOT NULL) AND {read} = 0"
        )),
        Some(atm_storage::SearchMailboxSelection::PendingAck) => clauses.push(format!(
            "{pending_ack_at} IS NOT NULL AND {acknowledged_at} IS NULL"
        )),
        Some(atm_storage::SearchMailboxSelection::All) | None => {}
    }
    if filters.current_only {
        push_current_only_filter(aliases, clauses);
    }
}

fn push_current_only_filter(aliases: SqlFilterAliases, clauses: &mut Vec<String>) {
    let document = aliases.document;
    clauses.push(format!(
        "({document}.message_id IS NULL OR NOT EXISTS (
                    SELECT 1
                    FROM mail_messages successor
                    LEFT JOIN mail_message_states successor_state
                      ON (successor_state.team, successor_state.agent, successor_state.message_key)
                       = (successor.team, successor.agent, successor.message_key)
                    WHERE successor.team = {document}.team
                      AND successor.agent = {document}.agent
                      AND successor.parent_message_id = {document}.message_id
                      AND successor_state.deleted_at IS NULL
                      AND (
                           COALESCE(
                               successor_state.expires_at,
                               json_extract(successor.envelope_json, '$.expiresAt')
                           ) IS NULL
                           OR COALESCE(
                               successor_state.expires_at,
                               json_extract(successor.envelope_json, '$.expiresAt')
                           ) > strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                      )
                ))"
    ));
}

fn push_scalar_filters(
    filters: &atm_storage::SearchFilters,
    aliases: SqlFilterAliases,
    clauses: &mut Vec<String>,
    parameters: &mut Vec<SqlValue>,
) {
    push_identity_scalar_filters(filters, aliases, clauses, parameters);
    push_content_scalar_filters(filters, aliases, clauses, parameters);
    push_workflow_scalar_filters(filters, aliases, clauses, parameters);
}

fn push_identity_scalar_filters(
    filters: &atm_storage::SearchFilters,
    aliases: SqlFilterAliases,
    clauses: &mut Vec<String>,
    parameters: &mut Vec<SqlValue>,
) {
    if let Some(team) = &filters.team {
        push_equals(
            clauses,
            parameters,
            &format!("{}.team", aliases.document),
            team.to_string(),
        );
    }
    if let Some(agent) = &filters.agent {
        push_equals(
            clauses,
            parameters,
            &format!("{}.agent", aliases.document),
            agent.to_string(),
        );
    }
    if let Some(from_agent) = &filters.from_agent {
        push_equals(
            clauses,
            parameters,
            &format!("{}.from_agent", aliases.document),
            from_agent.to_string(),
        );
    }
    if let Some(message_id) = &filters.message_id {
        push_equals(
            clauses,
            parameters,
            &format!("{}.message_id", aliases.document),
            message_id.to_string(),
        );
    }
    if let Some(task_id) = &filters.task_id {
        push_equals(
            clauses,
            parameters,
            &format!(
                "json_extract({}.envelope_json, '$.taskId')",
                aliases.message
            ),
            task_id.to_string(),
        );
    }
}

fn push_content_scalar_filters(
    filters: &atm_storage::SearchFilters,
    aliases: SqlFilterAliases,
    clauses: &mut Vec<String>,
    parameters: &mut Vec<SqlValue>,
) {
    if let Some(contains) = &filters.contains {
        clauses.push(format!(
            "(instr(lower(COALESCE({}.summary, '')), lower(?)) > 0 OR instr(lower(COALESCE({}.message_text, '')), lower(?)) > 0)",
            aliases.message, aliases.message
        ));
        parameters.push(SqlValue::Text(contains.clone()));
        parameters.push(SqlValue::Text(contains.clone()));
    }
    if let Some(template_sha) = &filters.template_sha {
        push_equals(
            clauses,
            parameters,
            &format!("{}.template_sha", aliases.message),
            template_sha.to_string(),
        );
    }
    if let Some(category) = &filters.category {
        push_equals(
            clauses,
            parameters,
            &format!("{}.category", aliases.message),
            category.clone(),
        );
    }
}

fn push_workflow_scalar_filters(
    filters: &atm_storage::SearchFilters,
    aliases: SqlFilterAliases,
    clauses: &mut Vec<String>,
    parameters: &mut Vec<SqlValue>,
) {
    if let Some(scope_kind) = &filters.workflow_scope_kind {
        push_equals(
            clauses,
            parameters,
            &format!("{}.workflow_scope_kind", aliases.message),
            scope_kind.to_string(),
        );
    }
    if let Some(scope_id) = &filters.workflow_scope_id {
        push_equals(
            clauses,
            parameters,
            &format!("{}.workflow_scope_id", aliases.message),
            scope_id.as_str().to_owned(),
        );
    }
    if let Some(state) = &filters.workflow_state {
        push_equals(
            clauses,
            parameters,
            &format!("{}.workflow_state", aliases.message),
            state.to_string(),
        );
    }
    if let Some(stage) = &filters.workflow_stage {
        push_equals(
            clauses,
            parameters,
            &format!("{}.workflow_stage", aliases.message),
            stage.to_string(),
        );
    }
    if let Some(transition) = &filters.workflow_transition {
        push_equals(
            clauses,
            parameters,
            &format!("{}.workflow_transition", aliases.message),
            transition.to_string(),
        );
    }
    if let Some(iteration) = &filters.workflow_iteration {
        push_equals(
            clauses,
            parameters,
            &format!("{}.workflow_iteration", aliases.message),
            iteration.as_str().to_owned(),
        );
    }
}

fn push_equals(
    clauses: &mut Vec<String>,
    parameters: &mut Vec<SqlValue>,
    column: &str,
    value: String,
) {
    clauses.push(format!("{column} = ?"));
    parameters.push(SqlValue::Text(value));
}

fn push_time_filters(
    filters: &atm_storage::SearchFilters,
    aliases: SqlFilterAliases,
    clauses: &mut Vec<String>,
    parameters: &mut Vec<SqlValue>,
) {
    if let Some(time_range) = &filters.time_range {
        if let Some(since) = time_range.since {
            clauses.push(format!("{}.message_at >= ?", aliases.document));
            parameters.push(SqlValue::Text(since.to_string()));
        }
        if let Some(until) = time_range.until {
            clauses.push(format!("{}.message_at <= ?", aliases.document));
            parameters.push(SqlValue::Text(until.to_string()));
        }
    }
}

fn push_tag_filters(
    filters: &atm_storage::SearchFilters,
    aliases: SqlFilterAliases,
    clauses: &mut Vec<String>,
    parameters: &mut Vec<SqlValue>,
) {
    for tag in &filters.tags {
        clauses.push(format!("EXISTS (SELECT 1 FROM json_each(COALESCE({}.tags_json, '[]')) AS tag WHERE CAST(tag.value AS TEXT) = ?)", aliases.message));
        parameters.push(SqlValue::Text(tag.clone()));
    }
    for tag in &filters.effective_tags {
        clauses.push(format!("EXISTS (SELECT 1 FROM json_each(COALESCE({}.effective_tags_json, '[]')) AS effective_tag WHERE CAST(effective_tag.value AS TEXT) = ?)", aliases.message));
        parameters.push(SqlValue::Text(tag.as_str().to_owned()));
    }
}

fn push_json_filters(
    filters: &atm_storage::SearchFilters,
    aliases: SqlFilterAliases,
    clauses: &mut Vec<String>,
    parameters: &mut Vec<SqlValue>,
) {
    for (key, value) in &filters.vars {
        clauses.push(json_scalar_filter(&format!(
            "{}.vars_json",
            aliases.message
        )));
        push_json_scalar_parameters(parameters, &format!("$.{}", key.as_str()), value.as_str());
    }
    for (key, matcher) in &filters.template_metadata {
        let path = format!("$.metadata.{}", key.as_str());
        match matcher {
            SearchMetadataMatch::Exact(value) => {
                clauses.push(json_scalar_filter("t.schema_json"));
                push_json_scalar_parameters(parameters, &path, value.as_str());
            }
            SearchMetadataMatch::Prefix(value) => {
                clauses.push(json_scalar_prefix_filter("t.schema_json"));
                push_json_scalar_prefix_parameters(parameters, &path, value.as_str());
            }
        }
    }
}

fn json_scalar_filter(column: &str) -> String {
    format!(
        "(CASE json_type({column}, ?) WHEN 'true' THEN 'true' WHEN 'false' THEN 'false' WHEN 'null' THEN 'null' ELSE CAST(json_extract({column}, ?) AS TEXT) END = ?)"
    )
}

fn json_scalar_prefix_filter(column: &str) -> String {
    format!(
        "(CASE json_type({column}, ?) WHEN 'true' THEN 'true' WHEN 'false' THEN 'false' WHEN 'null' THEN 'null' ELSE CAST(json_extract({column}, ?) AS TEXT) END LIKE ? ESCAPE '\\')"
    )
}

fn push_json_scalar_parameters(parameters: &mut Vec<SqlValue>, path: &str, value: &str) {
    parameters.push(SqlValue::Text(path.to_owned()));
    parameters.push(SqlValue::Text(path.to_owned()));
    parameters.push(SqlValue::Text(value.to_owned()));
}

fn push_json_scalar_prefix_parameters(parameters: &mut Vec<SqlValue>, path: &str, value: &str) {
    parameters.push(SqlValue::Text(path.to_owned()));
    parameters.push(SqlValue::Text(path.to_owned()));
    let escaped = value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    parameters.push(SqlValue::Text(format!("{escaped}%")));
}
