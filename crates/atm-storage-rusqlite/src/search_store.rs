//! Private FTS5/JSON1 implementation of the typed `atm-storage` search port.

use std::sync::Arc;

use atm_storage::{
    AsyncMessageSearchStore, AtmError, MessageSearchPage, MessageSearchQuery, MessageSearchStore,
    SearchAggregate, SearchCursor, SearchDeadline, SearchExpression, SearchGroup, SearchGroupBy,
    SearchGroupField, SearchMatchField, SearchMetadataMatch, SearchResultKey, SimpleAggregate,
    StoredSearchAddress, StoredSearchMatch, StoredWorkflowMetadata, TemplateFrontmatter,
    TemplateTag, WorkflowIteration, WorkflowScopeId, WorkflowScopeKind, WorkflowSnapshot,
    WorkflowStage, WorkflowState, WorkflowTransition,
};
use rusqlite::{Connection, params_from_iter, types::Value as SqlValue};
use serde_json::Value;

use crate::shared_db::{SharedDb, SharedDbTarget, sqlite_error};

pub(crate) fn search_store(db: Arc<SharedDb>) -> Arc<dyn MessageSearchStore> {
    Arc::new(SqliteMessageSearchStore::new(db))
}

pub(crate) fn async_search_store(db: Arc<SharedDb>) -> Arc<dyn AsyncMessageSearchStore> {
    Arc::new(SqliteMessageSearchStore::new(db))
}

#[derive(Debug)]
struct SqliteMessageSearchStore {
    db: Arc<SharedDb>,
}

impl SqliteMessageSearchStore {
    fn new(db: Arc<SharedDb>) -> Self {
        Self { db }
    }

    fn search_blocking(&self, query: &MessageSearchQuery) -> Result<MessageSearchPage, AtmError> {
        self.db.submit_search(query.clone())
    }
}

pub(crate) fn execute_search(
    query: &MessageSearchQuery,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<MessageSearchPage, AtmError> {
    let execution = SearchExecution::prepare(query)?;
    let mut matches = execute_primary_search(connection, target, &execution)?;
    if let Some(expression) = execution.expression.as_deref() {
        matches.extend(execute_template_search(
            connection,
            target,
            expression,
            &execution.filters,
        )?);
        matches.sort_by(|left, right| stable_compare(&left.stored, &right.stored));
    }
    finish_search_page(query, execution.cursor.as_ref(), matches)
}

struct SearchExecution {
    cursor: Option<CursorTuple>,
    expression: Option<String>,
    filters: SqlFilters,
}

impl SearchExecution {
    fn prepare(query: &MessageSearchQuery) -> Result<Self, AtmError> {
        query.validate()?;
        Ok(Self {
            cursor: query
                .page
                .cursor
                .as_ref()
                .map(|cursor| decode_cursor(cursor, query))
                .transpose()?,
            expression: query
                .expression
                .as_ref()
                .map(compile_expression)
                .transpose()?,
            filters: compile_sql_filters(&query.filters),
        })
    }
}

fn execute_primary_search(
    connection: &Connection,
    target: &SharedDbTarget,
    execution: &SearchExecution,
) -> Result<Vec<SearchRecord>, AtmError> {
    let sql = primary_search_sql(execution.expression.is_some(), &execution.filters);
    let parameters = search_parameters(execution.expression.as_deref(), &execution.filters);
    query_search_rows(connection, target, &sql, parameters, "typed FTS")
}

fn primary_search_sql(uses_fts: bool, filters: &SqlFilters) -> String {
    if uses_fts {
        format!(
            "SELECT d.team, d.agent, d.message_key, d.message_id, d.message_at,
                    d.from_agent, m.source_chat_id, m.destination_chat_id, m.template_sha,
                    t.template_type, m.category,
                    CASE WHEN instr(highlight(mail_messages_fts, 0, char(1), char(2)), char(1)) > 0 THEN 1 ELSE 0 END,
                    CASE WHEN instr(highlight(mail_messages_fts, 1, char(1), char(2)), char(1)) > 0 THEN 1 ELSE 0 END,
                    CASE WHEN instr(highlight(mail_messages_fts, 2, char(1), char(2)), char(1)) > 0 THEN 1 ELSE 0 END,
                    CASE WHEN instr(highlight(mail_messages_fts, 3, char(1), char(2)), char(1)) > 0 THEN 1 ELSE 0 END,
                    CASE WHEN instr(highlight(mail_messages_fts, 4, char(1), char(2)), char(1)) > 0 THEN 1 ELSE 0 END,
                    0,
                    snippet(mail_messages_fts, -1, char(1), char(2), '…', 16),
                    m.tags_json, m.vars_json, t.schema_json, m.content_format,
                    m.workflow_scope_kind, m.workflow_scope_id, m.workflow_state,
                    m.workflow_stage, m.workflow_transition, m.workflow_iteration,
                    m.applied_template_tags_json, m.effective_tags_json
             FROM mail_message_search_documents d
             JOIN mail_messages_fts ON mail_messages_fts.rowid = d.search_rowid
             JOIN mail_messages m ON (m.team, m.agent, m.message_key) = (d.team, d.agent, d.message_key)
             LEFT JOIN message_templates t ON t.template_sha = m.template_sha
             WHERE mail_messages_fts MATCH ?{}
             ORDER BY d.message_at DESC, d.team ASC, d.agent ASC, d.message_key ASC",
            filters.clause
        )
    } else {
        format!(
            "SELECT d.team, d.agent, d.message_key, d.message_id, d.message_at,
                    d.from_agent, m.source_chat_id, m.destination_chat_id, m.template_sha,
                    t.template_type, m.category,
                    0, 0, 0, 0, 0, 0, NULL, m.tags_json, m.vars_json, t.schema_json,
                    m.content_format, m.workflow_scope_kind, m.workflow_scope_id,
                    m.workflow_state, m.workflow_stage, m.workflow_transition,
                    m.workflow_iteration, m.applied_template_tags_json, m.effective_tags_json
             FROM mail_message_search_documents d
             JOIN mail_messages m ON (m.team, m.agent, m.message_key) = (d.team, d.agent, d.message_key)
             LEFT JOIN message_templates t ON t.template_sha = m.template_sha
             WHERE 1 = 1 {}
             ORDER BY d.message_at DESC, d.team ASC, d.agent ASC, d.message_key ASC",
            filters.clause
        )
    }
}

fn execute_template_search(
    connection: &Connection,
    target: &SharedDbTarget,
    expression: &str,
    filters: &SqlFilters,
) -> Result<Vec<SearchRecord>, AtmError> {
    let sql = format!(
        "SELECT d.team, d.agent, d.message_key, d.message_id, d.message_at,
                d.from_agent, m.source_chat_id, m.destination_chat_id, m.template_sha,
                t.template_type, m.category,
                0, 0, 0, 0, 0,
                CASE WHEN instr(highlight(message_templates_fts, 0, char(1), char(2)), char(1)) > 0 THEN 1 ELSE 0 END,
                snippet(message_templates_fts, -1, char(1), char(2), '…', 16),
                m.tags_json, m.vars_json, t.schema_json, m.content_format,
                m.workflow_scope_kind, m.workflow_scope_id, m.workflow_state,
                m.workflow_stage, m.workflow_transition, m.workflow_iteration,
                m.applied_template_tags_json, m.effective_tags_json
         FROM message_template_search_documents td
         JOIN message_templates_fts ON message_templates_fts.rowid = td.search_rowid
         JOIN mail_messages m ON m.template_sha = td.template_sha
         JOIN mail_message_search_documents d
           ON (d.team, d.agent, d.message_key) = (m.team, m.agent, m.message_key)
         LEFT JOIN message_templates t ON t.template_sha = m.template_sha
         WHERE message_templates_fts MATCH ?{}
         ORDER BY d.message_at DESC, d.team ASC, d.agent ASC, d.message_key ASC",
        filters.clause
    );
    query_search_rows(
        connection,
        target,
        &sql,
        search_parameters(Some(expression), filters),
        "template FTS",
    )
}

fn search_parameters(expression: Option<&str>, filters: &SqlFilters) -> Vec<SqlValue> {
    let mut parameters = expression
        .map(|expression| vec![SqlValue::Text(expression.to_owned())])
        .unwrap_or_default();
    parameters.extend(filters.parameters.clone());
    parameters
}

fn query_search_rows(
    connection: &Connection,
    target: &SharedDbTarget,
    sql: &str,
    parameters: Vec<SqlValue>,
    query_name: &str,
) -> Result<Vec<SearchRecord>, AtmError> {
    let mut statement = connection.prepare(sql).map_err(|error| {
        sqlite_error(
            target,
            format!("failed to prepare {query_name} query"),
            error,
        )
    })?;
    statement
        .query_map(params_from_iter(parameters), decode_search_row)
        .map_err(|error| {
            sqlite_error(
                target,
                format!("failed to execute {query_name} query"),
                error,
            )
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            AtmError::mailbox_read(format!("failed to decode {query_name} row: {error}"))
        })
}

fn finish_search_page(
    query: &MessageSearchQuery,
    cursor: Option<&CursorTuple>,
    mut matches: Vec<SearchRecord>,
) -> Result<MessageSearchPage, AtmError> {
    if !query.per_mailbox {
        // Pagination must operate over the deduplicated stable result set so a
        // duplicate skipped on the first page cannot reappear after its cursor.
        deduplicate(&mut matches);
    }
    if let Some(cursor) = cursor {
        matches.retain(|record| after_cursor(&record.stored, cursor));
    }
    let aggregate = aggregate(&matches, query.aggregate.as_ref());
    let limit = query.page.limit.get() as usize;
    let next_cursor = matches
        .get(limit.saturating_sub(1))
        .filter(|_| matches.len() > limit)
        .map(|record| encode_cursor(&record.stored, query))
        .transpose()?;
    matches.truncate(limit);
    Ok(MessageSearchPage {
        matches: matches.into_iter().map(|record| record.stored).collect(),
        aggregate,
        next_cursor,
    })
}

impl atm_storage::contract::sealed::Sealed for SqliteMessageSearchStore {}

impl MessageSearchStore for SqliteMessageSearchStore {
    fn search(&self, query: &MessageSearchQuery) -> Result<MessageSearchPage, AtmError> {
        self.search_blocking(query)
    }
}

#[async_trait::async_trait]
impl AsyncMessageSearchStore for SqliteMessageSearchStore {
    async fn search_async(
        &self,
        query: MessageSearchQuery,
        deadline: SearchDeadline,
    ) -> Result<MessageSearchPage, AtmError> {
        tokio::time::timeout(
            deadline.remaining(),
            self.db.submit_search_async(query, deadline.remaining()),
        )
        .await
        .map_err(|_| AtmError::daemon_unavailable("search reader lane exceeded its deadline"))?
    }
}

type SearchRow = (
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

#[derive(Debug, Clone)]
struct SearchRecord {
    stored: StoredSearchMatch,
    vars: Value,
}

struct DecodedSearchRow {
    team: String,
    agent: String,
    message_key: String,
    message_id: Option<String>,
    message_at: String,
    from_agent: String,
    source_chat_id: Option<String>,
    destination_chat_id: Option<String>,
    template_sha: Option<String>,
    template_type: Option<String>,
    category: Option<String>,
    match_fields: Vec<SearchMatchField>,
    snippet: Option<String>,
    vars_json: Option<String>,
    workflow: Option<StoredWorkflowMetadata>,
}

fn decode_search_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SearchRecord> {
    let decoded = decode_search_row_values(row)?;
    let vars = parse_json_or_default(decoded.vars_json.clone(), Value::Object(Default::default()))?;
    Ok(SearchRecord {
        vars,
        stored: decoded.into_stored()?,
    })
}

fn decode_search_row_values(row: &rusqlite::Row<'_>) -> rusqlite::Result<DecodedSearchRow> {
    let values: SearchRow = (
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
        row.get(13)?,
        row.get(14)?,
        row.get(15)?,
        row.get(16)?,
        row.get(17)?,
        row.get(18)?,
        row.get(19)?,
        row.get(20)?,
        row.get(21)?,
        row.get(22)?,
        row.get(23)?,
        row.get(24)?,
        row.get(25)?,
        row.get(26)?,
        row.get(27)?,
        row.get(28)?,
        row.get(29)?,
    );
    let (
        team,
        agent,
        message_key,
        message_id,
        message_at,
        from_agent,
        source_chat_id,
        destination_chat_id,
        template_sha,
        template_type,
        category,
        body,
        summary,
        tag,
        var_value,
        from_agent_match,
        template_content,
        snippet,
        tags_json,
        vars_json,
        template_metadata_json,
        content_format,
        scope_kind,
        scope_id,
        state,
        stage,
        transition,
        iteration,
        applied_tags_json,
        effective_tags_json,
    ) = values;
    Ok(DecodedSearchRow {
        team,
        agent,
        message_key,
        message_id,
        message_at,
        from_agent,
        source_chat_id,
        destination_chat_id,
        template_sha,
        template_type: template_type.clone(),
        category,
        match_fields: decode_match_fields(
            body,
            summary,
            tag,
            var_value,
            from_agent_match,
            template_content,
        ),
        snippet,
        vars_json,
        workflow: decode_workflow_metadata(
            tags_json,
            template_type.as_deref(),
            content_format.as_deref(),
            template_metadata_json,
            scope_kind,
            scope_id,
            state,
            stage,
            transition,
            iteration,
            applied_tags_json,
            effective_tags_json,
        )
        .map_err(to_sqlite_conversion_error)?,
    })
}

impl DecodedSearchRow {
    fn into_stored(self) -> rusqlite::Result<StoredSearchMatch> {
        let team: atm_storage::TeamName = self.team.parse().map_err(to_sqlite_conversion_error)?;
        let agent: atm_storage::AgentName =
            self.agent.parse().map_err(to_sqlite_conversion_error)?;
        let from_agent: atm_storage::AgentName = self
            .from_agent
            .parse()
            .map_err(to_sqlite_conversion_error)?;
        Ok(StoredSearchMatch {
            key: SearchResultKey {
                team: team.clone(),
                agent: agent.clone(),
                message_key: self
                    .message_key
                    .parse()
                    .map_err(to_sqlite_conversion_error)?,
            },
            message_id: self.message_id,
            message_at: self.message_at.parse().map_err(invalid_timestamp_error)?,
            from: StoredSearchAddress {
                agent: from_agent,
                team: team.clone(),
                chat_id: parse_optional_search_value(self.source_chat_id)?,
            },
            to: StoredSearchAddress {
                agent,
                team,
                chat_id: parse_optional_search_value(self.destination_chat_id)?,
            },
            template_sha: parse_optional_search_value(self.template_sha)?,
            template_type: self.template_type,
            category: self.category,
            match_fields: self.match_fields,
            snippet: self.snippet,
            workflow: self.workflow,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_workflow_metadata(
    tags_json: Option<String>,
    template_type: Option<&str>,
    content_format: Option<&str>,
    schema_json: Option<String>,
    scope_kind: Option<String>,
    scope_id: Option<String>,
    state: Option<String>,
    stage: Option<String>,
    transition: Option<String>,
    iteration: Option<String>,
    applied_tags_json: Option<String>,
    effective_tags_json: Option<String>,
) -> Result<Option<StoredWorkflowMetadata>, AtmError> {
    let snapshot = match (scope_kind, scope_id, state, stage, transition, iteration) {
        (
            Some(scope_kind),
            Some(scope_id),
            Some(state),
            Some(stage),
            Some(transition),
            iteration,
        ) => Some(WorkflowSnapshot {
            scope_kind: WorkflowScopeKind::new(scope_kind)?,
            scope_id: WorkflowScopeId::new(scope_id)?,
            state: WorkflowState::new(state)?,
            stage: WorkflowStage::new(stage)?,
            transition: WorkflowTransition::new(transition)?,
            iteration: iteration.map(WorkflowIteration::new).transpose()?,
        }),
        (None, None, None, None, None, None) => None,
        _ => {
            return Err(AtmError::mailbox_read(
                "stored workflow snapshot is incomplete",
            ));
        }
    };
    match (snapshot, applied_tags_json, effective_tags_json) {
        (None, None, None) => Ok(None),
        (Some(snapshot), Some(applied), Some(effective)) => {
            let instance_tags: Vec<atm_storage::InstanceTag> = serde_json::from_str(
                &tags_json.unwrap_or_else(|| "[]".to_owned()),
            )
            .map_err(|error| {
                AtmError::mailbox_read(format!("stored instance tags are invalid: {error}"))
            })?;
            let applied_template_tags: Vec<TemplateTag> =
                serde_json::from_str(&applied).map_err(|error| {
                    AtmError::mailbox_read(format!("stored applied tags are invalid: {error}"))
                })?;
            let effective_tags: Vec<atm_storage::EffectiveTag> = serde_json::from_str(&effective)
                .map_err(|error| {
                AtmError::mailbox_read(format!("stored effective tags are invalid: {error}"))
            })?;
            let frontmatter: TemplateFrontmatter =
                serde_json::from_str(schema_json.as_deref().unwrap_or("{}")).map_err(|error| {
                    AtmError::mailbox_read(format!("stored template metadata is invalid: {error}"))
                })?;
            let expected = atm_storage::DecomposedMessageAdmission::expected_tag_provenance_for(
                &instance_tags,
                &frontmatter.template_tags,
                template_type,
                content_format,
                &snapshot,
            )?;
            if expected.applied_template_tags != applied_template_tags
                || expected.effective_tags != effective_tags
            {
                return Err(AtmError::mailbox_read(
                    "stored workflow tag provenance does not match immutable admission inputs",
                ));
            }
            Ok(Some(StoredWorkflowMetadata {
                snapshot,
                tag_provenance: expected,
            }))
        }
        _ => Err(AtmError::mailbox_read(
            "stored workflow tag provenance is incomplete",
        )),
    }
}

fn decode_match_fields(
    body: i64,
    summary: i64,
    tag: i64,
    var_value: i64,
    from_agent: i64,
    template_content: i64,
) -> Vec<SearchMatchField> {
    [
        (body, SearchMatchField::BodyText),
        (summary, SearchMatchField::Summary),
        (tag, SearchMatchField::Tag),
        (var_value, SearchMatchField::VarValue),
        (from_agent, SearchMatchField::FromAgent),
        (template_content, SearchMatchField::TemplateContent),
    ]
    .into_iter()
    .filter_map(|(matched, field)| (matched != 0).then_some(field))
    .collect()
}

fn parse_optional_search_value<T, E>(value: Option<String>) -> rusqlite::Result<Option<T>>
where
    T: std::str::FromStr<Err = E>,
    E: std::fmt::Display,
{
    value
        .map(|value| value.parse::<T>())
        .transpose()
        .map_err(|error| to_sqlite_conversion_error(AtmError::validation(error.to_string())))
}

fn invalid_timestamp_error(error: impl std::fmt::Display) -> rusqlite::Error {
    to_sqlite_conversion_error(AtmError::validation(format!(
        "stored search timestamp is invalid: {error}"
    )))
}

fn parse_json_or_default(raw: Option<String>, default: Value) -> rusqlite::Result<Value> {
    match raw {
        Some(raw) => serde_json::from_str(&raw).map_err(|error| {
            to_sqlite_conversion_error(AtmError::mailbox_read(format!(
                "stored search JSON projection is invalid: {error}"
            )))
        }),
        None => Ok(default),
    }
}

fn to_sqlite_conversion_error(error: AtmError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::other(error.to_string())),
    )
}

fn scalar_json_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Null => "null".to_owned(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

#[derive(Debug, Clone)]
struct SqlFilters {
    clause: String,
    parameters: Vec<SqlValue>,
}

fn compile_sql_filters(filters: &atm_storage::SearchFilters) -> SqlFilters {
    let mut clauses = Vec::new();
    let mut parameters = Vec::new();
    let mut equals = |column: &str, value: String| {
        clauses.push(format!("{column} = ?"));
        parameters.push(SqlValue::Text(value));
    };
    if let Some(team) = &filters.team {
        equals("d.team", team.to_string());
    }
    if let Some(agent) = &filters.agent {
        equals("d.agent", agent.to_string());
    }
    if let Some(from_agent) = &filters.from_agent {
        equals("d.from_agent", from_agent.to_string());
    }
    if let Some(template_sha) = &filters.template_sha {
        equals("m.template_sha", template_sha.to_string());
    }
    if let Some(category) = &filters.category {
        equals("m.category", category.clone());
    }
    if let Some(scope_kind) = &filters.workflow_scope_kind {
        equals("m.workflow_scope_kind", scope_kind.to_string());
    }
    if let Some(scope_id) = &filters.workflow_scope_id {
        equals("m.workflow_scope_id", scope_id.as_str().to_owned());
    }
    if let Some(state) = &filters.workflow_state {
        equals("m.workflow_state", state.to_string());
    }
    if let Some(stage) = &filters.workflow_stage {
        equals("m.workflow_stage", stage.to_string());
    }
    if let Some(transition) = &filters.workflow_transition {
        equals("m.workflow_transition", transition.to_string());
    }
    if let Some(iteration) = &filters.workflow_iteration {
        equals("m.workflow_iteration", iteration.as_str().to_owned());
    }
    if let Some(time_range) = &filters.time_range {
        if let Some(since) = time_range.since {
            clauses.push("d.message_at >= ?".to_owned());
            parameters.push(SqlValue::Text(since.to_string()));
        }
        if let Some(until) = time_range.until {
            clauses.push("d.message_at <= ?".to_owned());
            parameters.push(SqlValue::Text(until.to_string()));
        }
    }
    for tag in &filters.tags {
        clauses.push(
            "EXISTS (SELECT 1 FROM json_each(COALESCE(m.tags_json, '[]')) AS tag WHERE CAST(tag.value AS TEXT) = ?)"
                .to_owned(),
        );
        parameters.push(SqlValue::Text(tag.clone()));
    }
    for tag in &filters.effective_tags {
        clauses.push(
            "EXISTS (SELECT 1 FROM json_each(COALESCE(m.effective_tags_json, '[]')) AS effective_tag WHERE CAST(effective_tag.value AS TEXT) = ?)"
                .to_owned(),
        );
        parameters.push(SqlValue::Text(tag.as_str().to_owned()));
    }
    for (key, value) in &filters.vars {
        clauses.push(json_scalar_filter("m.vars_json"));
        push_json_scalar_parameters(
            &mut parameters,
            &format!("$.{}", key.as_str()),
            value.as_str(),
        );
    }
    for (key, matcher) in &filters.template_metadata {
        let path = format!("$.metadata.{}", key.as_str());
        match matcher {
            SearchMetadataMatch::Exact(value) => {
                clauses.push(json_scalar_filter("t.schema_json"));
                push_json_scalar_parameters(&mut parameters, &path, value.as_str());
            }
            SearchMetadataMatch::Prefix(value) => {
                clauses.push(json_scalar_prefix_filter("t.schema_json"));
                push_json_scalar_prefix_parameters(&mut parameters, &path, value.as_str());
            }
        }
    }
    SqlFilters {
        clause: clauses
            .into_iter()
            .map(|clause| format!(" AND {clause}"))
            .collect(),
        parameters,
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

fn deduplicate(matches: &mut Vec<SearchRecord>) {
    let mut seen = std::collections::BTreeSet::new();
    matches.retain(|record| {
        let stored = &record.stored;
        let key = stored.message_id.clone().unwrap_or_else(|| {
            format!(
                "{}\u{1f}{}\u{1f}{}",
                stored.key.team, stored.key.agent, stored.key.message_key
            )
        });
        seen.insert(key)
    });
}

fn stable_compare(left: &StoredSearchMatch, right: &StoredSearchMatch) -> std::cmp::Ordering {
    right
        .message_at
        .cmp(&left.message_at)
        .then_with(|| left.key.team.cmp(&right.key.team))
        .then_with(|| left.key.agent.cmp(&right.key.agent))
        .then_with(|| left.key.message_key.cmp(&right.key.message_key))
}

fn aggregate(
    records: &[SearchRecord],
    requested: Option<&SimpleAggregate>,
) -> Option<SearchAggregate> {
    match requested {
        None => None,
        Some(SimpleAggregate::Count) => Some(SearchAggregate::Count {
            value: records.len() as u64,
        }),
        Some(SimpleAggregate::Min(field)) => Some(SearchAggregate::Timestamp {
            field: *field,
            value: records.iter().map(|record| record.stored.message_at).min(),
        }),
        Some(SimpleAggregate::Max(field)) => Some(SearchAggregate::Timestamp {
            field: *field,
            value: records.iter().map(|record| record.stored.message_at).max(),
        }),
        Some(SimpleAggregate::GroupBy(by)) => {
            let mut buckets = std::collections::BTreeMap::<String, u64>::new();
            for record in records {
                let stored = &record.stored;
                let key = match by {
                    SearchGroupBy::Field(SearchGroupField::Team) => stored.key.team.to_string(),
                    SearchGroupBy::Field(SearchGroupField::Agent) => stored.key.agent.to_string(),
                    SearchGroupBy::Field(SearchGroupField::FromAgent) => {
                        stored.from.agent.to_string()
                    }
                    SearchGroupBy::Field(SearchGroupField::TemplateType) => {
                        stored.template_type.clone().unwrap_or_default()
                    }
                    SearchGroupBy::Field(SearchGroupField::Category) => {
                        stored.category.clone().unwrap_or_default()
                    }
                    SearchGroupBy::Field(SearchGroupField::WorkflowScopeKind) => stored
                        .workflow
                        .as_ref()
                        .map(|workflow| workflow.snapshot.scope_kind.to_string())
                        .unwrap_or_default(),
                    SearchGroupBy::Field(SearchGroupField::WorkflowState) => stored
                        .workflow
                        .as_ref()
                        .map(|workflow| workflow.snapshot.state.to_string())
                        .unwrap_or_default(),
                    SearchGroupBy::Field(SearchGroupField::WorkflowStage) => stored
                        .workflow
                        .as_ref()
                        .map(|workflow| workflow.snapshot.stage.to_string())
                        .unwrap_or_default(),
                    SearchGroupBy::Field(SearchGroupField::WorkflowTransition) => stored
                        .workflow
                        .as_ref()
                        .map(|workflow| workflow.snapshot.transition.to_string())
                        .unwrap_or_default(),
                    SearchGroupBy::Var(key) => record
                        .vars
                        .as_object()
                        .and_then(|vars| vars.get(key.as_str()))
                        .map(scalar_json_text)
                        .unwrap_or_default(),
                };
                *buckets.entry(key).or_default() += 1;
            }
            Some(SearchAggregate::Groups {
                by: by.clone(),
                groups: buckets
                    .into_iter()
                    .map(|(key, count)| SearchGroup { key, count })
                    .collect(),
            })
        }
    }
}

fn compile_expression(expression: &SearchExpression) -> Result<String, AtmError> {
    expression.validate()?;
    Ok(match expression {
        SearchExpression::Atom(atom) => quote_fts(atom.text()),
        SearchExpression::All(children) => children
            .iter()
            .map(compile_expression)
            .collect::<Result<Vec<_>, _>>()?
            .join(" AND "),
        SearchExpression::Any(children) => format!(
            "({})",
            children
                .iter()
                .map(compile_expression)
                .collect::<Result<Vec<_>, _>>()?
                .join(" OR ")
        ),
        SearchExpression::Not(child) => format!("NOT {}", compile_expression(child)?),
        SearchExpression::Near {
            terms,
            max_distance,
        } => format!(
            "NEAR({}, {})",
            terms
                .iter()
                .map(|term| quote_fts(term.text()))
                .collect::<Vec<_>>()
                .join(" "),
            max_distance
        ),
    })
}

fn quote_fts(term: &str) -> String {
    format!("\"{}\"", term.replace('"', "\"\""))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CursorTuple(String, String, String, String, String);

fn encode_cursor(
    record: &StoredSearchMatch,
    query: &MessageSearchQuery,
) -> Result<SearchCursor, AtmError> {
    SearchCursor::new(
        serde_json::to_string(&CursorTuple(
            query_signature(query),
            record.message_at.to_string(),
            record.key.team.to_string(),
            record.key.agent.to_string(),
            record.key.message_key.to_string(),
        ))
        .expect("cursor tuple serializes"),
    )
}

fn decode_cursor(
    cursor: &SearchCursor,
    query: &MessageSearchQuery,
) -> Result<CursorTuple, AtmError> {
    let cursor = serde_json::from_str::<CursorTuple>(cursor.as_str()).map_err(|_| {
        AtmError::validation("search cursor is malformed or belongs to a different query")
    })?;
    if cursor.0 != query_signature(query) {
        return Err(AtmError::validation(
            "search cursor is malformed or belongs to a different query",
        ));
    }
    Ok(cursor)
}

fn query_signature(query: &MessageSearchQuery) -> String {
    // Cursor signing is an integrity check against accidental query reuse,
    // not a security token. It never exposes SQLite/FTS implementation text.
    let mut signature_query = query.clone();
    signature_query.page.cursor = None;
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in format!("{signature_query:?}").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn after_cursor(record: &StoredSearchMatch, cursor: &CursorTuple) -> bool {
    // The stable order is timestamp DESC then mailbox/key ASC. A tuple after
    // the cursor therefore uses the inverse comparison only for timestamp.
    cursor
        .1
        .cmp(&record.message_at.to_string())
        .then_with(|| record.key.team.to_string().cmp(&cursor.2))
        .then_with(|| record.key.agent.to_string().cmp(&cursor.3))
        .then_with(|| record.key.message_key.to_string().cmp(&cursor.4))
        .is_gt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SqliteStorageBackend;
    use atm_storage::schema::MessageEnvelope;
    use atm_storage::{
        AtmMessageId, IsoTimestamp, MessageKey, SearchAggregate, SearchAtom, SearchExpression,
        SearchGroupBy, SearchGroupField, SearchLimit, SimpleAggregate,
    };
    use serde_json::Map;

    fn save(
        backend: &SqliteStorageBackend,
        team: &str,
        agent: &str,
        key: &str,
        message_id: Option<&str>,
        timestamp: &str,
    ) {
        save_with_extra(backend, team, agent, key, message_id, timestamp, Map::new());
    }

    fn save_with_extra(
        backend: &SqliteStorageBackend,
        team: &str,
        agent: &str,
        key: &str,
        message_id: Option<&str>,
        timestamp: &str,
        extra: Map<String, serde_json::Value>,
    ) {
        backend
            .save_message_record(
                team.parse().expect("team"),
                agent.parse().expect("agent"),
                MessageKey::new(key).expect("message key"),
                MessageEnvelope {
                    from: "sender".parse().expect("sender"),
                    source_chat_id: None,
                    text: format!("durable query fixture {key}"),
                    timestamp: timestamp.parse::<IsoTimestamp>().expect("timestamp"),
                    read: false,
                    source_team: Some(team.parse().expect("source team")),
                    destination_chat_id: None,
                    summary: None,
                    message_id: message_id
                        .map(|value| value.parse::<AtmMessageId>().expect("message ID")),
                    requires_ack: false,
                    pending_ack_at: None,
                    acknowledged_at: None,
                    acknowledges_message_id: None,
                    parent_message_id: None,
                    thread_mode: None,
                    expires_at: None,
                    task_id: None,
                    extra,
                },
            )
            .expect("seed production SQLite backend");
    }

    fn query(per_mailbox: bool, limit: u32, cursor: Option<SearchCursor>) -> MessageSearchQuery {
        MessageSearchQuery {
            page: atm_storage::SearchPageRequest {
                limit: SearchLimit::new(limit).expect("limit"),
                cursor,
            },
            per_mailbox,
            ..MessageSearchQuery::default()
        }
    }

    #[test]
    fn production_sqlite_search_deduplicates_and_pages_null_message_ids() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let shared_message_id = "01KZTTRD6K9WJYJ2N7E39CVB9P";
        save(
            &backend,
            "query-team",
            "agent-one",
            "atm:one",
            Some(shared_message_id),
            "2026-08-12T00:03:00Z",
        );
        save(
            &backend,
            "query-team",
            "agent-two",
            "atm:two",
            Some(shared_message_id),
            "2026-08-12T00:02:00Z",
        );
        save(
            &backend,
            "query-team",
            "agent-three",
            "atm:three",
            None,
            "2026-08-12T00:01:00Z",
        );

        let first = backend
            .message_search_store()
            .search(&query(false, 1, None))
            .expect("first page");
        let cursor = first
            .next_cursor
            .clone()
            .expect("deduplicated page continues");
        let second = backend
            .message_search_store()
            .search(&query(false, 1, Some(cursor)))
            .expect("second page");
        let keys = first
            .matches
            .into_iter()
            .chain(second.matches)
            .map(|hit| hit.key.message_key.to_string())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            keys.len(),
            2,
            "default dedup and cursor omit no durable result"
        );

        let mailbox_rows = backend
            .message_search_store()
            .search(&query(true, 10, None))
            .expect("per-mailbox search");
        assert_eq!(
            mailbox_rows.matches.len(),
            3,
            "per-mailbox retains duplicate message IDs and NULL IDs"
        );
        assert!(
            mailbox_rows
                .matches
                .iter()
                .any(|hit| hit.message_id.is_none())
        );

        let mut aggregate_query = query(false, 10, None);
        aggregate_query.aggregate = Some(SimpleAggregate::Count);
        let aggregate = backend
            .message_search_store()
            .search(&aggregate_query)
            .expect("aggregate search");
        assert_eq!(
            aggregate.aggregate,
            Some(SearchAggregate::Count { value: 2 }),
            "aggregate observes the same deduplicated result set as paging"
        );
    }

    #[test]
    fn production_sqlite_search_applies_filters_and_aggregates() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let mut assignment = Map::new();
        assignment.insert(
            "category".to_owned(),
            serde_json::Value::String("assignment".to_owned()),
        );
        assignment.insert("tags".to_owned(), serde_json::json!(["phase-an", "urgent"]));
        save_with_extra(
            &backend,
            "query-team",
            "agent-one",
            "atm:assignment",
            Some("01KZTTRD6K9WJYJ2N7E39CVB9P"),
            "2026-08-12T00:03:00Z",
            assignment,
        );
        let mut completion = Map::new();
        completion.insert(
            "category".to_owned(),
            serde_json::Value::String("completion".to_owned()),
        );
        completion.insert("tags".to_owned(), serde_json::json!(["phase-an"]));
        save_with_extra(
            &backend,
            "query-team",
            "agent-two",
            "atm:completion",
            Some("01KZTTRD6K9WJYJ2N7E39CVB9Q"),
            "2026-08-12T00:02:00Z",
            completion,
        );
        save(
            &backend,
            "other-team",
            "agent-three",
            "atm:other",
            None,
            "2026-08-12T00:01:00Z",
        );

        let mut filtered = MessageSearchQuery::default();
        filtered.filters.team = Some("query-team".parse().expect("team"));
        filtered.filters.category = Some("assignment".to_owned());
        filtered.filters.tags = vec!["phase-an".to_owned(), "urgent".to_owned()];
        filtered.expression = Some(SearchExpression::Atom(
            SearchAtom::phrase("durable query fixture").expect("phrase"),
        ));
        let page = backend
            .message_search_store()
            .search(&filtered)
            .expect("filtered production search");
        assert_eq!(page.matches.len(), 1);
        assert_eq!(page.matches[0].key.message_key.as_str(), "atm:assignment");

        let mut grouped = MessageSearchQuery::default();
        grouped.filters.team = Some("query-team".parse().expect("team"));
        grouped.aggregate = Some(SimpleAggregate::GroupBy(SearchGroupBy::Field(
            SearchGroupField::Category,
        )));
        let grouped_page = backend
            .message_search_store()
            .search(&grouped)
            .expect("grouped production search");
        assert_eq!(
            grouped_page.aggregate,
            Some(SearchAggregate::Groups {
                by: SearchGroupBy::Field(SearchGroupField::Category),
                groups: vec![
                    atm_storage::SearchGroup {
                        key: "assignment".to_owned(),
                        count: 1,
                    },
                    atm_storage::SearchGroup {
                        key: "completion".to_owned(),
                        count: 1,
                    },
                ],
            })
        );

        let mut by_tag = MessageSearchQuery::default();
        by_tag.filters.tags = vec!["urgent".to_owned()];
        let tag_page = backend
            .message_search_store()
            .search(&by_tag)
            .expect("tag and variable production search");
        assert_eq!(tag_page.matches.len(), 1);
        assert_eq!(
            tag_page.matches[0].key.message_key.as_str(),
            "atm:assignment"
        );
    }

    #[test]
    fn production_search_filters_workflow_and_returns_explicit_provenance() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let record = atm_storage::Message {
            team: "query-team".parse().expect("team"),
            agent: "agent".parse().expect("agent"),
            message_key: MessageKey::new("atm:workflow-search").expect("key"),
            envelope: MessageEnvelope {
                from: "sender".parse().expect("sender"),
                source_chat_id: None,
                text: "workflow search".to_owned(),
                timestamp: "2026-08-12T00:00:00Z".parse().expect("time"),
                read: false,
                source_team: Some("query-team".parse().expect("team")),
                destination_chat_id: None,
                summary: None,
                message_id: Some("01KZTTRD6K9WJYJ2N7E39CVB9P".parse().expect("message id")),
                requires_ack: false,
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: None,
                extra: Map::new(),
            },
        };
        backend.message_store().save_message(&record).expect("save");
        let mut template = atm_storage::TemplateRegistration {
            sha: "a".repeat(64).parse().expect("sha"),
            template_type: Some("workflow-event".to_owned()),
            template_name: Some("workflow search fixture".to_owned()),
            content_bytes: b"workflow".to_vec(),
            content_text: "workflow".to_owned(),
            output_format: atm_storage::TemplateOutputFormat::Text,
            frontmatter: atm_storage::TemplateFrontmatter::default(),
            first_seen: atm_storage::TemplateFirstSeen::new(IsoTimestamp::now(), "tester")
                .expect("seen"),
        };
        template.frontmatter.metadata = [
            ("tags".to_owned(), serde_json::json!(["domain:testing"])),
            ("workflow".to_owned(), serde_json::json!({"scope":{"kind":"sprint","variable":"sprint"},"state":"opened","stage":"dev","transition":"start"})),
        ].into_iter().collect();
        let template = template
            .into_normalized_workflow_metadata()
            .expect("template");
        let snapshot = atm_storage::WorkflowSnapshot {
            scope_kind: atm_storage::WorkflowScopeKind::new("sprint").expect("kind"),
            scope_id: atm_storage::WorkflowScopeId::new("an-11").expect("id"),
            state: atm_storage::WorkflowState::new("opened").expect("state"),
            stage: atm_storage::WorkflowStage::new("dev").expect("stage"),
            transition: atm_storage::WorkflowTransition::new("start").expect("transition"),
            iteration: None,
        };
        let mut admission = atm_storage::DecomposedMessageAdmission {
            template: template.clone(),
            message: atm_storage::DecomposedMessageRecord {
                key: record.message_key.clone(),
                template_sha: template.sha.clone(),
                vars: atm_storage::MergedVarsJson::from_merged_object(
                    [(String::from("sprint"), serde_json::json!("an-11"))]
                        .into_iter()
                        .collect(),
                ),
                category: None,
                tags: vec![atm_storage::InstanceTag::new("audience:test").expect("tag")],
                content_format: Some("markdown".to_owned()),
                workflow: None,
            },
        };
        let provenance = admission
            .expected_tag_provenance(&snapshot)
            .expect("provenance");
        admission.message.workflow = Some(atm_storage::WorkflowAdmission {
            snapshot,
            tag_provenance: provenance,
        });
        backend
            .template_catalog_store()
            .admit_decomposed_message(admission)
            .expect("admit");

        let mut query = MessageSearchQuery::default();
        query.filters.workflow_scope_kind =
            Some(atm_storage::WorkflowScopeKind::new("sprint").expect("kind"));
        query.filters.workflow_scope_id =
            Some(atm_storage::WorkflowScopeId::new("an-11").expect("id"));
        query.filters.workflow_state =
            Some(atm_storage::WorkflowState::new("opened").expect("state"));
        query.filters.effective_tags =
            vec![atm_storage::EffectiveTag::new("workflow-state:opened").expect("tag")];
        query.aggregate = Some(SimpleAggregate::GroupBy(SearchGroupBy::Field(
            SearchGroupField::WorkflowState,
        )));
        let page = backend
            .message_search_store()
            .search(&query)
            .expect("search");
        let workflow = page
            .matches
            .into_iter()
            .next()
            .expect("one match")
            .workflow
            .expect("provenance");
        assert_eq!(
            workflow.tag_provenance.instance_tags[0].as_str(),
            "audience:test"
        );
        assert_eq!(
            workflow.tag_provenance.applied_template_tags[0].as_str(),
            "domain:testing"
        );
        assert!(
            workflow
                .tag_provenance
                .derived_tags
                .iter()
                .any(|tag| tag.as_str() == "workflow-state:opened")
        );
        assert!(matches!(
            page.aggregate,
            Some(SearchAggregate::Groups { .. })
        ));
    }
}
