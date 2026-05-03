use atm_core::roster_store::{
    PidUpdate, RosterMemberRecord, RosterRole, RosterStore, RosterStoreHealth, TransportKind,
};
use atm_core::store::{ProcessId, StoreError};
use atm_core::types::{AgentName, TeamName};
use rusqlite::Connection;
use rusqlite::OptionalExtension;

use crate::{
    RusqliteStore, classify_store_error, invalid_store_data, parse_optional, parse_required,
    table_exists,
};

#[derive(Debug)]
struct RawRosterRow {
    team_name: String,
    agent_name: String,
    role: String,
    transport_kind: String,
    host_name: String,
    recipient_pane_id: Option<String>,
    pid: Option<i64>,
    metadata_json: Option<String>,
}

type RosterMemberParams<'a> = (
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    Option<String>,
    Option<i64>,
    Option<&'a str>,
);

impl RosterStore for RusqliteStore {
    fn replace_roster(
        &self,
        team_name: &TeamName,
        members: &[RosterMemberRecord],
    ) -> Result<(), StoreError> {
        self.with_transaction(|transaction| {
            transaction
                .execute(
                    "DELETE FROM team_roster WHERE team_name = ?1",
                    [team_name.as_str()],
                )
                .map_err(|error| classify_store_error(error, "failed to clear existing roster"))?;
            for member in members {
                insert_roster_member_row(transaction, member).map_err(|error| {
                    classify_store_error(error, "failed to insert replacement roster member")
                })?;
            }
            Ok(())
        })
    }

    fn upsert_roster_member(
        &self,
        member: &RosterMemberRecord,
    ) -> Result<RosterMemberRecord, StoreError> {
        let connection = self.lock_connection()?;
        connection
            .execute(
                r#"
                INSERT INTO team_roster (
                    team_name,
                    agent_name,
                    role,
                    transport_kind,
                    host_name,
                    recipient_pane_id,
                    pid,
                    metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(team_name, agent_name) DO UPDATE SET
                    role = excluded.role,
                    transport_kind = excluded.transport_kind,
                    host_name = excluded.host_name,
                    recipient_pane_id = excluded.recipient_pane_id,
                    pid = excluded.pid,
                    metadata_json = excluded.metadata_json
                "#,
                roster_member_params(member),
            )
            .map_err(|error| classify_store_error(error, "failed to upsert roster member"))?;
        Ok(member.clone())
    }

    fn load_roster(&self, team_name: &TeamName) -> Result<Vec<RosterMemberRecord>, StoreError> {
        let connection = self.lock_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT team_name, agent_name, role, transport_kind, host_name, recipient_pane_id, pid, metadata_json FROM team_roster WHERE team_name = ?1 ORDER BY agent_name",
            )
            .map_err(|error| classify_store_error(error, "failed to prepare roster load query"))?;
        let rows = statement
            .query_map([team_name.as_str()], |row| {
                Ok(RawRosterRow {
                    team_name: row.get(0)?,
                    agent_name: row.get(1)?,
                    role: row.get(2)?,
                    transport_kind: row.get(3)?,
                    host_name: row.get(4)?,
                    recipient_pane_id: row.get(5)?,
                    pid: row.get(6)?,
                    metadata_json: row.get(7)?,
                })
            })
            .map_err(|error| classify_store_error(error, "failed to query roster rows"))?;

        let mut members = Vec::new();
        for row in rows {
            let raw =
                row.map_err(|error| classify_store_error(error, "failed to read roster row"))?;
            members.push(convert_roster_row(raw)?);
        }
        Ok(members)
    }

    fn update_member_pid(
        &self,
        team_name: &TeamName,
        agent_name: &AgentName,
        update: PidUpdate,
    ) -> Result<Option<RosterMemberRecord>, StoreError> {
        let connection = self.lock_connection()?;
        connection
            .execute(
                "UPDATE team_roster SET pid = ?1 WHERE team_name = ?2 AND agent_name = ?3",
                (update.pid.get(), team_name.as_str(), agent_name.as_str()),
            )
            .map_err(|error| classify_store_error(error, "failed to update roster PID"))?;

        connection
            .query_row(
                "SELECT team_name, agent_name, role, transport_kind, host_name, recipient_pane_id, pid, metadata_json FROM team_roster WHERE team_name = ?1 AND agent_name = ?2",
                (team_name.as_str(), agent_name.as_str()),
                |row| {
                    Ok(RawRosterRow {
                        team_name: row.get(0)?,
                        agent_name: row.get(1)?,
                        role: row.get(2)?,
                        transport_kind: row.get(3)?,
                        host_name: row.get(4)?,
                        recipient_pane_id: row.get(5)?,
                        pid: row.get(6)?,
                        metadata_json: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(|error| classify_store_error(error, "failed to reload roster member after pid update"))?
            .map(convert_roster_row)
            .transpose()
    }

    fn roster_health(&self) -> Result<RosterStoreHealth, StoreError> {
        let connection = self.lock_connection()?;
        Ok(RosterStoreHealth {
            team_roster_ready: table_exists(&connection, "team_roster")?,
        })
    }
}

fn insert_roster_member_row(
    connection: &Connection,
    member: &RosterMemberRecord,
) -> rusqlite::Result<()> {
    connection.execute(
        r#"
        INSERT INTO team_roster (
            team_name,
            agent_name,
            role,
            transport_kind,
            host_name,
            recipient_pane_id,
            pid,
            metadata_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        roster_member_params(member),
    )?;
    Ok(())
}

fn roster_member_params(member: &RosterMemberRecord) -> RosterMemberParams<'_> {
    (
        member.team_name.as_str(),
        member.agent_name.as_str(),
        member.role.as_str(),
        member.transport_kind.as_str(),
        member.host_name.as_str(),
        member.recipient_pane_id.as_ref().map(ToString::to_string),
        member.pid.map(ProcessId::get),
        member.metadata_json.as_deref(),
    )
}

fn convert_roster_row(raw: RawRosterRow) -> Result<RosterMemberRecord, StoreError> {
    Ok(RosterMemberRecord {
        team_name: parse_required(raw.team_name, "team_name")?,
        agent_name: parse_required(raw.agent_name, "agent_name")?,
        role: parse_required::<RosterRole>(raw.role, "role")?,
        transport_kind: parse_required::<TransportKind>(raw.transport_kind, "transport_kind")?,
        host_name: parse_required(raw.host_name, "host_name")?,
        recipient_pane_id: parse_optional(raw.recipient_pane_id, "recipient_pane_id")?,
        pid: raw
            .pid
            .map(|value| ProcessId::new(value).map_err(|error| invalid_store_data("pid", error)))
            .transpose()?,
        metadata_json: raw.metadata_json,
    })
}
