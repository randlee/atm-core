use std::time::Duration;

use crate::error::AtmError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchDeadline {
    remaining: Option<Duration>,
}

impl SearchDeadline {
    pub fn new(remaining: Duration) -> Result<Self, AtmError> {
        if remaining.is_zero() {
            return Err(AtmError::validation("search deadline must be non-zero"));
        }
        Ok(Self {
            remaining: Some(remaining),
        })
    }

    #[must_use]
    pub const fn unbounded() -> Self {
        Self { remaining: None }
    }

    #[must_use]
    pub const fn remaining(self) -> Option<Duration> {
        self.remaining
    }
}
