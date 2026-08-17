use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Result;
use atm_core::address::AgentAddress;
use atm_core::load_atm_config;
use atm_core::send::{
    MessageClassification, SendMessageSource, SendRequest, TemplateSendSource, input,
};
use atm_core::types::{AgentIdentity, HostName, TaskId, TeamName};
use atm_daemon_bootstrap::with_default_peer_address_stores;
use atm_storage::{AtmError, PeerConfigStore, RosterStore, TrustedPeer};
use clap::Args;

use crate::commands::caller_context::{
    CallerChatIdOverride, CallerContextOverrides, CallerIdentityOverride, CallerTeamOverride,
    resolve_cli_mutation_caller_context, resolve_cli_mutation_caller_context_with_overrides,
};
use crate::composition::{
    AtmHomePath, CliComposition, InvocationDir, resolve_command_runtime_context,
};
use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
#[command(
    after_help = "Path-only bodies are admitted for compatibility but recorded as content_format=path-ref and warned on stderr; use `atm send --template <path> --vars <file>` to send rendered content, and `atm compose --template <path>` to preview it. Post-send hooks can be configured in .atm.toml via one or more [[atm.post_send_hooks]] rules with recipient = \"name-or-*\" and command = [\"argv\", ...]. Matching rules run after a successful non-dry-run send, in config order. Path-like command[0] values resolve relative to the declaring .atm.toml; bare executables like bash or python3 use normal PATH resolution. Recipient non-match is silent. For hook troubleshooting, combine --stderr-logs with ATM_LOG=debug to surface debug-level hook diagnostics on stderr."
)]
/// Send one ATM mailbox message.
pub struct SendCommand {
    #[arg()]
    to: String,

    #[arg(index = 2)]
    message: Option<String>,

    #[arg(long)]
    team: Option<String>,

    /// Route this send through the explicitly named host.
    ///
    /// Supplying this flag allows a same-identity send to be an intentional
    /// physical delivery test (for example, `--host localhost` or a same-host
    /// IP).
    /// This is wire-equivalent to a host-qualified recipient address. When
    /// both forms are supplied, they must name the same host.
    #[arg(long, value_name = "HOST")]
    host: Option<String>,

    #[arg(long = "chat-id", conflicts_with = "actor")]
    chat_id: Option<String>,

    #[arg(long = "as")]
    actor: Option<String>,

    #[arg(long)]
    file: Option<PathBuf>,

    #[arg(long)]
    stdin: bool,

    /// Render and send a locally loaded template through the daemon-owned
    /// template admission path.
    #[arg(long, value_name = "PATH")]
    template: Option<PathBuf>,

    /// JSON object providing template variables. `-` reads this object from
    /// stdin; it is distinct from `--stdin`, which is a plain message source.
    #[arg(long, value_name = "FILE|-")]
    vars: Option<String>,

    /// One template variable. May be repeated; values parse as JSON when
    /// possible and otherwise remain strings.
    #[arg(long = "var", value_name = "KEY=VALUE")]
    var: Vec<String>,

    /// Capture current environment variables with this prefix at CLI
    /// composition time.
    #[arg(long = "env-prefix", value_name = "PREFIX")]
    env_prefix: Option<String>,

    #[arg(long, value_name = "CATEGORY")]
    category: Option<String>,

    #[arg(long = "tag", value_name = "TAG")]
    tag: Vec<String>,

    #[arg(long = "content-format", value_name = "FORMAT")]
    content_format: Option<String>,

    #[arg(long)]
    summary: Option<String>,

    #[arg(long = "requires-ack")]
    requires_ack: bool,

    #[arg(long = "task-id")]
    task_id: Option<TaskId>,

    #[arg(long)]
    dry_run: bool,

    #[arg(long)]
    json: bool,
}

impl SendCommand {
    fn message_validation_error(
        message: impl Into<String>,
        recovery: impl Into<String>,
    ) -> anyhow::Error {
        atm_core::error::AtmError::validation_with_recovery(message, recovery).into()
    }

    fn template_load_error(message: impl Into<String>) -> anyhow::Error {
        atm_core::error::AtmError::new(atm_core::error::AtmErrorCode::TemplateLoadFailed, message)
            .into()
    }

    fn template_classification_error(message: impl Into<String>) -> anyhow::Error {
        atm_core::error::AtmError::new(
            atm_core::error::AtmErrorCode::TemplateClassificationInvalid,
            message,
        )
        .into()
    }

    /// Execute the `atm send` command.
    pub async fn run(self, observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("send")?;
        let json = self.json;
        let request = self.build_request(home_dir.clone(), current_dir.clone())?;
        let composition = CliComposition::bootstrap(
            "send",
            observability,
            InvocationDir::new(&current_dir),
            AtmHomePath::new(&home_dir),
        )?;
        let outcome = composition.send(request).await?;

        output::print_send_result(&outcome, json)
    }

    fn build_request(self, home_dir: PathBuf, current_dir: PathBuf) -> Result<SendRequest> {
        let max_message_bytes = load_atm_config(&current_dir)?
            .map(|config| {
                config.max_message_bytes.as_usize().ok_or_else(|| {
                    anyhow::anyhow!("configured max_message_bytes does not fit this platform")
                })
            })
            .transpose()?
            .unwrap_or(input::default_message_max_bytes());
        let caller_context = if self.actor.is_some() || self.chat_id.is_some() {
            resolve_cli_mutation_caller_context_with_overrides(CallerContextOverrides {
                identity_override: self.actor.as_deref().map(CallerIdentityOverride),
                chat_id_override: self.chat_id.as_deref().map(CallerChatIdOverride),
                team_override: self.team.as_deref().map(CallerTeamOverride),
            })?
        } else {
            resolve_cli_mutation_caller_context(self.team.as_deref().map(CallerTeamOverride))?
        };
        let target = self.target_with_explicit_host(&caller_context.caller_team)?;
        let classification = self.build_classification()?;
        let message_source = self.build_message_source(max_message_bytes, &current_dir)?;
        SendRequest::new(
            home_dir,
            current_dir,
            caller_context.caller_identity,
            &target,
            caller_context.caller_team,
            message_source,
            self.summary,
            self.requires_ack,
            self.task_id,
            self.dry_run,
        )
        .map(|request| {
            request
                .with_caller_chat_id(caller_context.caller_chat_id)
                .with_activity_observation(caller_context.activity_observation)
                .with_max_message_bytes(max_message_bytes)
                .with_classification(classification)
        })
        .map_err(Into::into)
    }

    fn target_with_explicit_host(&self, caller_team: &TeamName) -> Result<String> {
        if !self.requires_peer_authority_lookup(caller_team) {
            return self.legacy_target_with_explicit_host(caller_team);
        }

        // The shorthand is resolved at CLI composition time. The daemon and
        // HTTP request receive the same canonical AgentAddress as if the user
        // had typed the full `agent@team.host` form themselves.
        with_default_peer_address_stores(|roster_store, peer_store| {
            self.target_with_authorities(caller_team, roster_store, peer_store)
        })
        .map_err(Into::into)
    }

    fn requires_peer_authority_lookup(&self, caller_team: &TeamName) -> bool {
        if self
            .host
            .as_deref()
            .is_some_and(|host| !is_legacy_direct_host(host))
        {
            return true;
        }

        let Some((_, destination)) = self.to.trim().split_once('@') else {
            return false;
        };
        if destination.eq(caller_team.as_str()) {
            return false;
        }
        destination
            .split_once('.')
            .is_none_or(|(_, host)| !host.eq_ignore_ascii_case("localhost"))
    }

    fn target_with_authorities(
        &self,
        caller_team: &TeamName,
        roster_store: &(dyn RosterStore + Send + Sync),
        peer_store: &(dyn PeerConfigStore + Send + Sync),
    ) -> std::result::Result<String, AtmError> {
        let known_teams = roster_store.list_teams()?;
        let peers = peer_store.list_trusted_peers()?;
        self.target_with_peer_records(caller_team, &known_teams, &peers)
    }

    fn target_with_peer_records(
        &self,
        caller_team: &TeamName,
        known_teams: &[TeamName],
        peers: &[TrustedPeer],
    ) -> std::result::Result<String, AtmError> {
        let parsed = CliRecipientInput::parse(&self.to)?;
        let recipient = match parsed.destination {
            None => AgentAddress::new(parsed.identity.agent, parsed.identity.chat_id, None, None)?,
            Some(destination) => {
                if destination == caller_team.as_str() {
                    AgentAddress::new(
                        parsed.identity.agent,
                        parsed.identity.chat_id,
                        Some(caller_team.clone()),
                        None,
                    )?
                } else if let Some(team) =
                    known_teams.iter().find(|team| team.as_str() == destination)
                {
                    AgentAddress::new(
                        parsed.identity.agent,
                        parsed.identity.chat_id,
                        Some(team.clone()),
                        None,
                    )?
                } else if Ipv4Addr::from_str(&destination).is_ok() {
                    let host = resolve_host_input(&destination, peers)?;
                    AgentAddress::new(
                        parsed.identity.agent,
                        parsed.identity.chat_id,
                        Some(caller_team.clone()),
                        Some(host),
                    )?
                } else if let Some((team, host_input)) = destination.split_once('.') {
                    let team = team.parse::<TeamName>()?;
                    if let Some(host) = try_resolve_host_input(host_input, peers)? {
                        AgentAddress::new(
                            parsed.identity.agent,
                            parsed.identity.chat_id,
                            Some(team),
                            Some(host),
                        )?
                    } else if let Some(host) = resolve_trusted_host(&destination, peers)? {
                        AgentAddress::new(
                            parsed.identity.agent,
                            parsed.identity.chat_id,
                            Some(caller_team.clone()),
                            Some(host),
                        )?
                    } else {
                        return Err(peer_resolution_error(
                            format!("no enabled trusted peer matches '{destination}'"),
                            "Run `atm peer trust list`; add or enable the intended peer, then retry using `agent@team.host`.",
                        ));
                    }
                } else if let Some(host) = resolve_trusted_host(&destination, peers)? {
                    AgentAddress::new(
                        parsed.identity.agent,
                        parsed.identity.chat_id,
                        Some(caller_team.clone()),
                        Some(host),
                    )?
                } else {
                    return Err(peer_resolution_error(
                        format!(
                            "'{destination}' is neither a known local team nor an enabled trusted peer"
                        ),
                        "Use `atm team list` to select a local team, or add/list the peer with `atm peer trust list` and retry with `agent@team.host`.",
                    ));
                }
            }
        };

        let explicit_host = self
            .host
            .as_deref()
            .map(|host| resolve_host_input(host, peers))
            .transpose()?;

        match (recipient.host(), explicit_host) {
            (Some(address_host), Some(flag_host))
                if !address_host
                    .as_str()
                    .eq_ignore_ascii_case(flag_host.as_str()) =>
            {
                Err(peer_resolution_error(
                    "recipient host and --host disagree",
                    "Use the same host in both places, or specify it once with either `recipient@team.host` or `--host <host>`.",
                ))
            }
            (Some(_), _) => Ok(recipient.to_string()),
            (None, None) => Ok(recipient.to_string()),
            (None, Some(host)) => AgentAddress::new(
                recipient.agent().clone(),
                recipient.chat_id().cloned(),
                Some(
                    recipient
                        .team()
                        .cloned()
                        .unwrap_or_else(|| caller_team.clone()),
                ),
                Some(host),
            )
            .map(|target| target.to_string()),
        }
    }

    fn legacy_target_with_explicit_host(&self, caller_team: &TeamName) -> Result<String> {
        let explicit_host = self.host.as_deref().map(parse_host_input).transpose()?;
        let recipient: AgentAddress = self.to.parse().map_err(anyhow::Error::from)?;

        match (recipient.host(), explicit_host) {
            (Some(address_host), Some(flag_host)) if address_host != &flag_host => {
                Err(Self::message_validation_error(
                    "recipient host and --host disagree",
                    "Use the same host in both places, or specify it once with either `recipient@team.host` or `--host <host>`.",
                ))
            }
            (Some(_), _) => Ok(recipient.to_string()),
            (None, None) => Ok(recipient.to_string()),
            (None, Some(host)) => AgentAddress::new(
                recipient.agent().clone(),
                recipient.chat_id().cloned(),
                Some(
                    recipient
                        .team()
                        .cloned()
                        .unwrap_or_else(|| caller_team.clone()),
                ),
                Some(host),
            )
            .map(|target| target.to_string())
            .map_err(Into::into),
        }
    }

    fn build_message_source(
        &self,
        max_message_bytes: usize,
        current_dir: &std::path::Path,
    ) -> Result<SendMessageSource> {
        if self.template.is_some() && (self.stdin || self.file.is_some() || self.message.is_some())
        {
            return Err(Self::message_validation_error(
                "--template is mutually exclusive with message text, --file, and --stdin",
                "Use `--template <path>` by itself, then pass template data through --vars, --var, or --env-prefix.",
            ));
        }
        if self.stdin && self.file.is_some() {
            return Err(Self::message_validation_error(
                "--stdin and --file are mutually exclusive",
                "Choose exactly one message source: either pass `--stdin` or `--file <path>` before retrying `atm send`.",
            ));
        }

        if self.stdin && self.message.is_some() {
            return Err(Self::message_validation_error(
                "--stdin and positional message text are mutually exclusive",
                "Choose exactly one message source: either pass `--stdin` or provide positional message text before retrying `atm send`.",
            ));
        }

        if let Some(template) = &self.template {
            return self
                .build_template_source(template, current_dir)
                .map(SendMessageSource::Template);
        }

        if self.vars.is_some() || !self.var.is_empty() || self.env_prefix.is_some() {
            return Err(Self::message_validation_error(
                "template-only option supplied without --template",
                "Pass `--template <path>` before using --vars, --var, or --env-prefix.",
            ));
        }

        match (&self.file, self.stdin, &self.message) {
            (Some(path), false, message) => Ok(SendMessageSource::File {
                path: path.clone(),
                message: message.clone(),
            }),
            // stdin is a CLI-owned input source. Materialize it before
            // bootstrapping the daemon so a wire request can never ask the
            // daemon (whose stdin is intentionally null) to read it.
            (None, true, None) => input::read_message_from_stdin_with_limit(max_message_bytes)
                .map(SendMessageSource::Inline)
                .map_err(Into::into),
            (None, false, Some(message)) => Ok(SendMessageSource::Inline(message.clone())),
            (None, false, None) => Err(Self::message_validation_error(
                "provide message text, --file, or --stdin",
                "Pass positional message text, `--file <path>`, or `--stdin` before retrying `atm send`.",
            )),
            (Some(_), true, _) => unreachable!("validated above"),
            (None, true, Some(_)) => unreachable!("validated above"),
        }
    }

    fn build_template_source(
        &self,
        template: &std::path::Path,
        current_dir: &std::path::Path,
    ) -> Result<TemplateSendSource> {
        let template_path = if template.is_absolute() {
            template.to_path_buf()
        } else {
            current_dir.join(template)
        };
        let canonical_template_path = std::fs::canonicalize(&template_path).map_err(|error| {
            Self::template_load_error(format!("template could not be resolved: {error}"))
        })?;
        let canonical_template_root = canonical_template_path
            .parent()
            .ok_or_else(|| Self::template_load_error("template path has no parent directory"))?
            .to_path_buf();
        let raw_file_bytes = std::fs::read(&canonical_template_path).map_err(|error| {
            Self::template_load_error(format!("template could not be read: {error}"))
        })?;
        let var_file_values = self.read_var_file(current_dir)?;
        let explicit_values = parse_assignment_values(&self.var)?;
        let environment_values = capture_environment_values(self.env_prefix.as_deref())?;
        Ok(TemplateSendSource {
            canonical_template_path,
            canonical_template_root,
            raw_file_bytes,
            input_defaults: serde_json::Map::new(),
            var_file_values,
            explicit_values,
            environment_values,
        })
    }

    fn build_classification(&self) -> Result<MessageClassification> {
        let category = normalize_optional_label(self.category.as_deref(), "category")?;
        let content_format =
            normalize_optional_label(self.content_format.as_deref(), "content format")?;
        let tags = parse_tags(&self.tag)?;
        validate_classification(&category, &tags, &content_format)?;
        Ok(MessageClassification {
            category,
            tags,
            content_format,
        })
    }

    fn read_var_file(
        &self,
        current_dir: &std::path::Path,
    ) -> Result<serde_json::Map<String, serde_json::Value>> {
        let Some(source) = self.vars.as_deref() else {
            return Ok(serde_json::Map::new());
        };
        let contents = if source == "-" {
            use std::io::Read as _;
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .map_err(|error| {
                    Self::template_load_error(format!("--vars stdin could not be read: {error}"))
                })?;
            input
        } else {
            let path = std::path::Path::new(source);
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                current_dir.join(path)
            };
            std::fs::read_to_string(path).map_err(|error| {
                Self::template_load_error(format!("--vars file could not be read: {error}"))
            })?
        };
        let value: serde_json::Value = serde_json::from_str(&contents).map_err(|error| {
            Self::template_load_error(format!("--vars must contain a JSON object: {error}"))
        })?;
        value
            .as_object()
            .cloned()
            .ok_or_else(|| Self::template_load_error("--vars must contain a JSON object"))
    }
}

/// CLI-only recipient syntax before a destination is normalized against the
/// local roster and durable peer authorities. It intentionally never crosses
/// into `atm-core`: every downstream boundary receives `AgentAddress`.
#[derive(Debug)]
struct CliRecipientInput {
    identity: AgentIdentity,
    destination: Option<String>,
}

impl CliRecipientInput {
    fn parse(raw: &str) -> std::result::Result<Self, AtmError> {
        let raw = raw.trim();
        let Some((identity, destination)) = raw.split_once('@') else {
            return Ok(Self {
                identity: raw.parse()?,
                destination: None,
            });
        };
        if destination.contains('@') {
            return Err(AtmError::address_parse("address may contain only one '@'"));
        }
        if destination.is_empty() {
            return Err(AtmError::address_parse("destination must not be empty"));
        }
        Ok(Self {
            identity: identity.parse()?,
            destination: Some(destination.to_string()),
        })
    }
}

fn parse_host_input(raw_host: &str) -> Result<HostName> {
    raw_host.parse::<HostName>().map_err(|_source| {
        SendCommand::message_validation_error(
            "invalid --host",
            "Pass a valid hostname or IP address to `--host` before retrying `atm send`.",
        )
    })
}

fn is_legacy_direct_host(raw_host: &str) -> bool {
    raw_host.eq_ignore_ascii_case("localhost")
}

fn resolve_host_input(
    raw_host: &str,
    peers: &[TrustedPeer],
) -> std::result::Result<HostName, AtmError> {
    try_resolve_host_input(raw_host, peers)?.ok_or_else(|| {
        peer_resolution_error(
            format!("no enabled trusted peer matches '{raw_host}'"),
            "Run `atm peer trust list`; add or enable the intended peer, then retry using its registered hostname.",
        )
    })
}

fn try_resolve_host_input(
    raw_host: &str,
    peers: &[TrustedPeer],
) -> std::result::Result<Option<HostName>, AtmError> {
    let host = raw_host.parse::<HostName>().map_err(|_source| {
        peer_resolution_error(
            "invalid host",
            "Pass a valid trusted hostname (optionally omitting terminal `.local`) or a diagnostic localhost/IP value.",
        )
    })?;
    if is_legacy_direct_host(host.as_str()) {
        return Ok(Some(host));
    }
    if let Ok(ip) = Ipv4Addr::from_str(host.as_str()) {
        return resolve_trusted_ipv4(ip, peers).map(Some);
    }
    resolve_trusted_host(host.as_str(), peers)
}

fn resolve_trusted_ipv4(
    target_ip: Ipv4Addr,
    peers: &[TrustedPeer],
) -> std::result::Result<HostName, AtmError> {
    resolve_trusted_ipv4_with_lookup(target_ip, peers, |peer| {
        let authority = (peer.host.as_str(), peer.https_port.get());
        authority
            .to_socket_addrs()
            .map(|addresses| addresses.map(|address| address.ip()).collect())
            .map_err(|error| {
                peer_resolution_error(
                    format!("trusted peer '{}' could not be resolved: {error}", peer.host),
                    "Verify the registered DNS/mDNS authority is resolvable, then retry the send with its hostname.",
                )
            })
    })
}

fn resolve_trusted_ipv4_with_lookup(
    target_ip: Ipv4Addr,
    peers: &[TrustedPeer],
    lookup: impl Fn(&TrustedPeer) -> std::result::Result<Vec<IpAddr>, AtmError>,
) -> std::result::Result<HostName, AtmError> {
    const MAX_ENABLED_PEER_LOOKUPS: usize = 32;

    let enabled_peers: Vec<&TrustedPeer> = peers.iter().filter(|peer| peer.enabled).collect();
    if enabled_peers.len() > MAX_ENABLED_PEER_LOOKUPS {
        return Err(peer_resolution_error(
            format!(
                "literal IP authority resolution exceeds the {MAX_ENABLED_PEER_LOOKUPS}-peer lookup bound"
            ),
            "Use the registered hostname directly, or reduce the enabled peer set before retrying the literal IP diagnostic.",
        ));
    }

    let mut matches = Vec::new();
    for peer in enabled_peers {
        if lookup(peer)?.contains(&IpAddr::V4(target_ip)) {
            matches.push(peer);
        }
    }
    match matches.as_slice() {
        [peer] => Ok(peer.host.clone()),
        [] => Err(peer_resolution_error(
            format!("no enabled trusted peer resolves to literal IP '{target_ip}'"),
            "Use a registered hostname, or add/enable the exact trusted peer before retrying.",
        )),
        _ => Err(peer_resolution_error(
            format!("literal IP '{target_ip}' resolves to multiple enabled trusted peers"),
            "Use the exact registered hostname instead of the shared IP address.",
        )),
    }
}

fn resolve_trusted_host(
    raw_host: &str,
    peers: &[TrustedPeer],
) -> std::result::Result<Option<HostName>, AtmError> {
    let exact_or_local = |host: &HostName| {
        let canonical_lower = host.as_str().to_ascii_lowercase();
        host.as_str().eq_ignore_ascii_case(raw_host)
            || (!raw_host.to_ascii_lowercase().ends_with(".local")
                && canonical_lower
                    .strip_suffix(".local")
                    .is_some_and(|short| short.eq_ignore_ascii_case(raw_host)))
    };
    let matches: Vec<&TrustedPeer> = peers
        .iter()
        .filter(|peer| peer.enabled && exact_or_local(&peer.host))
        .collect();
    match matches.as_slice() {
        [] => Ok(None),
        [peer] => Ok(Some(peer.host.clone())),
        _ => Err(peer_resolution_error(
            format!("trusted peer shorthand '{raw_host}' is ambiguous"),
            "Use the exact registered hostname from `atm peer trust list`, or remove the duplicate enabled peer record before retrying.",
        )),
    }
}

fn peer_resolution_error(message: impl Into<String>, recovery: impl Into<String>) -> AtmError {
    AtmError::validation_with_recovery(message, recovery)
}

pub(crate) fn parse_assignment_values(
    values: &[String],
) -> Result<serde_json::Map<String, serde_json::Value>> {
    let mut parsed = serde_json::Map::new();
    for raw in values {
        let (key, value) = raw.split_once('=').ok_or_else(|| {
            SendCommand::message_validation_error(
                format!("invalid --var '{raw}'"),
                "Use `--var key=value`; the key must not be blank.",
            )
        })?;
        if key.trim().is_empty() {
            return Err(SendCommand::message_validation_error(
                "template variable key must not be blank",
                "Use `--var key=value` with a non-blank key.",
            ));
        }
        let json_value = serde_json::from_str(value)
            .unwrap_or_else(|_| serde_json::Value::String(value.to_string()));
        parsed.insert(key.to_string(), json_value);
    }
    Ok(parsed)
}

pub(crate) fn capture_environment_values(
    prefix: Option<&str>,
) -> Result<serde_json::Map<String, serde_json::Value>> {
    let Some(prefix) = prefix else {
        return Ok(serde_json::Map::new());
    };
    if prefix.is_empty() {
        return Err(SendCommand::message_validation_error(
            "--env-prefix must not be empty",
            "Pass a non-empty prefix such as `ATM_TEMPLATE_`.",
        ));
    }
    Ok(std::env::vars()
        .filter_map(|(key, value)| {
            key.strip_prefix(prefix)
                .map(|name| (name.to_string(), serde_json::Value::String(value)))
        })
        .collect())
}

fn normalize_optional_label(value: Option<&str>, kind: &str) -> Result<Option<String>> {
    value
        .map(|value| {
            let value = value.trim();
            if value.is_empty() {
                Err(SendCommand::message_validation_error(
                    format!("template {kind} must not be blank"),
                    format!("Pass a non-blank --{} value.", kind.replace(' ', "-")),
                ))
            } else {
                Ok(value.to_string())
            }
        })
        .transpose()
}

fn parse_tags(raw_tags: &[String]) -> Result<Vec<String>> {
    let tags: Vec<String> = raw_tags
        .iter()
        .flat_map(|raw| raw.split(','))
        .map(str::trim)
        .map(str::to_string)
        .collect();
    if tags.iter().any(|tag| tag.is_empty()) {
        return Err(SendCommand::message_validation_error(
            "template tag must not be blank",
            "Use comma-separated non-blank tags, or repeat --tag.",
        ));
    }
    Ok(tags)
}

fn validate_classification(
    category: &Option<String>,
    tags: &[String],
    content_format: &Option<String>,
) -> Result<()> {
    const MAX_TAGS: usize = 16;
    if tags.len() > MAX_TAGS {
        return Err(SendCommand::template_classification_error(format!(
            "template tag count exceeds {MAX_TAGS}"
        )));
    }
    let valid_label = |label: &str| {
        !label.is_empty()
            && label.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'-' | b'_' | b'.')
            })
    };
    if category.as_deref().is_some_and(|value| !valid_label(value))
        || content_format
            .as_deref()
            .is_some_and(|value| !valid_label(value))
        || tags.iter().any(|tag| !valid_label(tag))
    {
        return Err(SendCommand::template_classification_error(
            "template category, tag, and content format must use lowercase letters, digits, '-', '_', or '.'",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::num::NonZeroU16;
    use std::path::{Path, PathBuf};

    use super::{SendCommand, resolve_trusted_ipv4_with_lookup};
    use atm_core::roles::ROLE_TEAM_LEAD;
    use atm_core::send::{SendMessageSource, input};
    use atm_core::test_support::{EnvGuard, TEST_SENDER};
    use atm_core::types::TeamName;
    use atm_storage::{HostName, TrustedPeer};
    use clap::Parser;
    use serial_test::serial;
    use tempfile::TempDir;

    const TEST_TEAM: &str = "test-team";

    fn send_command(to: &str, host: Option<&str>) -> SendCommand {
        SendCommand {
            to: to.to_string(),
            message: Some("hello".to_string()),
            team: Some(TEST_TEAM.to_string()),
            host: host.map(str::to_string),
            chat_id: None,
            actor: None,
            file: None,
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        }
    }

    fn trusted_peer(host: &str) -> TrustedPeer {
        TrustedPeer {
            host: host.parse::<HostName>().expect("host"),
            fingerprint: "a".repeat(64).parse().expect("fingerprint"),
            enabled: true,
            https_port: NonZeroU16::new(43101).expect("port"),
        }
    }

    fn known_teams() -> Vec<TeamName> {
        vec![
            TEST_TEAM.parse().expect("caller team"),
            "other-team".parse().expect("team"),
        ]
    }

    #[test]
    fn same_team_host_shorthand_builds_the_existing_canonical_target() {
        let command = send_command("arch-ctm@rand-m5", None);
        let target = command
            .target_with_peer_records(
                &TEST_TEAM.parse().expect("caller team"),
                &known_teams(),
                &[trusted_peer("rand-m5.local")],
            )
            .expect("same-team host shorthand");

        assert_eq!(target, "arch-ctm@test-team.rand-m5.local");
    }

    #[test]
    fn m_dns_suffix_and_hostname_case_are_cli_only_conveniences() {
        let command = send_command("arch-ctm@RAND-M5.LOCAL", None);
        let target = command
            .target_with_peer_records(
                &TEST_TEAM.parse().expect("caller team"),
                &known_teams(),
                &[trusted_peer("rand-m5.local")],
            )
            .expect("case-insensitive mDNS authority");

        assert_eq!(target, "arch-ctm@test-team.rand-m5.local");
    }

    #[test]
    fn dotted_m_dns_host_shorthand_is_not_mistaken_for_team_host_syntax() {
        let command = send_command("arch-ctm@fastpc4.radiant.local", None);
        let target = command
            .target_with_peer_records(
                &TEST_TEAM.parse().expect("caller team"),
                &known_teams(),
                &[trusted_peer("fastpc4.radiant.local")],
            )
            .expect("multi-label mDNS authority");

        assert_eq!(target, "arch-ctm@test-team.fastpc4.radiant.local");
    }

    #[test]
    fn explicit_remote_team_preserves_team_and_canonicalizes_host() {
        let command = send_command("arch-ctm@other-team.rand-m5", None);
        let target = command
            .target_with_peer_records(
                &TEST_TEAM.parse().expect("caller team"),
                &known_teams(),
                &[trusted_peer("rand-m5.local")],
            )
            .expect("explicit remote team host");

        assert_eq!(target, "arch-ctm@other-team.rand-m5.local");
    }

    #[test]
    fn explicit_team_host_form_wins_over_a_coincidentally_matching_full_hostname() {
        let command = send_command("arch-ctm@other-team.rand-m5.local", None);
        let target = command
            .target_with_peer_records(
                &TEST_TEAM.parse().expect("caller team"),
                &known_teams(),
                &[
                    trusted_peer("rand-m5.local"),
                    trusted_peer("other-team.rand-m5.local"),
                ],
            )
            .expect("explicit team host form");

        assert_eq!(target, "arch-ctm@other-team.rand-m5.local");
    }

    #[test]
    fn known_team_wins_over_a_matching_host_shorthand() {
        let command = send_command("arch-ctm@other-team", None);
        let target = command
            .target_with_peer_records(
                &TEST_TEAM.parse().expect("caller team"),
                &known_teams(),
                &[trusted_peer("other-team.local")],
            )
            .expect("known team retains legacy meaning");

        assert_eq!(target, "arch-ctm@other-team");
    }

    #[test]
    fn untrusted_and_ambiguous_host_shorthand_fail_with_recovery() {
        let caller_team = TEST_TEAM.parse().expect("caller team");
        let no_peer_error = send_command("arch-ctm@rand-m5", None)
            .target_with_peer_records(&caller_team, &known_teams(), &[])
            .expect_err("untrusted peer must fail closed");
        assert!(
            no_peer_error
                .to_string()
                .contains("neither a known local team nor an enabled trusted peer")
        );

        let ambiguous_error = send_command("arch-ctm@rand-m5", None)
            .target_with_peer_records(
                &caller_team,
                &known_teams(),
                &[trusted_peer("rand-m5.local"), trusted_peer("RAND-M5.LOCAL")],
            )
            .expect_err("ambiguous shorthand must fail closed");
        assert!(ambiguous_error.to_string().contains("ambiguous"));

        let no_fuzzy_error = send_command("arch-ctm@rand-m5.example", None)
            .target_with_peer_records(
                &caller_team,
                &known_teams(),
                &[trusted_peer("rand-m5.local")],
            )
            .expect_err("only terminal .local completion is permitted");
        assert!(
            no_fuzzy_error
                .to_string()
                .contains("no enabled trusted peer")
        );
    }

    #[test]
    fn explicit_host_flag_uses_the_same_trusted_authority_canonicalization() {
        let command = send_command("arch-ctm@test-team", Some("RAND-M5"));
        let target = command
            .target_with_peer_records(
                &TEST_TEAM.parse().expect("caller team"),
                &known_teams(),
                &[trusted_peer("rand-m5.local")],
            )
            .expect("canonicalized --host");

        assert_eq!(target, "arch-ctm@test-team.rand-m5.local");
    }

    #[test]
    fn caller_team_remains_known_when_the_roster_is_empty() {
        let command = send_command("arch-ctm@test-team", Some("rand-m5"));
        let target = command
            .target_with_peer_records(
                &TEST_TEAM.parse().expect("caller team"),
                &[],
                &[trusted_peer("rand-m5.local")],
            )
            .expect("caller team must not depend on a roster row");

        assert_eq!(target, "arch-ctm@test-team.rand-m5.local");
    }

    #[test]
    fn literal_ip_resolves_to_the_single_canonical_trusted_hostname() {
        let peers = [trusted_peer("rand-m5.local")];
        let canonical =
            resolve_trusted_ipv4_with_lookup(Ipv4Addr::new(192, 168, 1, 63), &peers, |_| {
                Ok(vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 63))])
            })
            .expect("single trusted authority");

        assert_eq!(canonical.as_str(), "rand-m5.local");
    }

    #[test]
    fn literal_ip_fails_closed_when_multiple_trusted_hosts_resolve_to_it() {
        let peers = [
            trusted_peer("rand-m5.local"),
            trusted_peer("fastpc4.radiant.local"),
        ];
        let error =
            resolve_trusted_ipv4_with_lookup(Ipv4Addr::new(192, 168, 1, 63), &peers, |_| {
                Ok(vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 63))])
            })
            .expect_err("shared IP must not choose a peer");

        assert!(error.to_string().contains("multiple enabled trusted peers"));
    }

    #[test]
    fn literal_ip_resolution_rejects_an_unbounded_enabled_peer_set() {
        let peers: Vec<TrustedPeer> = (0..33)
            .map(|index| trusted_peer(&format!("peer-{index}.local")))
            .collect();
        let error =
            resolve_trusted_ipv4_with_lookup(Ipv4Addr::new(192, 168, 1, 63), &peers, |_| {
                panic!("lookup must not begin after the preflight bound fails")
            })
            .expect_err("lookup set must remain bounded");

        assert!(error.to_string().contains("32-peer lookup bound"));
    }

    #[test]
    #[serial(env)]
    fn explicit_host_is_wire_equivalent_to_a_host_qualified_destination() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", Some(TEST_SENDER))]);
        let via_flag = send_command(&format!("{TEST_SENDER}@{TEST_TEAM}"), Some("localhost"))
            .build_request(".".into(), ".".into())
            .expect("explicit loopback host target");
        let via_destination = send_command(&format!("{TEST_SENDER}@{TEST_TEAM}.localhost"), None)
            .build_request(".".into(), ".".into())
            .expect("host-qualified destination");

        assert_eq!(
            via_flag.to.expect("flag target").to_string(),
            via_destination.to.expect("destination target").to_string(),
            "both forms must create the same host-qualified destination before shared self-send validation"
        );
    }

    #[test]
    #[serial(env)]
    fn explicit_localhost_qualifies_a_self_send_for_the_shared_guard() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", Some(TEST_SENDER))]);
        let request = send_command(&format!("{TEST_SENDER}@{TEST_TEAM}"), Some("localhost"))
            .build_request(".".into(), ".".into())
            .expect("same-host target");

        assert_eq!(
            request.to.expect("target").to_string(),
            format!("{TEST_SENDER}@{TEST_TEAM}.localhost")
        );
    }

    #[test]
    fn explicit_host_rejects_a_conflicting_destination_suffix() {
        let error = send_command("arch-ctm@test-team.rand-m5", Some("fastpc4"))
            .target_with_peer_records(
                &TEST_TEAM.parse().expect("caller team"),
                &known_teams(),
                &[trusted_peer("rand-m5.local"), trusted_peer("fastpc4.local")],
            )
            .expect_err("mismatched host selection must be rejected");

        assert!(
            error
                .to_string()
                .contains("recipient host and --host disagree")
        );
    }

    #[test]
    fn cli_accepts_the_explicit_host_flag() {
        crate::commands::Cli::try_parse_from([
            "atm",
            "send",
            "recipient-a@test-team",
            "hello",
            "--host",
            "localhost",
        ])
        .expect("documented explicit host command must parse");
    }

    #[test]
    #[serial(env)]
    fn build_request_rejects_invalid_target_before_core() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", Some(ROLE_TEAM_LEAD))]);
        let command = SendCommand {
            to: "../evil".to_string(),
            message: Some("hello".to_string()),
            team: Some(TEST_TEAM.to_string()),
            host: None,
            chat_id: None,
            actor: None,
            file: None,
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };

        let error = command
            .build_request(".".into(), ".".into())
            .expect_err("invalid target");

        assert!(error.to_string().contains("agent name"));
    }

    #[test]
    fn validation_errors_retain_their_actionable_recovery() {
        let command = SendCommand {
            to: "recipient@test-team".to_string(),
            message: Some("hello".to_string()),
            team: None,
            host: None,
            chat_id: None,
            actor: None,
            file: Some(PathBuf::from("message.txt")),
            stdin: true,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };
        let error = command
            .build_message_source(
                input::default_message_max_bytes(),
                std::path::Path::new("."),
            )
            .expect_err("invalid sources");
        assert!(
            error
                .to_string()
                .contains("Choose exactly one message source")
        );
    }

    #[test]
    fn build_message_source_rejects_conflicting_input_flags() {
        let stdin_and_file = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: None,
            team: None,
            host: None,
            chat_id: None,
            actor: None,
            file: Some(PathBuf::from("message.md")),
            stdin: true,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };
        let stdin_and_message = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: Some("hello".to_string()),
            team: None,
            host: None,
            chat_id: None,
            actor: None,
            file: None,
            stdin: true,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };

        let file_error = stdin_and_file
            .build_message_source(
                input::default_message_max_bytes(),
                std::path::Path::new("."),
            )
            .expect_err("stdin/file conflict");
        let message_error = stdin_and_message
            .build_message_source(
                input::default_message_max_bytes(),
                std::path::Path::new("."),
            )
            .expect_err("stdin/message conflict");

        assert!(file_error.to_string().contains(
            "Choose exactly one message source: either pass `--stdin` or `--file <path>`"
        ));
        assert!(message_error.to_string().contains(
            "Choose exactly one message source: either pass `--stdin` or provide positional message text"
        ));
    }

    #[test]
    fn build_message_source_requires_one_input_channel() {
        let command = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: None,
            team: None,
            host: None,
            chat_id: None,
            actor: None,
            file: None,
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };

        let error = command
            .build_message_source(
                input::default_message_max_bytes(),
                std::path::Path::new("."),
            )
            .expect_err("missing message");

        assert!(error.to_string().contains(
            "Pass positional message text, `--file <path>`, or `--stdin` before retrying `atm send`."
        ));
    }

    #[test]
    #[serial(env)]
    fn template_path_is_resolved_from_the_command_invocation_directory() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", Some(ROLE_TEAM_LEAD))]);
        let tempdir = TempDir::new().expect("tempdir");
        let template_dir = tempdir.path().join("templates");
        std::fs::create_dir_all(&template_dir).expect("template directory");
        std::fs::write(template_dir.join("notice.j2"), "hello {{ name }}")
            .expect("template fixture");
        let mut command = send_command("recipient-a@test-team", None);
        command.message = None;
        command.template = Some(PathBuf::from("templates/notice.j2"));
        command.var = vec!["name=Rand".to_string()];

        let source = command
            .build_message_source(input::default_message_max_bytes(), tempdir.path())
            .expect("template source");

        let SendMessageSource::Template(source) = source else {
            panic!("expected template source");
        };
        assert_eq!(
            source.canonical_template_path,
            std::fs::canonicalize(template_dir.join("notice.j2")).expect("canonical path")
        );
        assert_eq!(
            source.explicit_values.get("name"),
            Some(&serde_json::Value::String("Rand".to_string()))
        );
    }

    #[test]
    fn template_source_rejects_plain_message_and_invalid_classification() {
        let mut conflict = send_command("recipient-a@test-team", None);
        conflict.template = Some(PathBuf::from("notice.j2"));
        let error = conflict
            .build_message_source(input::default_message_max_bytes(), Path::new("."))
            .expect_err("template and positional message conflict");
        assert!(
            error
                .to_string()
                .contains("--template is mutually exclusive")
        );

        let error = super::validate_classification(&Some("Uppercase".to_string()), &[], &None)
            .expect_err("classification must be normalized");
        let error = error
            .downcast_ref::<atm_core::error::AtmError>()
            .expect("typed ATM error");
        assert_eq!(
            error.code(),
            atm_core::error::AtmErrorCode::TemplateClassificationInvalid
        );
    }

    #[test]
    #[serial(env)]
    fn classification_is_available_for_an_ordinary_send() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(ROLE_TEAM_LEAD)),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);
        let mut command = send_command("recipient-a@test-team", None);
        command.category = Some("assignment".to_owned());
        command.tag = vec!["phase-an,dev".to_owned()];
        command.content_format = Some("text.markdown".to_owned());

        let request = command
            .build_request(".".into(), ".".into())
            .expect("ordinary classified request");

        assert_eq!(
            request.classification.category.as_deref(),
            Some("assignment")
        );
        assert_eq!(request.classification.tags, ["phase-an", "dev"]);
        assert_eq!(
            request.classification.content_format.as_deref(),
            Some("text.markdown")
        );
    }

    #[test]
    #[serial(env)]
    fn build_request_preserves_cli_send_options() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(ROLE_TEAM_LEAD)),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);
        let command = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: Some("hello from send".to_string()),
            team: Some(TEST_TEAM.to_string()),
            host: None,
            chat_id: None,
            actor: None,
            file: None,
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: Some("summary".to_string()),
            requires_ack: true,
            task_id: Some("TASK-42".parse().expect("task id")),
            dry_run: true,
            json: true,
        };
        let tempdir = TempDir::new().expect("tempdir");

        let request = command
            .build_request(tempdir.path().join("home"), tempdir.path().join("cwd"))
            .expect("request");

        assert_eq!(Some(request.caller_identity.as_str()), Some(ROLE_TEAM_LEAD));
        assert_eq!(Some(request.caller_team.as_str()), Some(TEST_TEAM));
        assert_eq!(request.summary_override.as_deref(), Some("summary"));
        assert!(request.requires_ack);
        assert_eq!(
            request.task_id.as_ref().map(|value| value.as_str()),
            Some("TASK-42")
        );
        assert!(request.dry_run);
        assert_eq!(
            request.to.expect("destination").to_string(),
            "recipient-a@test-team"
        );
        match request.message_source {
            SendMessageSource::Inline(message) => assert_eq!(message, "hello from send"),
            other => panic!("expected inline message source, got {other:?}"),
        }
    }

    #[test]
    #[serial(env)]
    fn build_request_uses_environment_when_overrides_are_absent() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("sender-a")),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);
        let command = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: Some("hello".to_string()),
            team: None,
            host: None,
            chat_id: None,
            actor: None,
            file: None,
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };

        let request = command
            .build_request(".".into(), ".".into())
            .expect("request");

        assert_eq!(request.caller_identity.as_str(), "sender-a");
        assert_eq!(request.caller_team.as_str(), TEST_TEAM);
    }

    #[test]
    #[serial(env)]
    fn chat_id_and_equivalent_as_construct_the_same_caller() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);
        let base = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: Some("hello".to_string()),
            team: None,
            host: None,
            chat_id: Some("1234".to_string()),
            actor: None,
            file: None,
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };
        let explicit = SendCommand {
            chat_id: None,
            actor: Some(format!("{TEST_SENDER}:1234")),
            ..base
        };

        let shorthand = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: Some("hello".to_string()),
            team: None,
            host: None,
            chat_id: Some("1234".to_string()),
            actor: None,
            file: None,
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        }
        .build_request(".".into(), ".".into())
        .expect("shorthand request");
        let explicit = explicit
            .build_request(".".into(), ".".into())
            .expect("explicit request");

        assert_eq!(shorthand.caller_identity, explicit.caller_identity);
        assert_eq!(shorthand.caller_chat_id, explicit.caller_chat_id);
        assert_eq!(shorthand.caller_chat_id.unwrap().as_str(), "1234");
    }

    #[test]
    #[serial(env)]
    fn build_request_rejects_as_for_a_different_base_agent() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some(TEST_SENDER)),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);
        let command = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: Some("hello".to_string()),
            team: None,
            host: None,
            chat_id: None,
            actor: Some(format!("{TEST_SENDER}-other:1234")),
            file: None,
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };

        let error = command
            .build_request(".".into(), ".".into())
            .expect_err("different agent must be rejected");
        assert!(error.to_string().contains("same base agent"));
    }

    #[test]
    #[serial(env)]
    fn build_request_uses_environment_identity_even_with_team_override() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("env-sender")),
            ("ATM_TEAM", Some("env-team")),
        ]);
        let command = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: Some("hello".to_string()),
            team: Some(TEST_TEAM.to_string()),
            host: None,
            chat_id: None,
            actor: None,
            file: None,
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };

        let request = command
            .build_request(".".into(), ".".into())
            .expect("request");

        assert_eq!(request.caller_identity.as_str(), "env-sender");
        assert_eq!(request.caller_team.as_str(), TEST_TEAM);
    }

    #[test]
    #[serial(env)]
    fn build_request_supports_file_with_trailing_inline_note() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", Some(ROLE_TEAM_LEAD))]);
        let command = SendCommand {
            to: "recipient-a@test-team".to_string(),
            message: Some("note".to_string()),
            team: Some(TEST_TEAM.to_string()),
            host: None,
            chat_id: None,
            actor: None,
            file: Some(PathBuf::from("incident.md")),
            stdin: false,
            template: None,
            vars: None,
            var: Vec::new(),
            env_prefix: None,
            category: None,
            tag: Vec::new(),
            content_format: None,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };
        let tempdir = TempDir::new().expect("tempdir");

        let request = command
            .build_request(tempdir.path().join("home"), tempdir.path().join("cwd"))
            .expect("request");

        match request.message_source {
            SendMessageSource::File { path, message } => {
                assert_eq!(path, PathBuf::from("incident.md"));
                assert_eq!(message.as_deref(), Some("note"));
            }
            other => panic!("expected file message source, got {other:?}"),
        }
    }
}
