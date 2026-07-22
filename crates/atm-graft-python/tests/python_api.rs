//! Integration marker for the public Python-binding crate.
//!
//! Behavioral coverage lives beside the binding so it can construct the
//! private conversion helpers without opening a daemon connection.

#[test]
fn python_binding_crate_is_linkable() {
    assert_eq!(env!("CARGO_PKG_NAME"), "atm-graft-python");
}
