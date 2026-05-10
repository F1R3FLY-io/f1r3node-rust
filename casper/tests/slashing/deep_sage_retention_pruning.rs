fn evidence_retained(retention_window: u64, slash_delay: u64) -> bool {
    retention_window >= slash_delay
}

#[test]
fn uc_95_retention_window_must_cover_slash_delay() {
    assert!(!evidence_retained(4, 5));
    assert!(evidence_retained(5, 5));
    assert!(evidence_retained(6, 5));
}

#[test]
fn uc_95_safe_retention_prevents_pruning_exploit() {
    let slash_delay = 8;
    let configured_retention = 8;
    assert!(evidence_retained(configured_retention, slash_delay));
}
