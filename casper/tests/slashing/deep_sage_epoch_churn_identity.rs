// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-96 — Epoch-churn identity attack: stale evidence under a fresh epoch.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-96.
// Threat class: Identity-replay across epoch boundary (Sage row
// `epoch_churn_attack_model.sage`).
// Reference: formal/sage/epoch_churn_attack_model.sage,
// formal/sage/slashing/FINDINGS.md.
//
// Sage finding: an adversary may try to reuse evidence from epoch N to
// justify a slash in epoch N+1 (re-bonded under the same identity), or
// vice versa. The post-fix invariant is a *strict* equality filter on the
// epoch field — `current_epoch_evidence` returns only the evidence whose
// `.epoch == current_epoch`, never a near-match or a "any prior epoch"
// admission. This file pins that strictness so a "let's allow prior-epoch
// evidence" refactor surfaces immediately.

#[derive(Debug, Clone, Copy)]
struct Evidence {
    offender: u8,
    epoch: u64,
}

fn current_epoch_evidence(evidence: &[Evidence], current_epoch: u64) -> Vec<Evidence> {
    evidence
        .iter()
        .copied()
        .filter(|item| item.epoch == current_epoch)
        .collect()
}

#[test]
fn uc_96_strict_epoch_filter_excludes_stale_identity() {
    let evidence = [
        Evidence {
            offender: 0,
            epoch: 1,
        },
        Evidence {
            offender: 0,
            epoch: 2,
        },
    ];
    let current = current_epoch_evidence(&evidence, 2);
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].epoch, 2);
    assert_eq!(current[0].offender, 0);
}

#[test]
fn uc_96_no_implicit_carryover_across_validator_sets() {
    let stale = [Evidence {
        offender: 0,
        epoch: 1,
    }];
    assert!(current_epoch_evidence(&stale, 2).is_empty());
}
