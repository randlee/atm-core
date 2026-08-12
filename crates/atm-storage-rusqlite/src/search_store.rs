//! Private FTS5/JSON1 implementation of the typed `atm-storage` search port.

use std::sync::Arc;

use atm_storage::{
    AsyncMessageSearchStore, AtmError, MessageSearchPage, MessageSearchQuery, MessageSearchStore,
    SearchAggregate, SearchCursor, SearchDeadline, SearchExpression, SearchGroup, SearchGroupBy,
    SearchGroupField, SearchMatchField, SearchResultKey, SimpleAggregate, StoredSearchAddress,
    StoredSearchMatch,
};
use rusqlite::{Connection, params};
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
    query.validate()?;
    let cursor = query
        .page
        .cursor
        .as_ref()
        .map(|cursor| decode_cursor(cursor, query))
        .transpose()?;
    let match_expression = query
        .expression
        .as_ref()
        .map(compile_expression)
        .transpose()?;
    let uses_fts = match_expression.is_some();
    let sql = if uses_fts {
        "SELECT d.team, d.agent, d.message_key, d.message_id, d.message_at,
                        d.from_agent, m.source_chat_id, m.destination_chat_id, m.template_sha,
                        t.template_type, m.category,
                        CASE WHEN instr(highlight(mail_messages_fts, 0, char(1), char(2)), char(1)) > 0 THEN 1 ELSE 0 END,
                        CASE WHEN instr(highlight(mail_messages_fts, 1, char(1), char(2)), char(1)) > 0 THEN 1 ELSE 0 END,
                        CASE WHEN instr(highlight(mail_messages_fts, 2, char(1), char(2)), char(1)) > 0 THEN 1 ELSE 0 END,
                        CASE WHEN instr(highlight(mail_messages_fts, 3, char(1), char(2)), char(1)) > 0 THEN 1 ELSE 0 END,
                        0, m.tags_json, m.vars_json, t.schema_json
                 FROM mail_message_search_documents d
                 JOIN mail_messages_fts ON mail_messages_fts.rowid = d.search_rowid
                 JOIN mail_messages m ON (m.team, m.agent, m.message_key) = (d.team, d.agent, d.message_key)
                 LEFT JOIN message_templates t ON t.template_sha = m.template_sha
                 WHERE mail_messages_fts MATCH ?1
                 ORDER BY d.message_at DESC, d.team ASC, d.agent ASC, d.message_key ASC"
    } else {
        "SELECT d.team, d.agent, d.message_key, d.message_id, d.message_at,
                        d.from_agent, m.source_chat_id, m.destination_chat_id, m.template_sha,
                        t.template_type, m.category,
                        0, 0, 0, 0, 0, m.tags_json, m.vars_json, t.schema_json
                 FROM mail_message_search_documents d
                 JOIN mail_messages m ON (m.team, m.agent, m.message_key) = (d.team, d.agent, d.message_key)
                 LEFT JOIN message_templates t ON t.template_sha = m.template_sha
                 ORDER BY d.message_at DESC, d.team ASC, d.agent ASC, d.message_key ASC"
    };
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| sqlite_error(target, "failed to prepare typed FTS query", error))?;
    let rows = if let Some(expression) = &match_expression {
        statement.query_map(params![expression], decode_search_row)
    } else {
        statement.query_map([], decode_search_row)
    }
    .map_err(|error| sqlite_error(target, "failed to execute typed FTS query", error))?;
    let mut matches = rows.collect::<Result<Vec<_>, _>>().map_err(|error| {
        AtmError::mailbox_read(format!("failed to decode typed FTS row: {error}"))
    })?;
    if uses_fts {
        let template_sql = "SELECT d.team, d.agent, d.message_key, d.message_id, d.message_at,
                        d.from_agent, m.source_chat_id, m.destination_chat_id, m.template_sha,
                        t.template_type, m.category,
                        0, 0, 0, 0,
                        CASE WHEN instr(highlight(message_templates_fts, 0, char(1), char(2)), char(1)) > 0 THEN 1 ELSE 0 END,
                        m.tags_json, m.vars_json, t.schema_json
                     FROM message_template_search_documents td
                     JOIN message_templates_fts ON message_templates_fts.rowid = td.search_rowid
                     JOIN mail_messages m ON m.template_sha = td.template_sha
                     JOIN mail_message_search_documents d
                       ON (d.team, d.agent, d.message_key) = (m.team, m.agent, m.message_key)
                     LEFT JOIN message_templates t ON t.template_sha = m.template_sha
                     WHERE message_templates_fts MATCH ?1
                     ORDER BY d.message_at DESC, d.team ASC, d.agent ASC, d.message_key ASC";
        let mut templates = connection
            .prepare(template_sql)
            .map_err(|error| sqlite_error(target, "failed to prepare template FTS query", error))?;
        let template_matches = templates
            .query_map(
                params![match_expression.as_deref().expect("FTS expression")],
                decode_search_row,
            )
            .map_err(|error| sqlite_error(target, "failed to execute template FTS query", error))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                AtmError::mailbox_read(format!("failed to decode template FTS row: {error}"))
            })?;
        matches.extend(template_matches);
        matches.sort_by(|left, right| stable_compare(&left.stored, &right.stored));
    }
    matches.retain(|record| matches_filters(record, query));
    if let Some(cursor) = cursor.as_ref() {
        matches.retain(|record| after_cursor(&record.stored, cursor));
    }
    let aggregate = aggregate(&matches, query.aggregate.as_ref());
    if !query.per_mailbox {
        deduplicate(&mut matches);
    }
    let limit = query.page.limit.get() as usize;
    let next_cursor = if matches.len() > limit {
        let final_match = &matches[limit - 1];
        Some(encode_cursor(&final_match.stored, query)?)
    } else {
        None
    };
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
        tokio::time::timeout(deadline.remaining(), self.db.submit_search_async(query))
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
    Option<String>,
    Option<String>,
    Option<String>,
);

#[derive(Debug, Clone)]
struct SearchRecord {
    stored: StoredSearchMatch,
    tags: Value,
    vars: Value,
    template_metadata: Value,
}

fn decode_search_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SearchRecord> {
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
        template_content,
        tags_json,
        vars_json,
        template_metadata_json,
    ) = values;
    let mut match_fields = Vec::new();
    if body != 0 {
        match_fields.push(SearchMatchField::BodyText);
    }
    if summary != 0 {
        match_fields.push(SearchMatchField::Summary);
    }
    if tag != 0 {
        match_fields.push(SearchMatchField::Tag);
    }
    if var_value != 0 {
        match_fields.push(SearchMatchField::VarValue);
    }
    if template_content != 0 {
        match_fields.push(SearchMatchField::TemplateContent);
    }
    let team: atm_storage::TeamName = team.parse().map_err(to_sqlite_conversion_error)?;
    let agent: atm_storage::AgentName = agent.parse().map_err(to_sqlite_conversion_error)?;
    let from_agent: atm_storage::AgentName =
        from_agent.parse().map_err(to_sqlite_conversion_error)?;
    let stored = StoredSearchMatch {
        key: SearchResultKey {
            team: team.clone(),
            agent: agent.clone(),
            message_key: message_key.parse().map_err(to_sqlite_conversion_error)?,
        },
        message_id,
        message_at: message_at.parse().map_err(|error| {
            to_sqlite_conversion_error(AtmError::validation(format!(
                "stored search timestamp is invalid: {error}"
            )))
        })?,
        from: StoredSearchAddress {
            agent: from_agent,
            team: team.clone(),
            chat_id: source_chat_id
                .map(|value| value.parse())
                .transpose()
                .map_err(to_sqlite_conversion_error)?,
        },
        to: StoredSearchAddress {
            agent,
            team,
            chat_id: destination_chat_id
                .map(|value| value.parse())
                .transpose()
                .map_err(to_sqlite_conversion_error)?,
        },
        template_sha: template_sha
            .map(|value| value.parse())
            .transpose()
            .map_err(to_sqlite_conversion_error)?,
        template_type,
        category,
        match_fields,
    };
    Ok(SearchRecord {
        stored,
        tags: parse_json_or_default(tags_json, Value::Array(Vec::new()))?,
        vars: parse_json_or_default(vars_json, Value::Object(Default::default()))?,
        template_metadata: parse_template_metadata(template_metadata_json)?,
    })
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

fn parse_template_metadata(raw: Option<String>) -> rusqlite::Result<Value> {
    let schema = parse_json_or_default(raw, Value::Object(Default::default()))?;
    Ok(schema
        .as_object()
        .and_then(|schema| schema.get("metadata"))
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default())))
}

fn to_sqlite_conversion_error(error: AtmError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::other(error.to_string())),
    )
}

fn matches_filters(record: &SearchRecord, query: &MessageSearchQuery) -> bool {
    let filters = &query.filters;
    let stored = &record.stored;
    filters
        .team
        .as_ref()
        .is_none_or(|team| team == &stored.key.team)
        && filters
            .agent
            .as_ref()
            .is_none_or(|agent| agent == &stored.key.agent)
        && filters
            .from_agent
            .as_ref()
            .is_none_or(|agent| agent == &stored.from.agent)
        && filters
            .template_sha
            .as_ref()
            .is_none_or(|sha| stored.template_sha.as_ref() == Some(sha))
        && filters
            .category
            .as_ref()
            .is_none_or(|category| stored.category.as_ref() == Some(category))
        && filters.time_range.as_ref().is_none_or(|range| {
            range.since.is_none_or(|since| stored.message_at >= since)
                && range.until.is_none_or(|until| stored.message_at <= until)
        })
        && filters
            .tags
            .iter()
            .all(|tag| json_array_contains(&record.tags, tag))
        && filters
            .vars
            .iter()
            .all(|(key, value)| json_object_value_is(&record.vars, key.as_str(), value.as_str()))
        && filters.template_metadata.iter().all(|(key, value)| {
            json_object_value_is(&record.template_metadata, key.as_str(), value.as_str())
        })
}

fn json_array_contains(values: &Value, expected: &str) -> bool {
    values.as_array().is_some_and(|values| {
        values
            .iter()
            .any(|value| scalar_json_text(value) == expected)
    })
}

fn json_object_value_is(values: &Value, key: &str, expected: &str) -> bool {
    values
        .as_object()
        .and_then(|values| values.get(key))
        .is_some_and(|value| scalar_json_text(value) == expected)
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
