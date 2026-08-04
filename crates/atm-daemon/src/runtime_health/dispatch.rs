use super::*;
use atm_core::types::IsoTimestamp;

/// An activity observation whose local ingress proof was checked by the API
/// router.  Its constructor is intentionally private to this shared dispatch
/// path, so peer and smoke transports cannot reach the cache touch API.
pub(crate) struct TrustedActivityObservation(atm_core::caller_context::ActivityObservation);

impl TrustedActivityObservation {
    fn from_local(
        ingress: AuthenticatedIngress,
        observation: Option<atm_core::caller_context::ActivityObservation>,
    ) -> Option<Self> {
        (ingress == AuthenticatedIngress::Local)
            .then_some(observation)
            .flatten()
            .map(Self)
    }

    pub(crate) fn observation(&self) -> &atm_core::caller_context::ActivityObservation {
        &self.0
    }
}

pub(crate) struct MessageRecord {
    pub(crate) prepared: PreparedWrite,
    pub(crate) outbound_request: WriteRequest,
}

trait MessageWriter: Send + Sync {
    fn write(&self, request: WriteRequest) -> Result<MessageRecord, AtmError>;
}

pub(crate) trait PostWriteRouter: Send + Sync {
    /// Non-blocking, infallible post-commit scheduling. A committed admission
    /// response is constructed before this signal and cannot be relabelled by
    /// worker availability or notification delivery.
    fn dispatch(&self, message: &mut MessageRecord);
}

impl DaemonRequestDispatcher {
    #[cfg(test)]
    pub(crate) fn dispatch(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        self.dispatch_with_deadline(
            request,
            AuthenticatedIngress::Local,
            RequestDeadline::after(Duration::from_secs(5)),
        )
    }

    pub(crate) fn dispatch_with_deadline(
        &self,
        request: RequestEnvelope,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError> {
        let side_effecting = request_may_have_side_effects(&request);
        require_dispatch_budget(deadline, false)?;
        let response = match request {
            RequestEnvelope::Write(request) => self.route_write(*request, ingress),
            request => self.dispatch_non_write(request, ingress),
        }?;
        require_dispatch_budget(deadline, side_effecting)?;
        Ok(response)
    }

    fn route_write(
        &self,
        request: WriteRequest,
        ingress: AuthenticatedIngress,
    ) -> Result<ResponseEnvelope, AtmError> {
        let observation =
            TrustedActivityObservation::from_local(ingress, request.activity_observation.clone());
        let mut message = MessageWriter::write(self, request)?;
        let requires_post_commit_signal = message.prepared.requires_post_write_route();
        // Admission is complete before any post-commit work is even signalled.
        // In particular, an acknowledgement's source transition completes here,
        // before the peer worker can observe or deliver the reply.
        let outcome = message
            .prepared
            .finish(&self.service_runtime, self.observability.as_ref())?;
        let response = match outcome {
            WriteOutcome::Sent(outcome) => {
                ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome))
            }
            WriteOutcome::Acknowledged(outcome) => {
                ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome))
            }
        };
        if let Some(observation) = observation.as_ref() {
            self.status_cache
                .touch_member(observation, IsoTimestamp::now());
        }
        if requires_post_commit_signal {
            PostWriteRouter::dispatch(self, &mut message);
        }
        Ok(response)
    }

    fn persist_local_write(&self, request: WriteRequest) -> Result<PreparedWrite, AtmError> {
        self.admission_runtime_view
            .prepare_write(request, self.observability.as_ref())
    }

    fn dispatch_non_write(
        &self,
        request: RequestEnvelope,
        ingress: AuthenticatedIngress,
    ) -> Result<ResponseEnvelope, AtmError> {
        match request {
            RequestEnvelope::Heartbeat(request) => {
                Ok(ResponseEnvelope::Heartbeat(self.record_heartbeat(request)?))
            }
            RequestEnvelope::CompatibilityPreflight(preflight) => Ok(
                ResponseEnvelope::CompatibilityVerdict(Self::compatibility_verdict(preflight)?),
            ),
            RequestEnvelope::List(query) => Ok(ResponseEnvelope::List(list_mail(
                query,
                self.observability.as_ref(),
            )?)),
            RequestEnvelope::Peek(query) => Ok(ResponseEnvelope::Peek(Box::new(
                peek_mail_with_runtime(query, self.observability.as_ref(), &self.service_runtime)?,
            ))),
            RequestEnvelope::Receive(query) => {
                let observation = TrustedActivityObservation::from_local(
                    ingress,
                    query.activity_observation.clone(),
                );
                let response = read_mail_with_runtime(
                    query,
                    self.observability.as_ref(),
                    &self.service_runtime,
                )?;
                if let Some(observation) = observation.as_ref() {
                    self.status_cache
                        .touch_member(observation, IsoTimestamp::now());
                }
                Ok(ResponseEnvelope::Receive(Box::new(response)))
            }
            RequestEnvelope::Clear(query) => Ok(ResponseEnvelope::Clear(clear_mail_with_runtime(
                query,
                self.observability.as_ref(),
                &self.service_runtime,
            )?)),
            RequestEnvelope::Doctor(query) => Ok(ResponseEnvelope::Doctor(Box::new(
                self.project_doctor_report(query)?,
            ))),
            RequestEnvelope::PeerSync(request) => {
                Ok(ResponseEnvelope::PeerSync(self.sync_peer(request)?))
            }
            RequestEnvelope::ReloadRuntimeView => {
                self.reload_runtime_view()?;
                Ok(ResponseEnvelope::RuntimeViewReloaded)
            }
            RequestEnvelope::Write(_) => unreachable!("writes are handled by route_write"),
        }
    }
}

/// The router owns one absolute request budget across validation, persistence,
/// and response construction.  Work that has already crossed a mutable
/// boundary is allowed to finish for consistency, but its caller receives the
/// durable retry-safe uncertainty instead of a stale success response.
fn require_dispatch_budget(
    deadline: RequestDeadline,
    side_effecting_work_may_have_started: bool,
) -> Result<(), AtmError> {
    if !deadline.expired() {
        return Ok(());
    }
    if side_effecting_work_may_have_started {
        return Err(AtmError::daemon_may_have_executed(
            "daemon request exceeded its shared deadline after side-effecting dispatch work may have started",
        ));
    }
    Err(AtmError::daemon_unavailable(
        "daemon request exceeded its shared deadline before dispatch work started",
    ))
}

fn request_may_have_side_effects(request: &RequestEnvelope) -> bool {
    !matches!(
        request,
        RequestEnvelope::CompatibilityPreflight(_)
            | RequestEnvelope::List(_)
            | RequestEnvelope::Peek(_)
            | RequestEnvelope::Doctor(_)
    )
}

impl MessageWriter for DaemonRequestDispatcher {
    fn write(&self, request: WriteRequest) -> Result<MessageRecord, AtmError> {
        self.persist_local_write(request).map(|prepared| {
            let message_id = prepared.persisted_message_id();
            MessageRecord {
                outbound_request: prepared
                    .outbound_request()
                    .with_origin_metadata(message_id, prepared.persisted_timestamp()),
                prepared,
            }
        })
    }
}

impl DaemonRequestDispatcher {
    /// The canonical route and future recovery coordination use this sole
    /// event-to-projection writer; no transport adapter owns delivery state.
    pub(crate) fn record_peer_delivery_event(&self, event: PeerDeliveryEvent) {
        self.peer_delivery_projection
            .record(event, &self.runtime_health_observability);
    }

    pub(crate) fn peer_link_statuses(&self) -> Vec<atm_core::doctor::PeerLinkStatus> {
        self.peer_delivery_projection
            .statuses(self.peer_config_store.as_ref())
    }
}

impl DaemonRequestDispatcher {
    fn sync_peer(&self, request: PeerSyncRequest) -> Result<PeerSyncOutcome, AtmError> {
        let outcome = self.peer_delivery_coordinator.sync_peer(
            &request.peer,
            RequestDeadline::after(PEER_SYNC_REQUEST_DEADLINE),
        )?;
        match outcome {
            DrainPeerSyncOutcome::Confirmed { delivered } => Ok(PeerSyncOutcome {
                peer: request.peer,
                delivered,
                disposition: PeerSyncDisposition::Completed,
            }),
            DrainPeerSyncOutcome::Unconfirmed { code } => Err(AtmError::new(
                code,
                "peer synchronization completed with unconfirmed remote delivery",
            )),
            DrainPeerSyncOutcome::Expired { code } => Err(AtmError::new(
                code,
                "peer synchronization request deadline expired before confirmation",
            )),
        }
    }
}

impl DaemonRequestDispatcher {
    fn compatibility_verdict(
        preflight: atm_core::protocol::CompatibilityPreflight,
    ) -> Result<CompatibilityVerdict, AtmError> {
        let daemon_release = ReleaseVersion::current();
        let daemon_schema_version = atm_core::protocol::CLI_SCHEMA_VERSION;
        let daemon_http_api_version = atm_core::protocol::HttpApiVersion::current();
        if preflight.cli_schema_version == daemon_schema_version
            && preflight.http_api_version.major() == daemon_http_api_version.major()
        {
            return Ok(CompatibilityVerdict::Compatible {
                daemon_release,
                daemon_schema_version,
                daemon_http_api_version,
            });
        }
        Ok(CompatibilityVerdict::Incompatible {
            client_release: preflight.client_release,
            daemon_release,
            client_schema_version: preflight.cli_schema_version,
            daemon_schema_version,
            client_http_api_version: preflight.http_api_version,
            daemon_http_api_version,
            code: AtmErrorCode::ClientDaemonVersionIncompatible,
        })
    }

    pub(crate) fn reload_runtime_view(&self) -> Result<(), AtmError> {
        let roster_store = self.roster_store.as_ref().cloned().ok_or_else(|| {
            self.runtime_health_observability.emit_or_warn(
                "reload_unavailable",
                "failed",
                "daemon runtime reload is unavailable because the roster store is not assembled",
            );
            AtmError::daemon_unavailable(
                "daemon runtime reload is unavailable because the roster store is not assembled",
            )
        })?;
        let current_state = self.status_cache.clone_state();
        let next_state =
            build_runtime_status_cache_state(Some(&current_state), roster_store.as_ref())?;
        self.refresh_https_trust()?;
        self.admission_runtime_view
            .reload(self.service_runtime.clone());
        let reloaded_members = next_state.member_count();
        self.status_cache.publish_state(next_state);
        tracing::info!(
            reloaded_members,
            "bounded daemon config/roster reload applied successfully"
        );
        Ok(())
    }

    pub(crate) fn install_runtime_reload_hook(
        &self,
        hook: RuntimeReloadHook,
    ) -> Result<(), AtmError> {
        let mut slot = lock_runtime_mutex(&self.runtime_reload_hook, "daemon runtime reload hook")?;
        *slot = Some(hook);
        Ok(())
    }

    fn refresh_https_trust(&self) -> Result<(), AtmError> {
        let hook =
            lock_runtime_mutex(&self.runtime_reload_hook, "daemon runtime reload hook")?.clone();
        if let Some(hook) = hook {
            hook()?;
        }
        Ok(())
    }

    pub(crate) fn finalize_observability_shutdown(&self) {
        let observability = self.observability.clone();
        Self::run_bounded_shutdown_step(
            "observability_flush",
            SHUTDOWN_OBSERVABILITY_FLUSH_DEADLINE,
            // The finalizer step already runs on a dedicated shutdown thread,
            // so the retained-log flush remains in a sync context.
            move || observability.best_effort_flush_blocking(),
        );
    }

    pub(crate) fn preflush_observability_shutdown(&self) {
        let observability = self.observability.clone();
        Self::run_bounded_shutdown_step(
            "observability_preflush",
            SHUTDOWN_OBSERVABILITY_FLUSH_DEADLINE,
            move || observability.best_effort_preflush_blocking(),
        );
    }

    fn record_heartbeat(
        &self,
        request: TeamMemberHeartbeatRequest,
    ) -> Result<TeamMemberHeartbeatResponse, AtmError> {
        let roster_store = self.roster_store.as_ref().cloned().ok_or_else(|| {
            self.runtime_health_observability.emit_or_warn(
                "heartbeat_unavailable",
                "failed",
                "daemon heartbeats are unavailable because the roster store is not assembled",
            );
            AtmError::daemon_unavailable(
                "daemon heartbeats are unavailable because the roster store is not assembled",
            )
        })?;
        let membership = roster_store
            .load_roster(&request.team)?
            .members
            .into_iter()
            .find(|entry| entry.agent_name == request.member);
        if membership.is_none() {
            return Err(AtmError::agent_not_found(
                request.member.as_str(),
                request.team.as_str(),
            ));
        }
        let cached_pid = self.status_cache.cached_pid(&request.team, &request.member);
        if let Some(existing_pid) = cached_pid.filter(|pid| *pid != request.pid)
            && process_is_alive(existing_pid)
        {
            self.status_cache
                .record_identity_conflict(&request, existing_pid);
            return Err(AtmError::identity_conflict(
                "ATM_IDENTITY_CONFLICT: stop and report to user immediately",
            ));
        }
        Ok(self
            .status_cache
            .record_heartbeat(&request, cached_pid.is_some_and(|pid| pid != request.pid)))
    }

    fn project_doctor_report(&self, query: DoctorQuery) -> Result<DoctorReport, AtmError> {
        let daemon_observability_finding = match self.observability.health() {
            Ok(health) => daemon_observability_finding(&health),
            Err(error) => doctor::health::observability_finding_from_error(&error),
        };
        let (peer_config, mut peer_findings) =
            doctor::peer_config_doctor_report(self.peer_config_store.as_ref());
        peer_findings.insert(0, daemon_observability_finding);
        let daemon_runtime = DaemonRuntimeDoctorReport {
            findings: peer_findings,
            peer_config: Some(peer_config),
            peer_links: self.peer_link_statuses(),
            peer_wire_security: None,
        };
        let mut report = doctor::run_doctor_with_runtime_ports(
            query,
            self.observability.as_ref(),
            &self.service_runtime,
            &self.doctor_ports,
            Some(daemon_runtime),
        )?;
        let runtime_status = match &report.member_roster {
            Some(roster) => self.status_cache.snapshot_for_members(
                roster
                    .members
                    .iter()
                    .map(|member| (roster.team.clone(), member.name.clone())),
            ),
            None => self.status_cache.snapshot(),
        };
        let runtime_status_finding = runtime_status_finding(&runtime_status);
        report.findings.push(runtime_status_finding.clone());
        if let Some(daemon_runtime) = report.daemon_runtime.as_mut() {
            daemon_runtime.findings.push(runtime_status_finding);
        } else {
            report.daemon_runtime = Some(DaemonRuntimeDoctorReport {
                findings: vec![runtime_status_finding],
                peer_config: None,
                peer_links: Vec::new(),
                peer_wire_security: None,
            });
        }
        report.runtime_status = Some(runtime_status);
        // This is existing doctor-only launch context. Client context remains
        // request-scoped and is reported separately.
        report.daemon_context = Some(DoctorExecutionContext {
            team: atm_core::caller_context::read_cli_team_from_env_or_warn(
                "atm_daemon::runtime_health::daemon_context",
            ),
            identity: atm_core::caller_context::read_cli_identity_from_env_or_warn(
                "atm_daemon::runtime_health::daemon_context",
            ),
            version: Some(ReleaseVersion::current()),
            cli_schema_version: Some(atm_core::protocol::CLI_SCHEMA_VERSION),
            http_api_version: Some(atm_core::protocol::HttpApiVersion::current()),
        });
        finalize_doctor_report(&mut report);
        Ok(report)
    }
}

impl ApiRouter for DaemonRequestDispatcher {
    fn route(
        &self,
        request: ApiRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        if deadline.expired() {
            return Err(AtmError::daemon_unavailable(
                "daemon API request exceeded its same-host deadline before routing",
            ));
        }
        let mut request = request.into_inner();
        if let RequestEnvelope::Write(write) = &mut request {
            if ingress == AuthenticatedIngress::Local {
                // The local IPC payload is caller-controlled. Peer provenance is
                // established only by the HTTPS adapter after authentication.
                // Strip a local claim before applying the canonical provenance gate.
                write.authenticated_source_host = None;
            }
            let write_ingress = match &ingress {
                AuthenticatedIngress::Local => WriteIngress::Local,
                AuthenticatedIngress::Peer => WriteIngress::Peer,
                AuthenticatedIngress::UntrustedSmoke(_) => WriteIngress::UntrustedSmoke,
                AuthenticatedIngress::AnonymousSmoke => WriteIngress::AnonymousSmoke,
            };
            validate_write_provenance(
                write_ingress,
                WriteProvenance {
                    target_host: write.to.as_ref().and_then(|address| address.host()),
                    authenticated_source_host: write.authenticated_source_host.as_ref(),
                    origin_message_id: write.origin_message_id.is_some(),
                    origin_timestamp: write.origin_timestamp.is_some(),
                },
            )?;
        }
        if matches!(request, RequestEnvelope::ReloadRuntimeView)
            && ingress != AuthenticatedIngress::Local
        {
            return Err(AtmError::validation(
                "runtime reload is available only through authenticated local IPC",
            ));
        }
        self.dispatch_with_deadline(request, ingress, deadline)
            .map(ApiResponse::new)
    }
}
