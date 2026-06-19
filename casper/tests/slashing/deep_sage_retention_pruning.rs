// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-95 — Retention-window-must-cover-slash-delay (pruning exploit).
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-95.
// Threat class: Storage-retention (pruning) exploit (Sage row
// `record_normalization_model.sage`).
// Reference: formal/sage/record_normalization_model.sage,
// formal/sage/slashing/FINDINGS.md.
//
// Sage finding: an operator may set `retention_window < slash_delay`
// (e.g. 4-block retention with 5-block slash delay) under disk-pressure.
// In that misconfiguration, evidence may be pruned before its slash is
// pending, silently losing accountability. The invariant: the retention
// window must be >= the slash delay. This file pins both the broken
// (4 < 5) and the safe (5 >= 5, 6 > 5) configurations.

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
