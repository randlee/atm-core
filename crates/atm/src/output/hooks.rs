use anyhow::Result;
use atm_core::boundary::{CronSchedule, HookRegistration, StopCondition};
use atm_core::doctor::hooks::HookDoctorEntry;
use atm_core::hook::HookActionsResponse;
use std::fmt::Write as _;

/// Print registrations as a human table or a bare JSON array.
pub fn print_hook_list(rows: &[HookRegistration], json: bool) -> Result<()> {
    print!("{}", render_hook_list(rows, json)?);
    Ok(())
}

pub(crate) fn render_hook_list(rows: &[HookRegistration], json: bool) -> Result<String> {
    if json {
        return Ok(format!("{}\n", serde_json::to_string(rows)?));
    }
    if rows.is_empty() {
        return Ok("none\n".into());
    }
    let mut output = "PHASE\tHOOK\tACTIVE\tMODULE\tCWD\tTIMEOUT_MS\tCUSTOM\tREGISTERED_BY\tREGISTERED_AT\tDISABLED_AT\n".to_owned();
    for row in rows {
        writeln!(
            output,
            "{}\t{}\t{}\t{}\t{}\t{}\t{} keys\t{}\t{}\t{}",
            row.phase,
            row.hook,
            row.active,
            row.module_path.display(),
            row.cwd.display(),
            row.timeout_ms,
            row.custom.len(),
            row.registered_by,
            row.registered_at,
            row.disabled_at
                .map(|at| at.to_string())
                .unwrap_or_else(|| "-".into())
        )?;
    }
    Ok(output)
}

/// Print schedules, including their stop conditions and persisted outcomes.
pub fn print_cron_list(rows: &[CronSchedule], json: bool) -> Result<()> {
    print!("{}", render_cron_list(rows, json)?);
    Ok(())
}

pub(crate) fn render_cron_list(rows: &[CronSchedule], json: bool) -> Result<String> {
    if json {
        return Ok(format!("{}\n", serde_json::to_string(rows)?));
    }
    if rows.is_empty() {
        return Ok("none\n".into());
    }
    let mut output = "PHASE\tNAME\tACTIVE\tCRON\tJITTER_S\tMODULE\tARGS\tTIMEOUT_MS\tCUSTOM\tSTOP\tLAST_RUN_AT\tLAST_OUTCOME\n".to_owned();
    for row in rows {
        let stop = match row.stop {
            StopCondition::Hook => "hook".into(),
            StopCondition::Until { at } => format!(
                "until {}",
                at.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
            ),
        };
        writeln!(
            output,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{} keys\t{}\t{}\t{}",
            row.phase,
            row.name,
            row.active,
            row.cron_expr,
            row.jitter_s,
            row.module_path.display(),
            serde_json::to_string(&row.args)?,
            row.timeout_ms,
            row.custom.len(),
            stop,
            row.last_run_at
                .map(|at| at.to_string())
                .unwrap_or_else(|| "-".into()),
            row.last_outcome.as_deref().unwrap_or("-")
        )?;
    }
    Ok(output)
}

/// Render module doctor replies without modifying the report summary.
pub fn print_doctor_hooks(entries: &[HookDoctorEntry]) -> Vec<String> {
    if entries.is_empty() {
        return vec!["Hooks: none".into()];
    }
    let mut lines = vec!["Hooks:".into()];
    for entry in entries {
        let message = entry
            .error
            .as_ref()
            .map(|error| error.message.as_str())
            .or(entry.message.as_deref())
            .unwrap_or("no message");
        lines.push(format!(
            "{}/{} {}: {}",
            entry.team,
            entry.phase,
            entry.module_path.display(),
            message
        ));
        for finding in &entry.findings {
            let severity = serde_json::to_value(finding.severity).expect("severity serializes");
            lines.push(format!(
                "  {} {}: {}",
                severity.as_str().unwrap_or("unknown"),
                finding.code,
                finding.message
            ));
        }
    }
    lines
}

/// Describe hook actions that replace a normal task command result.
pub fn print_hook_actions(response: &HookActionsResponse) -> String {
    format!("hook ran {} actions", response.dispatched)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_human_output_renders_hooks_findings_and_none() {
        let success: HookDoctorEntry = serde_json::from_value(serde_json::json!({
            "module_path":"hook.py","cwd":"/repo","team":"bk","phase":"bk","hooks":["task.close"],"schedules":[],
            "timeout_ms":2500,"success":true,"message":"ok","findings":[{"severity":"warning","code":"CUSTOM_WARNING","message":"needs attention"}]
        })).unwrap();
        let mut failure = success.clone();
        failure.message = None;
        failure.findings.clear();
        failure.error = Some(atm_core::doctor::hooks::HookDoctorError {
            code: atm_core::error::AtmErrorCode::HookMalformed,
            message: "doctor broken".into(),
        });
        let lines = print_doctor_hooks(&[success, failure]);
        assert_eq!(
            lines,
            [
                "Hooks:",
                "bk/bk hook.py: ok",
                "  warning CUSTOM_WARNING: needs attention",
                "bk/bk hook.py: doctor broken"
            ]
        );
        assert_eq!(print_doctor_hooks(&[]), ["Hooks: none"]);
    }
}
