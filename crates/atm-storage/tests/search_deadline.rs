use atm_storage::search::SearchDeadline;
use std::time::Duration;

#[test]
fn search_deadline_supports_explicit_unbounded_execution() {
    assert_eq!(SearchDeadline::unbounded().remaining(), None);
    assert_eq!(
        SearchDeadline::new(Duration::from_millis(1))
            .expect("positive deadline")
            .remaining(),
        Some(Duration::from_millis(1))
    );
    assert!(SearchDeadline::new(Duration::ZERO).is_err());
}
