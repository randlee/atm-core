//! Backend-neutral typed message-search capability.
//!
//! This module intentionally contains no FTS syntax, SQL, renderer handle, or
//! HTTP DTO.  The concrete storage adapter owns compilation into its local
//! search engine; callers can only construct validated data values.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::contract::{MessageKey, sealed};
use crate::error::AtmError;
use crate::types::{AgentName, ChatId, IsoTimestamp, TeamName, TemplateSha};

/// One bounded literal term in a typed search expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchAtom {
    Term(String),
    Phrase(String),
}

impl SearchAtom {
    pub fn term(value: impl Into<String>) -> Result<Self, AtmError> {
        Self::validated(value.into()).map(Self::Term)
    }

    pub fn phrase(value: impl Into<String>) -> Result<Self, AtmError> {
        Self::validated(value.into()).map(Self::Phrase)
    }

    fn validated(value: String) -> Result<String, AtmError> {
        if value.is_empty() || value.len() > 256 {
            return Err(AtmError::validation(
                "search atoms must contain 1 through 256 UTF-8 bytes",
            ));
        }
        Ok(value)
    }

    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Term(value) | Self::Phrase(value) => value,
        }
    }
}

/// A bounded expression tree.  Call [`SearchExpression::validate`] at the
/// storage boundary so manually assembled public DTOs cannot bypass limits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchExpression {
    Atom(SearchAtom),
    All(Vec<SearchExpression>),
    Any(Vec<SearchExpression>),
    Not(Box<SearchExpression>),
    Near {
        terms: Vec<SearchAtom>,
        max_distance: u8,
    },
}

impl SearchExpression {
    pub fn validate(&self) -> Result<(), AtmError> {
        let mut nodes = 0_usize;
        self.validate_inner(1, &mut nodes, false)
    }

    fn validate_inner(
        &self,
        depth: usize,
        nodes: &mut usize,
        inside_all_with_positive_sibling: bool,
    ) -> Result<(), AtmError> {
        if depth > 8 {
            return Err(AtmError::validation(
                "search expression exceeds depth limit of 8",
            ));
        }
        *nodes += 1;
        if *nodes > 64 {
            return Err(AtmError::validation(
                "search expression exceeds node limit of 64",
            ));
        }
        match self {
            Self::Atom(atom) => {
                let _ = SearchAtom::validated(atom.text().to_owned())?;
            }
            Self::All(children) => {
                if children.is_empty() {
                    return Err(AtmError::validation("search All group must not be empty"));
                }
                let positives = children
                    .iter()
                    .filter(|child| !matches!(child, Self::Not(_)))
                    .count();
                for child in children {
                    child.validate_inner(depth + 1, nodes, positives > 0)?;
                }
            }
            Self::Any(children) => {
                if children.is_empty() {
                    return Err(AtmError::validation("search Any group must not be empty"));
                }
                for child in children {
                    if matches!(child, Self::Not(_)) {
                        return Err(AtmError::validation("search Any group cannot contain Not"));
                    }
                    child.validate_inner(depth + 1, nodes, false)?;
                }
            }
            Self::Not(child) => {
                if !inside_all_with_positive_sibling {
                    return Err(AtmError::validation(
                        "search Not is valid only inside All with a positive sibling",
                    ));
                }
                child.validate_inner(depth + 1, nodes, false)?;
            }
            Self::Near {
                terms,
                max_distance,
            } => {
                if !(2..=8).contains(&terms.len()) {
                    return Err(AtmError::validation(
                        "search Near requires 2 through 8 atoms",
                    ));
                }
                if !(1..=16).contains(max_distance) {
                    return Err(AtmError::validation(
                        "search Near distance must be in the inclusive range 1..=16",
                    ));
                }
                for atom in terms {
                    let _ = SearchAtom::validated(atom.text().to_owned())?;
                }
            }
        }
        Ok(())
    }
}

/// Validated JSON-object key used in template metadata and variable filters.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SearchKey(String);

impl SearchKey {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        let bytes = value.as_bytes();
        let valid = (1..=64).contains(&bytes.len())
            && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
            && bytes[1..]
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
        if !valid {
            return Err(AtmError::validation(
                "search key must match ASCII ^[A-Za-z_][A-Za-z0-9_-]{0,63}$",
            ));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for SearchKey {
    type Err = AtmError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for SearchKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// Bounded data value. It is never interpreted as SQL or FTS syntax.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct SearchValue(String);

impl SearchValue {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.len() > 4096 {
            return Err(AtmError::validation("search value exceeds the 4 KiB limit"));
        }
        Ok(Self(value))
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for SearchValue {
    type Err = AtmError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for SearchValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// One parameterized metadata-match mode exposed by the public query API.
///
/// Prefix semantics are intentionally limited to template metadata. Variable
/// filters stay exact, which prevents a convenience wildcard from becoming a
/// second query language over JSON1 paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchMetadataMatch {
    Exact(SearchValue),
    Prefix(SearchValue),
}

impl SearchMetadataMatch {
    pub fn exact(value: impl Into<String>) -> Result<Self, AtmError> {
        Ok(Self::Exact(SearchValue::new(value)?))
    }

    pub fn prefix(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = SearchValue::new(value)?;
        if value.as_str().is_empty() {
            return Err(AtmError::validation(
                "search metadata prefix must not be empty",
            ));
        }
        Ok(Self::Prefix(value))
    }

    #[must_use]
    pub fn value(&self) -> &SearchValue {
        match self {
            Self::Exact(value) | Self::Prefix(value) => value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct SearchLimit(u32);

impl SearchLimit {
    pub const DEFAULT: Self = Self(50);
    pub fn new(value: u32) -> Result<Self, AtmError> {
        if !(1..=200).contains(&value) {
            return Err(AtmError::validation(
                "search limit must be in the inclusive range 1..=200",
            ));
        }
        Ok(Self(value))
    }
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for SearchLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(u32::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct SearchCursor(String);

impl SearchCursor {
    pub fn new(value: impl Into<String>) -> Result<Self, AtmError> {
        let value = value.into();
        if value.is_empty() || value.len() > 2048 {
            return Err(AtmError::validation(
                "search cursor must contain 1 through 2048 bytes",
            ));
        }
        Ok(Self(value))
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SearchCursor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchPageRequest {
    pub limit: SearchLimit,
    pub cursor: Option<SearchCursor>,
}

impl Default for SearchPageRequest {
    fn default() -> Self {
        Self {
            limit: SearchLimit::DEFAULT,
            cursor: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SearchFilters {
    pub team: Option<TeamName>,
    pub agent: Option<AgentName>,
    pub from_agent: Option<AgentName>,
    pub template_sha: Option<TemplateSha>,
    pub template_metadata: Vec<(SearchKey, SearchMetadataMatch)>,
    pub vars: Vec<(SearchKey, SearchValue)>,
    pub tags: Vec<String>,
    pub category: Option<String>,
    pub time_range: Option<TimeRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    pub since: Option<IsoTimestamp>,
    pub until: Option<IsoTimestamp>,
}

impl TimeRange {
    pub fn validate(&self) -> Result<(), AtmError> {
        if let (Some(since), Some(until)) = (self.since, self.until)
            && since > until
        {
            return Err(AtmError::validation(
                "search time range since must not be after until",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimpleAggregate {
    Count,
    GroupBy(SearchGroupBy),
    Min(SearchTimestampField),
    Max(SearchTimestampField),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchGroupBy {
    Field(SearchGroupField),
    Var(SearchKey),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchGroupField {
    Team,
    Agent,
    FromAgent,
    TemplateType,
    Category,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchTimestampField {
    MessageAt,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MessageSearchQuery {
    pub expression: Option<SearchExpression>,
    pub filters: SearchFilters,
    pub aggregate: Option<SimpleAggregate>,
    pub page: SearchPageRequest,
    pub per_mailbox: bool,
}

impl MessageSearchQuery {
    pub fn validate(&self) -> Result<(), AtmError> {
        if let Some(expression) = &self.expression {
            expression.validate()?;
        }
        if let Some(range) = &self.filters.time_range {
            range.validate()?;
        }
        // These values can enter through the serialized HTTP contract, so
        // validate their public invariants again at the storage boundary.
        let _ = SearchLimit::new(self.page.limit.get())?;
        if let Some(cursor) = &self.page.cursor {
            let _ = SearchCursor::new(cursor.as_str())?;
        }
        if let Some(template_sha) = &self.filters.template_sha {
            let _ = TemplateSha::new(template_sha.as_str())?;
        }
        for (key, value) in &self.filters.template_metadata {
            let _ = SearchKey::new(key.as_str())?;
            match value {
                SearchMetadataMatch::Exact(value) => {
                    let _ = SearchValue::new(value.as_str())?;
                }
                SearchMetadataMatch::Prefix(value) => {
                    let _ = SearchMetadataMatch::prefix(value.as_str())?;
                }
            }
        }
        for (key, value) in &self.filters.vars {
            let _ = SearchKey::new(key.as_str())?;
            let _ = SearchValue::new(value.as_str())?;
        }
        for tag in &self.filters.tags {
            if tag.len() > 4096 {
                return Err(AtmError::validation("search tag exceeds the 4 KiB limit"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchDeadline {
    remaining: Duration,
}
impl SearchDeadline {
    pub fn new(remaining: Duration) -> Result<Self, AtmError> {
        if remaining.is_zero() {
            return Err(AtmError::validation("search deadline must be non-zero"));
        }
        Ok(Self { remaining })
    }
    #[must_use]
    pub const fn remaining(self) -> Duration {
        self.remaining
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SearchResultKey {
    pub team: TeamName,
    pub agent: AgentName,
    pub message_key: MessageKey,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSearchAddress {
    pub agent: AgentName,
    pub team: TeamName,
    pub chat_id: Option<ChatId>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SearchMatchField {
    BodyText,
    Summary,
    Tag,
    VarValue,
    FromAgent,
    TemplateContent,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSearchMatch {
    pub key: SearchResultKey,
    pub message_id: Option<String>,
    pub message_at: IsoTimestamp,
    pub from: StoredSearchAddress,
    pub to: StoredSearchAddress,
    pub template_sha: Option<TemplateSha>,
    pub template_type: Option<String>,
    pub category: Option<String>,
    pub match_fields: Vec<SearchMatchField>,
    /// Backend-produced context for indexed text. Filter-only matches leave
    /// this absent, so callers can render honest metadata-only results.
    pub snippet: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchGroup {
    pub key: String,
    pub count: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchAggregate {
    Count {
        value: u64,
    },
    Groups {
        by: SearchGroupBy,
        groups: Vec<SearchGroup>,
    },
    Timestamp {
        field: SearchTimestampField,
        value: Option<IsoTimestamp>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSearchPage {
    pub matches: Vec<StoredSearchMatch>,
    pub aggregate: Option<SearchAggregate>,
    pub next_cursor: Option<SearchCursor>,
}

/// Fixed, backend-controlled search capability. `atm-storage-rusqlite` owns
/// the production FTS implementation; test contracts use the allowlisted fake.
pub trait MessageSearchStore: sealed::Sealed + Send + Sync {
    fn search(&self, query: &MessageSearchQuery) -> Result<MessageSearchPage, AtmError>;
}

/// Tokio-safe companion for the same semantic capability.
#[async_trait::async_trait]
pub trait AsyncMessageSearchStore: MessageSearchStore {
    async fn search_async(
        &self,
        query: MessageSearchQuery,
        deadline: SearchDeadline,
    ) -> Result<MessageSearchPage, AtmError>;
}

/// Complete fixture document accepted by the authorized in-memory search fake.
///
/// This keeps the fake on the same typed expression/filter/aggregate contract
/// as a production adapter without exposing a backend query language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InMemorySearchDocument {
    pub stored: StoredSearchMatch,
    pub body_text: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub vars: BTreeMap<SearchKey, SearchValue>,
    pub template_metadata: BTreeMap<SearchKey, SearchValue>,
    pub template_content: String,
}

impl From<StoredSearchMatch> for InMemorySearchDocument {
    fn from(stored: StoredSearchMatch) -> Self {
        Self {
            stored,
            body_text: String::new(),
            summary: String::new(),
            tags: Vec::new(),
            vars: BTreeMap::new(),
            template_metadata: BTreeMap::new(),
            template_content: String::new(),
        }
    }
}

#[derive(Default)]
pub struct InMemoryMessageSearchStore {
    records: std::sync::Mutex<Vec<InMemorySearchDocument>>,
    calls: std::sync::atomic::AtomicUsize,
}

impl InMemoryMessageSearchStore {
    pub fn insert_for_test(&self, record: StoredSearchMatch) {
        self.insert_document_for_test(record.into());
    }

    pub fn insert_document_for_test(&self, document: InMemorySearchDocument) {
        self.records
            .lock()
            .expect("search fake lock")
            .push(document);
    }

    #[doc(hidden)]
    #[must_use]
    pub fn search_calls_for_test(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl sealed::Sealed for InMemoryMessageSearchStore {}

impl MessageSearchStore for InMemoryMessageSearchStore {
    fn search(&self, query: &MessageSearchQuery) -> Result<MessageSearchPage, AtmError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        query.validate()?;
        let cursor = query
            .page
            .cursor
            .as_ref()
            .map(|cursor| decode_fake_cursor(cursor, query))
            .transpose()?;
        let mut records = self.records.lock().expect("search fake lock").clone();
        records.retain(|record| matches_filters(record, &query.filters));
        if let Some(expression) = &query.expression {
            records.retain(|record| expression_matches_document(record, expression));
            for record in &mut records {
                record.stored.match_fields = expression_match_fields(record, expression);
            }
        }
        records.sort_by(|left, right| stable_compare(&left.stored, &right.stored));
        if !query.per_mailbox {
            // Deduplicate the complete stable result set before applying a
            // cursor. Otherwise the duplicate skipped on page one can become
            // the first record on page two.
            deduplicate(&mut records);
        }
        if let Some(cursor) = cursor.as_ref() {
            records.retain(|record| after_fake_cursor(&record.stored, cursor));
        }
        let aggregate = aggregate(&records, query.aggregate.as_ref());
        let limit = query.page.limit.get() as usize;
        let next_cursor = (records.len() > limit)
            .then(|| encode_fake_cursor(&records[limit - 1].stored, query))
            .transpose()?;
        records.truncate(limit);
        Ok(MessageSearchPage {
            matches: records.into_iter().map(|record| record.stored).collect(),
            aggregate,
            next_cursor,
        })
    }
}

#[async_trait::async_trait]
impl AsyncMessageSearchStore for InMemoryMessageSearchStore {
    async fn search_async(
        &self,
        query: MessageSearchQuery,
        deadline: SearchDeadline,
    ) -> Result<MessageSearchPage, AtmError> {
        if deadline.remaining().is_zero() {
            return Err(AtmError::validation("search deadline must be non-zero"));
        }
        self.search(&query)
    }
}

fn matches_filters(record: &InMemorySearchDocument, filters: &SearchFilters) -> bool {
    let stored = &record.stored;
    filters
        .team
        .as_ref()
        .is_none_or(|team| &stored.key.team == team)
        && filters
            .agent
            .as_ref()
            .is_none_or(|agent| &stored.key.agent == agent)
        && filters
            .from_agent
            .as_ref()
            .is_none_or(|agent| &stored.from.agent == agent)
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
        && filters.tags.iter().all(|tag| record.tags.contains(tag))
        && filters
            .vars
            .iter()
            .all(|(key, value)| record.vars.get(key) == Some(value))
        && filters
            .template_metadata
            .iter()
            .all(|(key, matcher)| match matcher {
                SearchMetadataMatch::Exact(value) => {
                    record.template_metadata.get(key) == Some(value)
                }
                SearchMetadataMatch::Prefix(value) => record
                    .template_metadata
                    .get(key)
                    .is_some_and(|actual| actual.as_str().starts_with(value.as_str())),
            })
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
    records: &[InMemorySearchDocument],
    aggregate: Option<&SimpleAggregate>,
) -> Option<SearchAggregate> {
    match aggregate {
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
            let mut groups = BTreeMap::new();
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
                        .get(key)
                        .map(|value| value.as_str().to_owned())
                        .unwrap_or_default(),
                };
                *groups.entry(key).or_insert(0_u64) += 1;
            }
            Some(SearchAggregate::Groups {
                by: by.clone(),
                groups: groups
                    .into_iter()
                    .map(|(key, count)| SearchGroup { key, count })
                    .collect(),
            })
        }
    }
}

fn expression_match_fields(
    record: &InMemorySearchDocument,
    expression: &SearchExpression,
) -> Vec<SearchMatchField> {
    let mut fields = Vec::new();
    collect_positive_match_fields(record, expression, &mut fields);
    fields.sort();
    fields.dedup();
    fields
}

fn collect_positive_match_fields(
    record: &InMemorySearchDocument,
    expression: &SearchExpression,
    matched: &mut Vec<SearchMatchField>,
) {
    match expression {
        SearchExpression::Atom(atom) => {
            for (field, text) in searchable_fields(record) {
                if text.to_lowercase().contains(&atom.text().to_lowercase()) {
                    matched.push(field);
                }
            }
        }
        SearchExpression::All(children) | SearchExpression::Any(children) => {
            for child in children {
                collect_positive_match_fields(record, child, matched);
            }
        }
        SearchExpression::Not(_) => {}
        SearchExpression::Near {
            terms,
            max_distance,
        } => {
            for (field, text) in searchable_fields(record) {
                if near_matches_text(&text, terms, *max_distance) {
                    matched.push(field);
                }
            }
        }
    }
}

fn searchable_fields(record: &InMemorySearchDocument) -> Vec<(SearchMatchField, String)> {
    vec![
        (SearchMatchField::BodyText, record.body_text.clone()),
        (SearchMatchField::Summary, record.summary.clone()),
        (SearchMatchField::Tag, record.tags.join(" ")),
        (
            SearchMatchField::VarValue,
            record
                .vars
                .values()
                .map(|value| value.as_str())
                .collect::<Vec<_>>()
                .join(" "),
        ),
        (
            SearchMatchField::FromAgent,
            record.stored.from.agent.to_string(),
        ),
        (
            SearchMatchField::TemplateContent,
            record.template_content.clone(),
        ),
    ]
}

fn searchable_message_fields(record: &InMemorySearchDocument) -> Vec<(SearchMatchField, String)> {
    searchable_fields(record)
        .into_iter()
        .filter(|(field, _)| *field != SearchMatchField::TemplateContent)
        .collect()
}

fn expression_matches_document(
    record: &InMemorySearchDocument,
    expression: &SearchExpression,
) -> bool {
    // SQLite keeps message and template FTS projections separate. Preserve that
    // rule in the fake: boolean terms may not be satisfied across projections.
    expression_matches_fields(expression, &searchable_message_fields(record))
        || expression_matches_fields(
            expression,
            &[(
                SearchMatchField::TemplateContent,
                record.template_content.clone(),
            )],
        )
}

fn expression_matches_fields(
    expression: &SearchExpression,
    fields: &[(SearchMatchField, String)],
) -> bool {
    match expression {
        SearchExpression::Atom(atom) => fields
            .iter()
            .any(|(_, text)| text.to_lowercase().contains(&atom.text().to_lowercase())),
        SearchExpression::All(children) => children
            .iter()
            .all(|child| expression_matches_fields(child, fields)),
        SearchExpression::Any(children) => children
            .iter()
            .any(|child| expression_matches_fields(child, fields)),
        SearchExpression::Not(child) => !expression_matches_fields(child, fields),
        SearchExpression::Near {
            terms,
            max_distance,
        } => fields
            .iter()
            .any(|(_, text)| near_matches_text(text, terms, *max_distance)),
    }
}

fn near_matches_text(text: &str, terms: &[SearchAtom], max_distance: u8) -> bool {
    let tokens = text
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    let positions = terms
        .iter()
        .map(|term| {
            let needle = term.text().to_lowercase();
            tokens.iter().position(|token| token == &needle)
        })
        .collect::<Option<Vec<_>>>();
    let Some(positions) = positions else {
        return false;
    };
    let first = *positions
        .iter()
        .min()
        .expect("terms are validated non-empty");
    let last = *positions
        .iter()
        .max()
        .expect("terms are validated non-empty");
    last.saturating_sub(first)
        .saturating_sub(positions.len().saturating_sub(1))
        <= usize::from(max_distance)
}

#[derive(Serialize, Deserialize)]
struct FakeCursorTuple(String, String, String, String, String);

fn encode_fake_cursor(
    record: &StoredSearchMatch,
    query: &MessageSearchQuery,
) -> Result<SearchCursor, AtmError> {
    SearchCursor::new(
        serde_json::to_string(&FakeCursorTuple(
            query_signature(query),
            record.message_at.to_string(),
            record.key.team.to_string(),
            record.key.agent.to_string(),
            record.key.message_key.to_string(),
        ))
        .expect("fake cursor tuple serializes"),
    )
}

fn decode_fake_cursor(
    cursor: &SearchCursor,
    query: &MessageSearchQuery,
) -> Result<FakeCursorTuple, AtmError> {
    let tuple = serde_json::from_str::<FakeCursorTuple>(cursor.as_str()).map_err(|_| {
        AtmError::validation("search cursor is malformed or belongs to a different query")
    })?;
    if tuple.0 != query_signature(query) {
        return Err(AtmError::validation(
            "search cursor is malformed or belongs to a different query",
        ));
    }
    Ok(tuple)
}

fn after_fake_cursor(record: &StoredSearchMatch, cursor: &FakeCursorTuple) -> bool {
    cursor
        .1
        .cmp(&record.message_at.to_string())
        .then_with(|| record.key.team.to_string().cmp(&cursor.2))
        .then_with(|| record.key.agent.to_string().cmp(&cursor.3))
        .then_with(|| record.key.message_key.to_string().cmp(&cursor.4))
        .is_gt()
}

fn query_signature(query: &MessageSearchQuery) -> String {
    let mut signature_query = query.clone();
    signature_query.page.cursor = None;
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in format!("{signature_query:?}").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn deduplicate(records: &mut Vec<InMemorySearchDocument>) {
    let mut seen = std::collections::BTreeSet::new();
    records.retain(|record| {
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

impl fmt::Display for SearchKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn fixture(key: &str) -> StoredSearchMatch {
        let team: TeamName = "test-team".parse().expect("team");
        let agent: AgentName = "test-agent".parse().expect("agent");
        StoredSearchMatch {
            key: SearchResultKey {
                team: team.clone(),
                agent: agent.clone(),
                message_key: MessageKey::new(key).expect("key"),
            },
            message_id: None,
            message_at: IsoTimestamp::from_datetime(Utc::now()),
            from: StoredSearchAddress {
                agent: agent.clone(),
                team: team.clone(),
                chat_id: None,
            },
            to: StoredSearchAddress {
                agent,
                team,
                chat_id: None,
            },
            template_sha: None,
            template_type: None,
            category: Some("assignment".to_owned()),
            match_fields: vec![SearchMatchField::BodyText],
            snippet: None,
        }
    }

    #[test]
    fn validation_rejects_all_known_untrusted_search_shapes() {
        for invalid in ["", "0bad", "a.b", "a[b]", "a/b", &"x".repeat(65)] {
            assert!(SearchKey::new(invalid).is_err(), "{invalid:?} must reject");
        }
        assert!(SearchValue::new("x".repeat(4097)).is_err());
        assert!(SearchLimit::new(0).is_err());
        assert!(SearchLimit::new(201).is_err());
        assert!(
            SearchExpression::Not(Box::new(SearchExpression::Atom(
                SearchAtom::term("x").expect("atom")
            )))
            .validate()
            .is_err()
        );
        assert!(
            SearchExpression::Any(vec![SearchExpression::Not(Box::new(
                SearchExpression::Atom(SearchAtom::term("x").expect("atom"))
            ))])
            .validate()
            .is_err()
        );
        assert!(
            SearchExpression::Near {
                terms: vec![SearchAtom::term("x").expect("atom")],
                max_distance: 1
            }
            .validate()
            .is_err()
        );
        assert!(SearchAtom::phrase("x".repeat(257)).is_err());
        assert!(
            SearchExpression::All(vec![SearchExpression::Not(Box::new(
                SearchExpression::Atom(SearchAtom::term("x").expect("atom")),
            ))])
            .validate()
            .is_err()
        );
        let mut depth = SearchExpression::Atom(SearchAtom::term("x").expect("atom"));
        for _ in 0..8 {
            depth = SearchExpression::All(vec![depth]);
        }
        assert!(depth.validate().is_err());
        assert!(
            SearchExpression::Any(
                (0..65)
                    .map(|_| SearchExpression::Atom(SearchAtom::term("x").expect("atom")))
                    .collect(),
            )
            .validate()
            .is_err()
        );
    }

    #[test]
    fn in_memory_contract_honors_filters_sort_and_count_aggregate() {
        let store = InMemoryMessageSearchStore::default();
        store.insert_for_test(fixture("atm:one"));
        store.insert_for_test(fixture("atm:two"));
        let query = MessageSearchQuery {
            aggregate: Some(SimpleAggregate::Count),
            ..MessageSearchQuery::default()
        };
        let page = store.search(&query).expect("fake search");
        assert_eq!(page.matches.len(), 2);
        assert_eq!(page.aggregate, Some(SearchAggregate::Count { value: 2 }));
    }

    #[test]
    fn in_memory_contract_applies_typed_expression_structured_filters_and_var_grouping() {
        let store = InMemoryMessageSearchStore::default();
        let mut accepted = InMemorySearchDocument::from(fixture("atm:accepted"));
        accepted.body_text = "urgent release task".to_owned();
        accepted.tags = vec!["phase-an".to_owned()];
        accepted.vars.insert(
            SearchKey::new("sprint").expect("key"),
            SearchValue::new("AN.5").expect("value"),
        );
        accepted.template_metadata.insert(
            SearchKey::new("kind").expect("key"),
            SearchValue::new("assignment").expect("value"),
        );
        store.insert_document_for_test(accepted);

        let mut excluded = InMemorySearchDocument::from(fixture("atm:excluded"));
        excluded.body_text = "urgent old task".to_owned();
        excluded.tags = vec!["phase-an".to_owned()];
        excluded.vars.insert(
            SearchKey::new("sprint").expect("key"),
            SearchValue::new("AN.5").expect("value"),
        );
        excluded.template_metadata.insert(
            SearchKey::new("kind").expect("key"),
            SearchValue::new("assignment").expect("value"),
        );
        store.insert_document_for_test(excluded);

        let sprint = SearchKey::new("sprint").expect("key");
        let query = MessageSearchQuery {
            expression: Some(SearchExpression::All(vec![
                SearchExpression::Atom(SearchAtom::term("urgent").expect("atom")),
                SearchExpression::Not(Box::new(SearchExpression::Atom(
                    SearchAtom::term("old").expect("atom"),
                ))),
            ])),
            filters: SearchFilters {
                tags: vec!["phase-an".to_owned()],
                vars: vec![(sprint.clone(), SearchValue::new("AN.5").expect("value"))],
                template_metadata: vec![(
                    SearchKey::new("kind").expect("key"),
                    SearchMetadataMatch::exact("assignment").expect("value"),
                )],
                ..SearchFilters::default()
            },
            aggregate: Some(SimpleAggregate::GroupBy(SearchGroupBy::Var(sprint))),
            ..MessageSearchQuery::default()
        };
        let page = store.search(&query).expect("fake search");
        assert_eq!(page.matches.len(), 1);
        assert_eq!(page.matches[0].key.message_key.as_str(), "atm:accepted");
        assert_eq!(
            page.matches[0].match_fields,
            vec![SearchMatchField::BodyText]
        );
        assert_eq!(
            page.aggregate,
            Some(SearchAggregate::Groups {
                by: SearchGroupBy::Var(SearchKey::new("sprint").expect("key")),
                groups: vec![SearchGroup {
                    key: "AN.5".to_owned(),
                    count: 1,
                }],
            })
        );
    }

    #[test]
    fn in_memory_contract_uses_the_frozen_cursor_and_dedup_rules() {
        let store = InMemoryMessageSearchStore::default();
        let timestamp: IsoTimestamp = "2026-08-12T00:00:00Z".parse().expect("timestamp");
        for (key, team, message_id) in [
            ("atm:first", "a-team", Some("same-id")),
            ("atm:duplicate", "b-team", Some("same-id")),
            ("atm:third", "c-team", None),
        ] {
            let mut record = fixture(key);
            record.message_at = timestamp;
            record.key.team = team.parse().expect("team");
            record.to.team = record.key.team.clone();
            record.from.team = record.key.team.clone();
            record.message_id = message_id.map(str::to_owned);
            store.insert_document_for_test(InMemorySearchDocument {
                body_text: "cursor needle".to_owned(),
                ..record.into()
            });
        }

        let mut query = MessageSearchQuery {
            expression: Some(SearchExpression::Atom(
                SearchAtom::term("needle").expect("atom"),
            )),
            page: SearchPageRequest {
                limit: SearchLimit::new(1).expect("limit"),
                cursor: None,
            },
            ..MessageSearchQuery::default()
        };
        let first = store.search(&query).expect("first page");
        assert_eq!(first.matches[0].key.team.as_str(), "a-team");
        query.page.cursor = first.next_cursor;
        let second = store.search(&query).expect("second page");
        assert_eq!(second.matches[0].key.team.as_str(), "c-team");
        assert!(second.next_cursor.is_none());

        query.per_mailbox = true;
        query.page.cursor = None;
        let first_mailbox = store.search(&query).expect("first mailbox page");
        query.page.cursor = first_mailbox.next_cursor;
        let second_mailbox = store.search(&query).expect("second mailbox page");
        assert_eq!(second_mailbox.matches[0].key.team.as_str(), "b-team");
    }
}
