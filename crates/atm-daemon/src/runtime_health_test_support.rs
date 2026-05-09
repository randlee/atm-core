use super::*;

impl RuntimeStatusCache {
    pub(crate) fn member_state_for_test(
        &self,
        team: &TeamName,
        member: &AgentName,
    ) -> Result<Option<RuntimeMemberState>, AtmError> {
        let cache = self
            .state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("runtime status cache lock poisoned"))?;
        Ok(cache
            .members
            .get(&RuntimeMemberKey {
                team: team.clone(),
                member: member.clone(),
            })
            .map(|record| record.state))
    }

    pub(crate) fn hydrate_member_for_test(
        &self,
        team: TeamName,
        member: AgentName,
        pid: Option<u32>,
    ) -> Result<(), AtmError> {
        let mut cache = self
            .state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("runtime status cache lock poisoned"))?;
        let key = RuntimeMemberKey { team, member };
        cache.members.entry(key).or_insert(RuntimeMemberRecord {
            pid,
            state: RuntimeMemberState::Unknown,
            last_active_at: None,
        });
        Ok(())
    }

    pub(crate) fn insert_member_for_test(
        &self,
        team: TeamName,
        member: AgentName,
        pid: Option<u32>,
        state: RuntimeMemberState,
        last_active_at: Option<IsoTimestamp>,
    ) -> Result<(), AtmError> {
        let mut cache = self
            .state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("runtime status cache lock poisoned"))?;
        cache.members.insert(
            RuntimeMemberKey { team, member },
            RuntimeMemberRecord {
                pid,
                state,
                last_active_at,
            },
        );
        Ok(())
    }
}

impl DaemonRequestDispatcher {
    pub(crate) fn new_for_test(
        home_dir: PathBuf,
        status_cache: RuntimeStatusCache,
        roster_db_path: PathBuf,
    ) -> Self {
        let sqlite_boundary = match atm_rusqlite::assemble_boundary(&roster_db_path) {
            Ok(boundary) => {
                if let Err(error) =
                    build_runtime_status_cache_state(None, &home_dir, boundary.roster_store())
                        .and_then(|state| status_cache.replace_state(state))
                {
                    tracing::warn!(
                        %error,
                        "failed to hydrate test runtime status cache from sqlite roster state"
                    );
                    status_cache.mark_sqlite_unavailable();
                }
                Some(boundary)
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    path = %roster_db_path.display(),
                    "failed to assemble sqlite boundary for test daemon runtime health"
                );
                status_cache.mark_sqlite_unavailable();
                None
            }
        };
        Self {
            observability: DaemonObservability::new_with_sink_fault(
                home_dir,
                RetainedSinkFault::Healthy,
            ),
            status_cache,
            sqlite_boundary,
        }
    }
}
