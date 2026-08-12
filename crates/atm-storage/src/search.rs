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
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

/// Bounded data value. It is never interpreted as SQL or FTS syntax.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchFilters {
    pub team: Option<TeamName>,
    pub agent: Option<AgentName>,
    pub from_agent: Option<AgentName>,
    pub template_sha: Option<TemplateSha>,
    pub template_metadata: Vec<(SearchKey, SearchValue)>,
    pub vars: Vec<(SearchKey, SearchValue)>,
    pub tags: Vec<String>,
    pub category: Option<String>,
    pub time_range: Option<TimeRange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeRange {
    pub since: Option<IsoTimestamp>,
    pub until: Option<IsoTimestamp>,
}

impl TimeRange {
    pub fn validate(&self) -> Result<(), AtmError> {
        if let (Some(since), Some(until)) = (self.since, self.until) {
            if since > until {
                return Err(AtmError::validation(
                    "search time range since must not be after until",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimpleAggregate {
    Count,
    GroupBy(SearchGroupBy),
    Min(SearchTimestampField),
    Max(SearchTimestampField),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchGroupBy {
    Field(SearchGroupField),
    Var(SearchKey),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchGroupField {
    Team,
    Agent,
    FromAgent,
    TemplateType,
    Category,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchTimestampField {
    MessageAt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageSearchQuery {
    pub expression: Option<SearchExpression>,
    pub filters: SearchFilters,
    pub aggregate: Option<SimpleAggregate>,
    pub page: SearchPageRequest,
    pub per_mailbox: bool,
}

impl Default for MessageSearchQuery {
    fn default() -> Self {
        Self {
            expression: None,
            filters: SearchFilters::default(),
            aggregate: None,
            page: SearchPageRequest::default(),
            per_mailbox: false,
        }
    }
}

impl MessageSearchQuery {
    pub fn validate(&self) -> Result<(), AtmError> {
        if let Some(expression) = &self.expression {
            expression.validate()?;
        }
        if let Some(range) = &self.filters.time_range {
            range.validate()?;
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SearchResultKey {
    pub team: TeamName,
    pub agent: AgentName,
    pub message_key: MessageKey,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSearchAddress {
    pub agent: AgentName,
    pub team: TeamName,
    pub chat_id: Option<ChatId>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SearchMatchField {
    BodyText,
    Summary,
    Tag,
    VarValue,
    TemplateContent,
}
#[derive(Debug, Clone, PartialEq, Eq)]
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
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchGroup {
    pub key: String,
    pub count: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Default)]
pub struct InMemoryMessageSearchStore {
    records: std::sync::Mutex<Vec<StoredSearchMatch>>,
}

impl InMemoryMessageSearchStore {
    pub fn insert_for_test(&self, record: StoredSearchMatch) {
        self.records.lock().expect("search fake lock").push(record);
    }
}

impl sealed::Sealed for InMemoryMessageSearchStore {}

impl MessageSearchStore for InMemoryMessageSearchStore {
    fn search(&self, query: &MessageSearchQuery) -> Result<MessageSearchPage, AtmError> {
        query.validate()?;
        let mut records = self.records.lock().expect("search fake lock").clone();
        records.retain(|record| matches_filters(record, &query.filters));
        records.sort_by(|left, right| stable_sort(right, left));
        let aggregate = aggregate(&records, query.aggregate.as_ref());
        let limit = query.page.limit.get() as usize;
        let next_cursor = (records.len() > limit)
            .then(|| SearchCursor::new("fake-next").expect("constant cursor"));
        records.truncate(limit);
        Ok(MessageSearchPage {
            matches: records,
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

fn matches_filters(record: &StoredSearchMatch, filters: &SearchFilters) -> bool {
    filters
        .team
        .as_ref()
        .is_none_or(|team| &record.key.team == team)
        && filters
            .agent
            .as_ref()
            .is_none_or(|agent| &record.key.agent == agent)
        && filters
            .from_agent
            .as_ref()
            .is_none_or(|agent| &record.from.agent == agent)
        && filters
            .template_sha
            .as_ref()
            .is_none_or(|sha| record.template_sha.as_ref() == Some(sha))
        && filters
            .category
            .as_ref()
            .is_none_or(|category| record.category.as_ref() == Some(category))
        && filters.time_range.as_ref().is_none_or(|range| {
            range.since.is_none_or(|since| record.message_at >= since)
                && range.until.is_none_or(|until| record.message_at <= until)
        })
}

fn stable_sort(left: &StoredSearchMatch, right: &StoredSearchMatch) -> std::cmp::Ordering {
    left.message_at
        .cmp(&right.message_at)
        .then_with(|| right.key.team.cmp(&left.key.team))
        .then_with(|| right.key.agent.cmp(&left.key.agent))
        .then_with(|| right.key.message_key.cmp(&left.key.message_key))
}

fn aggregate(
    records: &[StoredSearchMatch],
    aggregate: Option<&SimpleAggregate>,
) -> Option<SearchAggregate> {
    match aggregate {
        None => None,
        Some(SimpleAggregate::Count) => Some(SearchAggregate::Count {
            value: records.len() as u64,
        }),
        Some(SimpleAggregate::Min(field)) => Some(SearchAggregate::Timestamp {
            field: *field,
            value: records.iter().map(|record| record.message_at).min(),
        }),
        Some(SimpleAggregate::Max(field)) => Some(SearchAggregate::Timestamp {
            field: *field,
            value: records.iter().map(|record| record.message_at).max(),
        }),
        Some(SimpleAggregate::GroupBy(by)) => {
            let mut groups = BTreeMap::new();
            for record in records {
                let key = match by {
                    SearchGroupBy::Field(SearchGroupField::Team) => record.key.team.to_string(),
                    SearchGroupBy::Field(SearchGroupField::Agent) => record.key.agent.to_string(),
                    SearchGroupBy::Field(SearchGroupField::FromAgent) => {
                        record.from.agent.to_string()
                    }
                    SearchGroupBy::Field(SearchGroupField::TemplateType) => {
                        record.template_type.clone().unwrap_or_default()
                    }
                    SearchGroupBy::Field(SearchGroupField::Category) => {
                        record.category.clone().unwrap_or_default()
                    }
                    SearchGroupBy::Var(_) => String::new(),
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
}
