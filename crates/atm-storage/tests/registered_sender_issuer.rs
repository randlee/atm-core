#[test]
fn external_inspection_issuer_is_authorized_but_cannot_issue_runtime_admission() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/registered_sender_issuer_authorized.rs");
    cases.compile_fail("tests/ui/registered_sender_issuer_cannot_admit.rs");
}
