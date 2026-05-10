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
