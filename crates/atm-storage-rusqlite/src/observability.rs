use atm_storage::{AtmError, AtmErrorCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqliteObservabilityOutcome {
    Failed,
    Timeout,
}

impl SqliteObservabilityOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::Timeout => "timeout",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SqliteObservabilityEvent {
    pub action: &'static str,
    pub outcome: SqliteObservabilityOutcome,
    pub message: String,
    pub error_code: Option<AtmErrorCode>,
}

impl SqliteObservabilityEvent {
    pub fn new(
        action: &'static str,
        outcome: SqliteObservabilityOutcome,
        message: impl Into<String>,
        error_code: Option<AtmErrorCode>,
    ) -> Self {
        Self {
            action,
            outcome,
            message: message.into(),
            error_code,
        }
    }
}

pub trait SqliteObservability: Send + Sync {
    fn emit(&self, event: SqliteObservabilityEvent) -> Result<(), AtmError>;

    fn emit_or_warn(&self, event: SqliteObservabilityEvent) {
        if let Err(error) = self.emit(event.clone()) {
            tracing::warn!(
                %error,
                action = event.action,
                outcome = event.outcome.as_str(),
                event_message = %event.message,
                error_code = ?event.error_code,
                "sqlite subsystem observability emission failed"
            );
        }
    }
}

#[cfg(test)]
#[derive(Debug, Default)]
pub struct NullSqliteObservability;

#[cfg(test)]
impl SqliteObservability for NullSqliteObservability {
    fn emit(&self, _event: SqliteObservabilityEvent) -> Result<(), AtmError> {
        Ok(())
    }
}

/// Deliberately passive adapter for library-only construction paths. The
/// daemon composition root injects its retained tracing adapter instead; this
/// distinct name makes a missing production injection mechanically visible.
#[derive(Debug, Default)]
pub(crate) struct PassiveSqliteObservability;

impl SqliteObservability for PassiveSqliteObservability {
    fn emit(&self, _event: SqliteObservabilityEvent) -> Result<(), AtmError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    fn rust_sources(root: &Path, sources: &mut Vec<std::path::PathBuf>) {
        for entry in fs::read_dir(root).expect("storage source directory is readable") {
            let path = entry.expect("source entry").path();
            if path.is_dir() {
                rust_sources(&path, sources);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push(path);
            }
        }
    }

    #[test]
    fn ac8_null_sqlite_observability_is_test_only_in_the_source_contract() {
        let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut sources = Vec::new();
        rust_sources(&source_root, &mut sources);

        for path in sources {
            let source = fs::read_to_string(&path).expect("Rust source is UTF-8");
            let test_module = source.find("#[cfg(test)]\nmod tests {");
            let lines = source.lines().collect::<Vec<_>>();
            for (index, line) in lines.iter().enumerate() {
                if !line.contains("NullSqliteObservability") {
                    continue;
                }
                let prior = lines[..index]
                    .iter()
                    .rev()
                    .find(|prior| !prior.trim().is_empty())
                    .map(|prior| prior.trim());
                let in_test_module =
                    test_module.is_some_and(|start| source[..start].lines().count() <= index);
                let nearby_test_gate = lines[..index]
                    .iter()
                    .rev()
                    .take(4)
                    .any(|prior| prior.trim() == "#[cfg(test)]");
                let is_definition = path
                    .file_name()
                    .is_some_and(|name| name == "observability.rs")
                    && (line.contains("struct NullSqliteObservability")
                        || line.contains("impl SqliteObservability for NullSqliteObservability")
                        || in_test_module);
                assert!(
                    is_definition
                        || in_test_module
                        || prior == Some("#[cfg(test)]")
                        || nearby_test_gate,
                    "non-test NullSqliteObservability reference in {}:{}",
                    path.strip_prefix(&source_root)
                        .expect("source remains below source root")
                        .display(),
                    index + 1
                );
            }
        }
    }
}
