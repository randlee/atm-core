//! Public typed search CLI surface.

use anyhow::Result;
use atm_core::search::{SearchAggregateInput, SearchInput, SearchResponse};
use atm_core::{WorkflowProjectionRequest, WorkflowSelector};
use atm_storage::{
    WorkflowScopeId, WorkflowScopeKind, WorkflowStage, WorkflowState, WorkflowTransition,
};
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

    /// Filter ATM's immutable effective-tag projection; may be repeated.
    #[arg(long = "effective-tag", value_name = "VALUE")]
    effective_tags: Vec<String>,

    #[arg(long)]
    category: Option<String>,

    #[arg(long)]
    from: Option<String>,

    #[arg(long)]
    team: Option<String>,

    #[arg(long)]
    agent: Option<String>,

    #[arg(long = "workflow-scope-kind", value_name = "VALUE")]
    workflow_scope_kind: Option<String>,

    #[arg(long = "workflow-scope-id", value_name = "VALUE")]
    workflow_scope_id: Option<String>,

    #[arg(long = "workflow-state", value_name = "VALUE")]
    workflow_state: Option<String>,

    #[arg(long = "workflow-stage", value_name = "VALUE")]
    workflow_stage: Option<String>,

    #[arg(long = "workflow-transition", value_name = "VALUE")]
    workflow_transition: Option<String>,

    #[arg(long = "workflow-iteration", value_name = "VALUE")]
    workflow_iteration: Option<String>,

    /// Project generic lifecycle observations over the local search result set.
    #[arg(long = "lifecycle-scope-kind", value_name = "VALUE")]
    lifecycle_scope_kind: Option<String>,

    #[arg(long = "lifecycle-scope-id", value_name = "VALUE")]
    lifecycle_scope_id: Option<String>,

    #[arg(long = "lifecycle-start-state", value_name = "VALUE")]
    lifecycle_start_state: Option<String>,

    #[arg(long = "lifecycle-start-stage", value_name = "VALUE")]
    lifecycle_start_stage: Option<String>,

    #[arg(long = "lifecycle-start-transition", value_name = "VALUE")]
    lifecycle_start_transition: Option<String>,

    #[arg(long = "lifecycle-end-state", value_name = "VALUE")]
    lifecycle_end_state: Option<String>,

    #[arg(long = "lifecycle-end-stage", value_name = "VALUE")]
    lifecycle_end_stage: Option<String>,

    #[arg(long = "lifecycle-end-transition", value_name = "VALUE")]
    lifecycle_end_transition: Option<String>,

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
        let lifecycle = self.lifecycle_request()?;
        let request = SearchInput {
            text: self.text.clone(),
            raw_match: self.raw_match,
            template_meta,
            template_sha: self.template_sha.clone(),
            vars: self.vars.clone(),
            tags: self.tags.clone(),
            effective_tags: self.effective_tags.clone(),
            category: self.category.clone(),
            from: self.from.clone(),
            team: self.team.clone(),
            agent: self.agent.clone(),
            workflow_scope_kind: self.workflow_scope_kind.clone(),
            workflow_scope_id: self.workflow_scope_id.clone(),
            workflow_state: self.workflow_state.clone(),
            workflow_stage: self.workflow_stage.clone(),
            workflow_transition: self.workflow_transition.clone(),
            workflow_iteration: self.workflow_iteration.clone(),
            since: self.since.clone(),
            until: self.until.clone(),
            limit: self.limit,
            cursor: self.cursor.clone(),
            per_mailbox: self.per_mailbox,
            aggregate: self.aggregate(),
        }
        .into_request();
        let mut request = atm_core::search::SearchRequest {
            lifecycle,
            ..request
        };
        // Fail before opening a daemon connection while keeping the exact
        // same core compiler authoritative for HTTP ingress.
        let query = request.compile_query()?;
        // A lifecycle projection is a view over this same bounded local
        // search result set, so its time window must not silently differ from
        // the ordinary search filters supplied on this command.
        if let Some(lifecycle) = &mut request.lifecycle {
            lifecycle.time_range = query.filters.time_range;
            lifecycle.validate()?;
        }
        Ok(request)
    }

    fn lifecycle_request(&self) -> Result<Option<WorkflowProjectionRequest>> {
        let supplied = [
            self.lifecycle_scope_kind.as_ref(),
            self.lifecycle_scope_id.as_ref(),
            self.lifecycle_start_state.as_ref(),
            self.lifecycle_start_stage.as_ref(),
            self.lifecycle_start_transition.as_ref(),
            self.lifecycle_end_state.as_ref(),
            self.lifecycle_end_stage.as_ref(),
            self.lifecycle_end_transition.as_ref(),
        ]
        .iter()
        .any(Option::is_some);
        if !supplied {
            return Ok(None);
        }
        let scope_kind = self.lifecycle_scope_kind.as_deref().ok_or_else(|| {
            anyhow::anyhow!("--lifecycle-scope-kind is required with lifecycle selectors")
        })?;
        let request = WorkflowProjectionRequest {
            scope_kind: WorkflowScopeKind::new(scope_kind)?,
            scope_id: self
                .lifecycle_scope_id
                .as_deref()
                .map(WorkflowScopeId::new)
                .transpose()?,
            start: WorkflowSelector {
                state: self
                    .lifecycle_start_state
                    .as_deref()
                    .map(WorkflowState::new)
                    .transpose()?,
                stage: self
                    .lifecycle_start_stage
                    .as_deref()
                    .map(WorkflowStage::new)
                    .transpose()?,
                transition: self
                    .lifecycle_start_transition
                    .as_deref()
                    .map(WorkflowTransition::new)
                    .transpose()?,
            },
            end: WorkflowSelector {
                state: self
                    .lifecycle_end_state
                    .as_deref()
                    .map(WorkflowState::new)
                    .transpose()?,
                stage: self
                    .lifecycle_end_stage
                    .as_deref()
                    .map(WorkflowStage::new)
                    .transpose()?,
                transition: self
                    .lifecycle_end_transition
                    .as_deref()
                    .map(WorkflowTransition::new)
                    .transpose()?,
            },
            time_range: None,
        };
        request.validate()?;
        Ok(Some(request))
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
    if let Some(lifecycle) = &response.lifecycle {
        println!("lifecycle: {}", serde_json::to_string(lifecycle)?);
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
            "--effective-tag",
            "workflow-state=dev-start",
            "--category",
            "work",
            "--from",
            "worker",
            "--team",
            TEST_TEAM,
            "--agent",
            TEST_ARCH_CTM,
            "--workflow-scope-kind",
            "sprint",
            "--workflow-scope-id",
            "an-11",
            "--workflow-state",
            "dev-start",
            "--workflow-stage",
            "implementation",
            "--workflow-transition",
            "opened",
            "--workflow-iteration",
            "1",
            "--lifecycle-scope-kind",
            "sprint",
            "--lifecycle-scope-id",
            "an-12",
            "--lifecycle-start-state",
            "dev-start",
            "--lifecycle-end-state",
            "dev-complete",
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
            effective_tags: vec![],
            category: None,
            from: None,
            team: None,
            agent: None,
            workflow_scope_kind: None,
            workflow_scope_id: None,
            workflow_state: None,
            workflow_stage: None,
            workflow_transition: None,
            workflow_iteration: None,
            lifecycle_scope_kind: None,
            lifecycle_scope_id: None,
            lifecycle_start_state: None,
            lifecycle_start_stage: None,
            lifecycle_start_transition: None,
            lifecycle_end_state: None,
            lifecycle_end_stage: None,
            lifecycle_end_transition: None,
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
    fn lifecycle_cli_surface_compiles_a_generic_projection() {
        let crate::commands::Cli { command, .. } = crate::commands::Cli::try_parse_from([
            "atm",
            "search",
            "--lifecycle-scope-kind",
            "campaign",
            "--lifecycle-start-stage",
            "prepare",
            "--lifecycle-end-stage",
            "release",
        ])
        .expect("CLI grammar");
        let super::super::Command::Search(command) = command else {
            unreachable!("search command")
        };
        let lifecycle = command
            .build_request()
            .expect("generic lifecycle request")
            .lifecycle
            .expect("lifecycle projection");
        assert_eq!(lifecycle.scope_kind.as_str(), "campaign");
        assert_eq!(
            lifecycle.start.stage.expect("start stage").as_str(),
            "prepare"
        );
        assert!(lifecycle.time_range.is_none());
    }

    #[test]
    fn lifecycle_projection_inherits_the_search_time_window() {
        let crate::commands::Cli { command, .. } = crate::commands::Cli::try_parse_from([
            "atm",
            "search",
            "--lifecycle-scope-kind",
            "campaign",
            "--lifecycle-start-state",
            "queued",
            "--lifecycle-end-state",
            "released",
            "--since",
            "2026-08-01T00:00:00Z",
            "--until",
            "2026-08-02T00:00:00Z",
        ])
        .expect("CLI grammar");
        let super::super::Command::Search(command) = command else {
            unreachable!("search command")
        };
        let request = command.build_request().expect("valid request");
        let range = request
            .lifecycle
            .expect("lifecycle projection")
            .time_range
            .expect("shared range");
        assert_eq!(
            range.since.expect("since"),
            "2026-08-01T00:00:00Z".parse().expect("timestamp")
        );
        assert_eq!(
            range.until.expect("until"),
            "2026-08-02T00:00:00Z".parse().expect("timestamp")
        );
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
