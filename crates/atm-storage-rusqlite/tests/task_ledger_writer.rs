mod coverage {
    include!("task_identity.rs");
}

macro_rules! writer_contract_test {
    ($name:ident) => {
        #[test]
        fn $name() {
            coverage::$name();
        }
    };
}

writer_contract_test!(start_by_assignee_moves_assigned_task_to_head_and_active);
writer_contract_test!(start_by_non_assignee_is_rejected);
writer_contract_test!(start_on_complete_task_is_rejected);
writer_contract_test!(start_while_another_task_is_active_is_rejected);
writer_contract_test!(duplicate_start_is_rejected_nothing_delivered_one_rejected_row);
writer_contract_test!(rejected_start_rolls_back_report_state_and_projection_atomically);
writer_contract_test!(concurrent_starts_admit_exactly_one_started_event);
writer_contract_test!(start_racing_reassign_is_rejected_as_not_counterparty);
writer_contract_test!(start_of_missing_task_appends_null_state_rejected_row);
writer_contract_test!(move_of_active_task_appends_moved_head_to_head);
writer_contract_test!(close_of_complete_task_retains_report_and_strips_link);
writer_contract_test!(close_by_non_party_is_rejected_and_retained);
writer_contract_test!(move_of_complete_task_appends_rejected_row);
writer_contract_test!(start_without_prior_reminder_succeeds);
writer_contract_test!(daemon_actor_can_no_longer_start_a_task);
