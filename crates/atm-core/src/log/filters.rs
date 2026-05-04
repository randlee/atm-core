//! Deferred retained-log filter helpers.
//!
//! ATM currently exposes its retained-log query model through
//! `crate::observability`. This module is reserved for future shared filter
//! helpers if multiple retained-log callers emerge.

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct DeferredLogFilterScope;
