//! Transport-neutral values for the local analyst-query extension.

use std::time::Duration;

use crate::error::AtmError;

#[derive(Debug, Clone, PartialEq)]
pub enum AnalystQueryValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

pub type AnalystQueryRow = Vec<(String, AnalystQueryValue)>;

/// Executes one defensive, local-only analyst query through the selected
/// storage backend. Raw SQL is intentionally not an ATM transport capability.
pub trait AnalystQueryStore: Send + Sync {
    fn query(
        &self,
        sql: &str,
        parameters: &[AnalystQueryValue],
        deadline: Duration,
        max_rows: usize,
        max_result_bytes: usize,
    ) -> Result<Vec<AnalystQueryRow>, AtmError>;
}
