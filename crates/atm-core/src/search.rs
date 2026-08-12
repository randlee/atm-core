//! Transport-neutral search application contract and public query grammar.
//!
//! Parsing belongs here so every client-facing surface produces the same
//! bounded `atm-storage` AST.  SQLite/FTS syntax intentionally never crosses
//! this boundary.

use std::str::FromStr;

use atm_storage::{
    AgentName, AsyncMessageSearchStore, AtmError, IsoTimestamp, MessageSearchPage,
    MessageSearchQuery, SearchAggregate, SearchAtom, SearchCursor, SearchDeadline,
    SearchExpression, SearchFilters, SearchGroupBy, SearchLimit, SearchMetadataMatch,
    SearchPageRequest, SearchResultKey, SearchTimestampField, SearchValue, SimpleAggregate,
    StoredSearchAddress, StoredSearchMatch, TeamName, TemplateSha, TimeRange,
};
use serde::{Deserialize, Serialize};

use crate::api::{AuthenticatedIngress, RequestDeadline};

/// Core-owned request shared by CLI and HTTP adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchRequest {
    /// Transport-neutral public input.  It is compiled exactly once by core,
    /// so CLI and direct HTTP callers share the same bounded grammar rather
    /// than HTTP accepting a second, serialized storage language.
    pub query: SearchInput,
}

/// Core-owned result shared by CLI and HTTP adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    pub aggregate: Option<SearchAggregate>,
    pub next_cursor: Option<SearchCursor>,
}

/// A rendered projection of one storage match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHit {
    pub key: SearchResultKey,
    pub message_id: Option<String>,
    pub message_at: IsoTimestamp,
    /// Sender identity, including team and optional chat identity.
    pub from_agent: StoredSearchAddress,
    /// Recipient mailbox identity, including team and optional chat identity.
    pub to_agent: StoredSearchAddress,
    pub template_type: Option<String>,
    pub category: Option<String>,
    pub snippet: String,
}

/// Public primitive values decoded by either the CLI or HTTP adapter.
///
/// This deliberately has no clap, HTTP, SQLite, or FTS dependency. The two
/// outer adapters construct it and call [`SearchInput::compile`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchInput {
    pub text: Option<String>,
    pub raw_match: bool,
    pub template_meta: Vec<String>,
    pub vars: Vec<String>,
    pub tags: Vec<String>,
    pub category: Option<String>,
    pub from: Option<String>,
    pub team: Option<String>,
    pub agent: Option<String>,
    pub template_sha: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    pub per_mailbox: bool,
    pub aggregate: Option<SearchAggregateInput>,
}

/// One deliberately small aggregation selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchAggregateInput {
    Count,
    GroupBy(String),
    MinMessageAt,
    MaxMessageAt,
}

impl SearchInput {
    /// Compiles public primitives to AN.5's backend-neutral storage DTO.
    pub fn compile_query(&self) -> Result<MessageSearchQuery, AtmError> {
        let expression = self
            .text
            .as_deref()
            .map(|value| parse_search_expression(value, self.raw_match))
            .transpose()?;
        let template_metadata = self
            .template_meta
            .iter()
            .map(|value| parse_metadata_match(value))
            .collect::<Result<Vec<_>, _>>()?;
        let filters = SearchFilters {
            team: self.team.as_deref().map(TeamName::from_str).transpose()?,
            agent: self.agent.as_deref().map(AgentName::from_str).transpose()?,
            from_agent: self.from.as_deref().map(AgentName::from_str).transpose()?,
            template_sha: self
                .template_sha
                .as_deref()
                .map(TemplateSha::from_str)
                .transpose()?,
            template_metadata,
            vars: self
                .vars
                .iter()
                .map(|value| parse_key_value(value, "variable"))
                .collect::<Result<Vec<_>, _>>()?,
            tags: self.tags.clone(),
            category: self.category.clone(),
            time_range: match (&self.since, &self.until) {
                (None, None) => None,
                (since, until) => Some(TimeRange {
                    since: since
                        .as_deref()
                        .map(IsoTimestamp::from_str)
                        .transpose()
                        .map_err(|error| {
                            AtmError::validation(format!(
                                "search --since must be RFC 3339: {error}"
                            ))
                        })?,
                    until: until
                        .as_deref()
                        .map(IsoTimestamp::from_str)
                        .transpose()
                        .map_err(|error| {
                            AtmError::validation(format!(
                                "search --until must be RFC 3339: {error}"
                            ))
                        })?,
                }),
            },
        };
        let query = MessageSearchQuery {
            expression,
            filters,
            aggregate: self
                .aggregate
                .as_ref()
                .map(SearchAggregateInput::compile)
                .transpose()?,
            page: SearchPageRequest {
                limit: self
                    .limit
                    .map(SearchLimit::new)
                    .transpose()?
                    .unwrap_or(SearchLimit::DEFAULT),
                cursor: self.cursor.clone().map(SearchCursor::new).transpose()?,
            },
            per_mailbox: self.per_mailbox,
        };
        query.validate()?;
        Ok(query)
    }

    /// Wraps this public input for a CLI or HTTP transport hop.
    #[must_use]
    pub fn into_request(self) -> SearchRequest {
        SearchRequest { query: self }
    }
}

impl SearchRequest {
    /// Compiles the shared public input immediately before storage selection.
    pub fn compile_query(&self) -> Result<MessageSearchQuery, AtmError> {
        self.query.compile_query()
    }
}

impl SearchAggregateInput {
    fn compile(&self) -> Result<SimpleAggregate, AtmError> {
        match self {
            Self::Count => Ok(SimpleAggregate::Count),
            Self::GroupBy(value) => {
                let by = parse_group_by(value)?;
                Ok(SimpleAggregate::GroupBy(by))
            }
            Self::MinMessageAt => Ok(SimpleAggregate::Min(SearchTimestampField::MessageAt)),
            Self::MaxMessageAt => Ok(SimpleAggregate::Max(SearchTimestampField::MessageAt)),
        }
    }
}

/// Executes a local-only query through the Tokio-safe storage capability.
pub async fn execute_search(
    ingress: AuthenticatedIngress,
    request: SearchRequest,
    store: &(dyn AsyncMessageSearchStore + Send + Sync),
    deadline: RequestDeadline,
) -> Result<SearchResponse, AtmError> {
    if ingress != AuthenticatedIngress::Local {
        return Err(AtmError::search_local_only());
    }
    let query = request.compile_query()?;
    let remaining = deadline.remaining().ok_or_else(|| {
        AtmError::daemon_unavailable("request deadline expired before local search execution")
    })?;
    let page = store
        .search_async(query, SearchDeadline::new(remaining)?)
        .await?;
    Ok(response_from_page(page))
}

/// Converts a storage result to the stable public hit projection.
#[must_use]
pub fn response_from_page(page: MessageSearchPage) -> SearchResponse {
    SearchResponse {
        hits: page.matches.into_iter().map(hit_from_match).collect(),
        aggregate: page.aggregate,
        next_cursor: page.next_cursor,
    }
}

fn hit_from_match(record: StoredSearchMatch) -> SearchHit {
    let snippet = record.snippet.unwrap_or_else(|| {
        if record.match_fields.is_empty() {
            "metadata match".to_owned()
        } else {
            record
                .match_fields
                .iter()
                .map(|field| format!("{field:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    });
    SearchHit {
        key: record.key,
        message_id: record.message_id,
        message_at: record.message_at,
        from_agent: record.from,
        to_agent: record.to,
        template_type: record.template_type,
        category: record.category,
        snippet,
    }
}

fn parse_key_value(
    value: &str,
    label: &str,
) -> Result<(atm_storage::SearchKey, SearchValue), AtmError> {
    let (key, value) = value
        .split_once('=')
        .ok_or_else(|| AtmError::validation(format!("{label} filter must use KEY=VALUE")))?;
    Ok((key.parse()?, SearchValue::new(value)?))
}

fn parse_metadata_match(
    value: &str,
) -> Result<(atm_storage::SearchKey, SearchMetadataMatch), AtmError> {
    let (key, value) = value
        .split_once('=')
        .ok_or_else(|| AtmError::validation("template metadata filter must use KEY=VALUE"))?;
    let matcher = if let Some(prefix) = value.strip_suffix('*') {
        SearchMetadataMatch::prefix(prefix)?
    } else {
        SearchMetadataMatch::exact(value)?
    };
    Ok((key.parse()?, matcher))
}

/// Parses an ordinary literal phrase or the documented bounded advanced
/// grammar. The advanced grammar intentionally stays tiny: quoted phrases,
/// words, `NEAR(term term[, distance])`, and `AND`/`OR`/`NOT` are converted
/// to the storage AST; arbitrary FTS5 syntax is never accepted.
pub fn parse_search_expression(value: &str, raw_match: bool) -> Result<SearchExpression, AtmError> {
    if !raw_match {
        return SearchAtom::phrase(value.to_owned()).map(SearchExpression::Atom);
    }
    let tokens = tokenize_advanced(value)?;
    if tokens.is_empty() {
        return Err(AtmError::validation(
            "--raw-match expression must not be blank",
        ));
    }
    parse_or(&tokens)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(String),
    Phrase(String),
    Near(Vec<SearchAtom>, u8),
    And,
    Or,
    Not,
}

fn tokenize_advanced(input: &str) -> Result<Vec<Token>, AtmError> {
    let mut output = Vec::new();
    let characters = input.chars().collect::<Vec<_>>();
    let mut index = 0;
    while let Some(&character) = characters.get(index) {
        if character.is_whitespace() {
            index += 1;
            continue;
        }
        if characters[index..].starts_with(&['N', 'E', 'A', 'R', '(']) {
            let close = characters[index + 5..]
                .iter()
                .position(|character| *character == ')')
                .map(|offset| index + 5 + offset)
                .ok_or_else(|| AtmError::validation("NEAR must use NEAR(term term[, distance])"))?;
            let near = characters[index..=close].iter().collect::<String>();
            output.push(parse_near(&near)?);
            index = close + 1;
            continue;
        }
        if character == '"' {
            index += 1;
            let mut phrase = String::new();
            let mut closed = false;
            while let Some(&character) = characters.get(index) {
                index += 1;
                if character == '"' {
                    closed = true;
                    break;
                }
                phrase.push(character);
            }
            if !closed {
                return Err(AtmError::validation(
                    "unterminated quoted phrase in --raw-match",
                ));
            }
            output.push(Token::Phrase(phrase));
            continue;
        }
        if !character.is_ascii_alphanumeric() && !matches!(character, '_' | '-') {
            return Err(AtmError::validation(
                "unsupported --raw-match syntax; use words, quoted phrases, NEAR(...), AND, OR, and NOT",
            ));
        }
        let mut word = String::new();
        while let Some(&character) = characters.get(index) {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                word.push(character);
                index += 1;
            } else {
                break;
            }
        }
        output.push(match word.as_str() {
            "AND" => Token::And,
            "OR" => Token::Or,
            "NOT" => Token::Not,
            _ => Token::Word(word),
        });
    }
    Ok(output)
}

fn parse_near(input: &str) -> Result<Token, AtmError> {
    let inner = input
        .strip_prefix("NEAR(")
        .and_then(|value| value.strip_suffix(')'))
        .ok_or_else(|| AtmError::validation("NEAR must use NEAR(term term[, distance])"))?;
    let (terms, distance) = match inner.rsplit_once(',') {
        Some((terms, distance)) => (
            terms,
            distance.trim().parse::<u8>().map_err(|_| {
                AtmError::validation(
                    "NEAR distance must be an integer in the inclusive range 1..=16",
                )
            })?,
        ),
        None => (inner, 10),
    };
    let terms = terms
        .split_whitespace()
        .map(|term| {
            if term.is_empty()
                || !term
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                return Err(AtmError::validation(
                    "NEAR terms must be plain words; quote/proximity nesting is not supported",
                ));
            }
            SearchAtom::term(term)
        })
        .collect::<Result<Vec<_>, _>>()?;
    SearchExpression::Near {
        terms: terms.clone(),
        max_distance: distance,
    }
    .validate()?;
    Ok(Token::Near(terms, distance))
}

fn parse_or(tokens: &[Token]) -> Result<SearchExpression, AtmError> {
    let mut groups = Vec::new();
    let mut start = 0;
    for (index, token) in tokens.iter().enumerate() {
        if *token == Token::Or {
            groups.push(parse_and(&tokens[start..index])?);
            start = index + 1;
        }
    }
    groups.push(parse_and(&tokens[start..])?);
    if groups.len() == 1 {
        Ok(groups.remove(0))
    } else {
        Ok(SearchExpression::Any(groups))
    }
}

fn parse_and(tokens: &[Token]) -> Result<SearchExpression, AtmError> {
    if tokens.is_empty() {
        return Err(AtmError::validation(
            "--raw-match cannot have an empty boolean branch",
        ));
    }
    let mut items = Vec::new();
    let mut negate = false;
    let mut needs_term = true;
    for token in tokens {
        match token {
            Token::And => {
                if needs_term {
                    return Err(AtmError::validation("AND requires a term on both sides"));
                }
                needs_term = true;
            }
            Token::Not => {
                if !needs_term || negate {
                    return Err(AtmError::validation("NOT must precede one term"));
                }
                negate = true;
            }
            Token::Or => {
                return Err(AtmError::validation(
                    "internal advanced-search parser error",
                ));
            }
            Token::Word(value) | Token::Phrase(value) => {
                let atom = match token {
                    Token::Word(_) => SearchAtom::term(value.clone())?,
                    Token::Phrase(_) => SearchAtom::phrase(value.clone())?,
                    _ => unreachable!(),
                };
                let expression = SearchExpression::Atom(atom);
                items.push(if negate {
                    negate = false;
                    SearchExpression::Not(Box::new(expression))
                } else {
                    expression
                });
                needs_term = false;
            }
            Token::Near(terms, max_distance) => {
                let expression = SearchExpression::Near {
                    terms: terms.clone(),
                    max_distance: *max_distance,
                };
                items.push(if negate {
                    negate = false;
                    SearchExpression::Not(Box::new(expression))
                } else {
                    expression
                });
                needs_term = false;
            }
        }
    }
    if needs_term {
        return Err(AtmError::validation(
            "--raw-match cannot end with an operator",
        ));
    }
    if items
        .iter()
        .all(|item| matches!(item, SearchExpression::Not(_)))
    {
        return Err(AtmError::validation("--raw-match requires a positive term"));
    }
    if items.len() == 1 {
        Ok(items.remove(0))
    } else {
        Ok(SearchExpression::All(items))
    }
}

fn parse_group_by(value: &str) -> Result<SearchGroupBy, AtmError> {
    if let Some(key) = value.strip_prefix("var:") {
        return Ok(SearchGroupBy::Var(key.parse()?));
    }
    let field = match value {
        "team" => atm_storage::SearchGroupField::Team,
        "agent" => atm_storage::SearchGroupField::Agent,
        "from" | "from_agent" => atm_storage::SearchGroupField::FromAgent,
        "template_type" => atm_storage::SearchGroupField::TemplateType,
        "category" => atm_storage::SearchGroupField::Category,
        _ => {
            return Err(AtmError::validation(
                "group-by must be var:KEY, team, agent, from_agent, template_type, or category",
            ));
        }
    };
    Ok(SearchGroupBy::Field(field))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{SearchInput, SearchRequest, execute_search, parse_search_expression};
    use atm_storage::{
        InMemoryMessageSearchStore, SearchExpression, SearchGroupBy, SearchMetadataMatch,
        SimpleAggregate,
    };

    #[test]
    fn ordinary_text_is_one_literal_phrase_even_when_it_looks_like_fts() {
        let expression = parse_search_expression("title:foo NEAR bar", false).expect("expression");
        assert_eq!(
            expression,
            SearchExpression::Atom(
                atm_storage::SearchAtom::phrase("title:foo NEAR bar").expect("phrase")
            )
        );
    }

    #[test]
    fn raw_grammar_rejects_arbitrary_fts_syntax() {
        assert!(parse_search_expression("title:foo", true).is_err());
        assert!(parse_search_expression("title:foo", true).is_err());
    }

    #[test]
    fn raw_grammar_composes_near_with_boolean_terms() {
        let expression = parse_search_expression("urgent AND NEAR(release candidate, 4)", true)
            .expect("bounded expression");
        assert!(matches!(expression, SearchExpression::All(items) if items.len() == 2));
    }

    #[test]
    fn raw_grammar_applies_not_to_a_near_expression() {
        let expression = parse_search_expression("urgent AND NOT NEAR(release candidate, 4)", true)
            .expect("bounded expression");
        assert!(
            matches!(expression, SearchExpression::All(items) if matches!(items[1], SearchExpression::Not(_)))
        );
    }

    #[test]
    fn input_compiles_shared_key_grammar_and_aggregate() {
        let request = SearchInput {
            vars: vec!["phase=an".to_owned()],
            aggregate: Some(super::SearchAggregateInput::GroupBy(
                "var:sprint".to_owned(),
            )),
            ..SearchInput::default()
        }
        .compile_query()
        .expect("request");
        assert_eq!(request.filters.vars[0].0.as_str(), "phase");
        assert_eq!(
            request.aggregate,
            Some(SimpleAggregate::GroupBy(SearchGroupBy::Var(
                atm_storage::SearchKey::new("sprint").expect("key")
            )))
        );
    }

    #[test]
    fn raw_grammar_compiles_only_bounded_near_syntax() {
        assert_eq!(
            parse_search_expression("NEAR(urgent release, 4)", true).expect("near"),
            SearchExpression::Near {
                terms: vec![
                    atm_storage::SearchAtom::term("urgent").expect("term"),
                    atm_storage::SearchAtom::term("release").expect("term"),
                ],
                max_distance: 4,
            }
        );
        assert!(parse_search_expression("NEAR(urgent release) OR task", true).is_ok());
    }

    #[test]
    fn metadata_star_is_a_bounded_prefix_match_not_sql_syntax() {
        let request = SearchInput {
            template_meta: vec!["type=qa-*".to_owned()],
            ..SearchInput::default()
        }
        .compile_query()
        .expect("request");
        assert!(matches!(
            request.filters.template_metadata[0].1,
            SearchMetadataMatch::Prefix(_)
        ));
    }

    #[tokio::test]
    async fn peer_search_is_rejected_before_the_storage_capability_is_selected() {
        let store = InMemoryMessageSearchStore::default();
        let request = SearchRequest {
            query: SearchInput::default(),
        };
        let error = execute_search(
            crate::api::AuthenticatedIngress::Peer,
            request,
            &store,
            crate::api::RequestDeadline::after(Duration::from_secs(1)),
        )
        .await
        .expect_err("peer search must reject");
        assert_eq!(error.code(), atm_storage::AtmErrorCode::SearchLocalOnly);
        assert_eq!(store.search_calls_for_test(), 0);
    }

    #[tokio::test]
    async fn local_search_uses_the_async_storage_capability() {
        let store = InMemoryMessageSearchStore::default();
        let response = execute_search(
            crate::api::AuthenticatedIngress::Local,
            SearchRequest {
                query: SearchInput::default(),
            },
            &store,
            crate::api::RequestDeadline::after(Duration::from_secs(1)),
        )
        .await
        .expect("local search");
        assert!(response.hits.is_empty());
        assert_eq!(store.search_calls_for_test(), 1);
    }

    #[test]
    fn serialized_http_input_uses_the_same_bounded_compiler_as_the_cli() {
        let input = SearchInput {
            text: Some("urgent AND NEAR(release candidate, 4)".to_owned()),
            raw_match: true,
            template_meta: vec!["type=qa-*".to_owned()],
            vars: vec!["phase=an".to_owned()],
            aggregate: Some(super::SearchAggregateInput::Count),
            ..SearchInput::default()
        };
        let cli_query = input.compile_query().expect("CLI compiler");
        let http_request: SearchRequest =
            serde_json::from_slice(&serde_json::to_vec(&input.into_request()).expect("JSON"))
                .expect("HTTP request JSON");
        assert_eq!(
            http_request.compile_query().expect("HTTP compiler"),
            cli_query
        );
    }
}
