//! Public typed search CLI surface.

use anyhow::Result;
use atm_core::search::{SearchAggregateInput, SearchInput, SearchResponse};
use clap::Args;

use crate::composition::{
    AtmHomePath, CliComposition, InvocationDir, resolve_command_runtime_context,
};
use crate::observability::CliObservability;

/// Search locally indexed ATM messages through the daemon's typed query API.
#[derive(Debug, Args)]
#[command(
    after_help = "Plain positional text is always a literal phrase. --raw-match enables ATM's bounded advanced grammar (words, quoted phrases, NEAR(term term[, distance]), AND, OR, NOT); it never passes raw SQLite FTS syntax."
)]
pub struct SearchCommand {
    /// Literal phrase by default, or an ATM advanced expression with --raw-match.
    text: Option<String>,

    /// Parse the positional text with ATM's documented bounded advanced grammar.
    #[arg(long, requires = "text")]
    raw_match: bool,

    /// Filter stored template frontmatter metadata; a trailing * is a prefix match.
    #[arg(long = "template-meta", value_name = "KEY=VALUE")]
    template_meta: Vec<String>,

    /// Shorthand for --template-meta type=VALUE.
    #[arg(long = "type", value_name = "VALUE")]
    template_type: Option<String>,

    /// Filter the exact immutable template revision.
    #[arg(long, value_name = "SHA")]
    template_sha: Option<String>,

    /// Filter one stored template variable; may be repeated.
    #[arg(long = "var", value_name = "KEY=VALUE")]
    vars: Vec<String>,

    #[arg(long = "tag", value_name = "VALUE")]
    tags: Vec<String>,

    #[arg(long)]
    category: Option<String>,

    #[arg(long)]
    from: Option<String>,

    #[arg(long)]
    team: Option<String>,

    #[arg(long)]
    agent: Option<String>,

    #[arg(long)]
    since: Option<String>,

    #[arg(long)]
    until: Option<String>,

    #[arg(long)]
    limit: Option<u32>,

    #[arg(long)]
    cursor: Option<String>,

    /// Preserve per-mailbox compound-key identities rather than default deduplication.
    #[arg(long)]
    per_mailbox: bool,

    #[arg(long, conflicts_with_all = ["group_by", "min", "max"])]
    count: bool,

    #[arg(long, value_name = "FIELD", conflicts_with_all = ["count", "min", "max"])]
    group_by: Option<String>,

    #[arg(long, value_parser = ["message_at"], conflicts_with_all = ["count", "group_by", "max"])]
    min: Option<String>,

    #[arg(long, value_parser = ["message_at"], conflicts_with_all = ["count", "group_by", "min"])]
    max: Option<String>,

    #[arg(long)]
    json: bool,
}

impl SearchCommand {
    pub async fn run(self, observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("search")?;
        let json = self.json;
        let request = self.build_request()?;
        let composition = CliComposition::bootstrap(
            "search",
            observability,
            InvocationDir::new(&current_dir),
            AtmHomePath::new(&home_dir),
        )?;
        let response = composition.search(request).await?;
        print_search_response(&response, json)
    }

    pub(crate) fn build_request(&self) -> Result<atm_core::search::SearchRequest> {
        let mut template_meta = self.template_meta.clone();
        if let Some(template_type) = &self.template_type {
            template_meta.push(format!("type={template_type}"));
        }
        let request = SearchInput {
            text: self.text.clone(),
            raw_match: self.raw_match,
            template_meta,
            template_sha: self.template_sha.clone(),
            vars: self.vars.clone(),
            tags: self.tags.clone(),
            category: self.category.clone(),
            from: self.from.clone(),
            team: self.team.clone(),
            agent: self.agent.clone(),
            since: self.since.clone(),
            until: self.until.clone(),
            limit: self.limit,
            cursor: self.cursor.clone(),
            per_mailbox: self.per_mailbox,
            aggregate: self.aggregate(),
        }
        .into_request();
        // Fail before opening a daemon connection while keeping the exact
        // same core compiler authoritative for HTTP ingress.
        request.compile_query()?;
        Ok(request)
    }

    fn aggregate(&self) -> Option<SearchAggregateInput> {
        if self.count {
            Some(SearchAggregateInput::Count)
        } else if let Some(group_by) = &self.group_by {
            Some(SearchAggregateInput::GroupBy(group_by.clone()))
        } else if self.min.is_some() {
            Some(SearchAggregateInput::MinMessageAt)
        } else if self.max.is_some() {
            Some(SearchAggregateInput::MaxMessageAt)
        } else {
            None
        }
    }
}

fn print_search_response(response: &SearchResponse, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(response)?);
        return Ok(());
    }
    for hit in &response.hits {
        let identity = hit
            .message_id
            .as_deref()
            .unwrap_or(hit.key.message_key.as_str());
        let classification = hit
            .template_type
            .as_deref()
            .or(hit.category.as_deref())
            .unwrap_or("unclassified");
        println!(
            "{identity} {} {} -> {} [{classification}] {}",
            hit.message_at, hit.from_agent.agent, hit.to_agent.agent, hit.snippet
        );
    }
    if let Some(aggregate) = &response.aggregate {
        println!("aggregate: {}", serde_json::to_string(aggregate)?);
    }
    if let Some(cursor) = &response.next_cursor {
        println!("next_cursor: {}", cursor.as_str());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::SearchCommand;
    use atm_core::test_support::{TEST_ARCH_CTM, TEST_TEAM};
    use clap::Parser;

    #[test]
    fn documented_search_surface_parses_all_filters() {
        crate::commands::Cli::try_parse_from([
            "atm",
            "search",
            "assignment",
            "--template-meta",
            "document_type=task",
            "--type",
            "dev",
            "--template-sha",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--var",
            "phase=an",
            "--tag",
            "priority",
            "--category",
            "work",
            "--from",
            "worker",
            "--team",
            TEST_TEAM,
            "--agent",
            TEST_ARCH_CTM,
            "--since",
            "2026-01-01T00:00:00Z",
            "--until",
            "2026-01-02T00:00:00Z",
            "--limit",
            "25",
            "--per-mailbox",
            "--count",
            "--json",
        ])
        .expect("search surface must parse");
    }

    #[test]
    fn type_is_compiled_as_metadata_sugar() {
        let command = SearchCommand {
            text: None,
            raw_match: false,
            template_meta: vec![],
            template_type: Some("dev".to_owned()),
            template_sha: None,
            vars: vec![],
            tags: vec![],
            category: None,
            from: None,
            team: None,
            agent: None,
            since: None,
            until: None,
            limit: None,
            cursor: None,
            per_mailbox: false,
            count: false,
            group_by: None,
            min: None,
            max: None,
            json: false,
        };
        let request = command.build_request().expect("request");
        assert_eq!(request.query.template_meta[0], "type=dev");
    }

    #[test]
    fn malformed_query_keys_fail_at_the_real_cli_command_boundary() {
        for flag_and_value in [
            ("--template-meta", "../phase=an"),
            ("--var", "phase/path=an"),
            ("--group-by", "var:../../phase"),
        ] {
            let mut arguments = vec!["atm", "search", flag_and_value.0, flag_and_value.1];
            if flag_and_value.0 == "--group-by" {
                arguments = vec!["atm", "search", "--group-by", flag_and_value.1];
            }
            let crate::commands::Cli { command, .. } =
                crate::commands::Cli::try_parse_from(arguments).expect("CLI grammar");
            let super::super::Command::Search(command) = command else {
                unreachable!("search command")
            };
            let error = command
                .build_request()
                .expect_err("invalid public key must reject at the core boundary");
            let typed = error
                .downcast_ref::<atm_storage::AtmError>()
                .expect("CLI must preserve the typed core validation error");
            assert_eq!(
                typed.code(),
                atm_storage::AtmErrorCode::MessageValidationFailed,
                "CLI query-key rejection must use the shared validation code"
            );
        }
    }
}
