use std::cell::Cell;

use super::*;

fn payer(byte: u8) -> Sig { Sig::Ground(vec![byte; 33]) }

fn payer_region(payer: &Sig, entropy: &[u8]) -> CostRegion {
    cost_region(
        &sig_to_cost_signature(payer).expect("ground payer converts"),
        entropy,
        0,
    )
    .expect("payer region")
}

fn seal(regions: Vec<CostRegion>) -> CostAuthority {
    canonical_authority(&CostAuthority { regions }).expect("canonical seal")
}

fn residue_seal(payer: &Sig, entropy: &[u8], deploy_id: [u8; 32]) -> CostAuthority {
    seal(vec![system_residue_region(
        &payer_region(payer, entropy),
        &deploy_id,
    )
    .expect("residue region")])
}

#[test]
fn a_unit_payer_keeps_its_region() {
    let genesis = payer_region(&Sig::Unit, b"genesis-entropy");
    assert_eq!(
        system_residue_region(&genesis, &[7; 32]).expect("unit region"),
        genesis
    );
}

#[test]
fn a_residue_region_is_unit_signed_and_bound_to_payer_region_and_deployment() {
    let paid = payer_region(&payer(1), b"entropy");
    let residue = system_residue_region(&paid, &[7; 32]).expect("residue region");
    assert!(is_unit_cost_signature(
        residue.signature.as_ref().expect("residue signature")
    ));
    let mut preimage = SYSTEM_RESIDUE_DOMAIN.to_vec();
    preimage.extend_from_slice(&[7; 32]);
    preimage.extend_from_slice(&paid.instance_id);
    assert_eq!(residue.instance_id, Blake2b256::hash(preimage));
    assert_ne!(
        system_residue_region(&paid, &[8; 32])
            .expect("other deployment")
            .instance_id,
        residue.instance_id
    );
}

#[test]
fn residue_resolves_to_the_payer_region_only_inside_its_deployment() {
    let entropy = b"entropy".as_slice();
    let stored = residue_seal(&payer(1), entropy, [7; 32]);
    let same = ResidueContext::new(&payer(1), [7; 32]).expect("context");
    assert_eq!(
        resolve_system_residue(&stored, entropy, &same)
            .expect("resolution")
            .into_owned(),
        seal(vec![payer_region(&payer(1), entropy)])
    );
    for other in [
        ResidueContext::new(&payer(1), [8; 32]).expect("later deployment"),
        ResidueContext::new(&payer(2), [7; 32]).expect("other payer"),
        ResidueContext::system(),
    ] {
        assert_eq!(
            resolve_system_residue(&stored, entropy, &other)
                .expect("no resolution")
                .as_ref(),
            &stored
        );
    }
    assert_eq!(
        resolve_system_residue(&stored, b"other-entropy", &same)
            .expect("other entropy")
            .as_ref(),
        &stored
    );
}

#[test]
fn genesis_regions_and_signed_regions_never_resolve() {
    let genesis = payer_region(&Sig::Unit, b"entropy");
    let signed = payer_region(&payer(3), b"entropy");
    let stored = seal(vec![genesis, signed]);
    let context = ResidueContext::new(&payer(3), [7; 32]).expect("context");
    assert_eq!(
        resolve_system_residue(&stored, b"entropy", &context)
            .expect("resolution")
            .as_ref(),
        &stored
    );
}

#[test]
fn only_the_matching_region_of_a_mixed_seal_resolves() {
    let entropy = b"entropy".as_slice();
    let residue =
        system_residue_region(&payer_region(&payer(1), entropy), &[7; 32]).expect("residue region");
    let genesis = payer_region(&Sig::Unit, b"genesis");
    let stored = seal(vec![residue, genesis.clone()]);
    let context = ResidueContext::new(&payer(1), [7; 32]).expect("context");
    assert_eq!(
        resolve_system_residue(&stored, entropy, &context)
            .expect("resolution")
            .into_owned(),
        seal(vec![payer_region(&payer(1), entropy), genesis])
    );
}

#[test]
fn system_seals_are_exactly_the_nonempty_all_unit_seals() {
    let unit = payer_region(&Sig::Unit, b"a");
    let residue =
        system_residue_region(&payer_region(&payer(1), b"b"), &[7; 32]).expect("residue region");
    let ground = payer_region(&payer(1), b"c");
    assert!(!is_system_seal(&CostAuthority::default()));
    assert!(is_system_seal(&seal(vec![unit.clone()])));
    assert!(is_system_seal(&seal(vec![unit.clone(), residue])));
    assert!(!is_system_seal(&seal(vec![unit, ground.clone()])));
    assert!(!is_system_seal(&seal(vec![ground])));
}

#[test]
fn residue_has_demand_only_inside_its_deployment() {
    let entropy = b"entropy".as_slice();
    let stored = residue_seal(&payer(1), entropy, [7; 32]);
    let later = ResidueContext::new(&payer(2), [8; 32]).expect("context");
    assert!(authority_demand(
        resolve_system_residue(&stored, entropy, &later)
            .expect("no resolution")
            .as_ref()
    )
    .expect("demand")
    .0
    .is_empty());
    let same = ResidueContext::new(&payer(1), [7; 32]).expect("context");
    assert_eq!(
        authority_demand(
            resolve_system_residue(&stored, entropy, &same)
                .expect("resolution")
                .as_ref()
        )
        .expect("demand"),
        authority_demand(&seal(vec![payer_region(&payer(1), entropy)])).expect("payer demand")
    );
}

#[test]
fn metered_resolution_matches_and_rejects_before_hashing() {
    let entropy = b"entropy".as_slice();
    let stored = residue_seal(&payer(1), entropy, [7; 32]);
    let unbounded = |_: usize, _: usize, _: usize| Ok(());
    for context in [
        ResidueContext::new(&payer(1), [7; 32]).expect("same deployment"),
        ResidueContext::new(&payer(1), [8; 32]).expect("later deployment"),
        ResidueContext::system(),
    ] {
        assert_eq!(
            resolve_system_residue_metered(&stored, entropy, &context, &unbounded)
                .expect("metered resolution"),
            resolve_system_residue(&stored, entropy, &context).expect("resolution")
        );
    }
    let calls = Cell::new(0_usize);
    let exhausted = |_: usize, _: usize, _: usize| {
        calls.set(calls.get() + 1);
        Err(BackingError::Rejected)
    };
    assert_eq!(
        resolve_system_residue_metered(
            &stored,
            entropy,
            &ResidueContext::new(&payer(1), [7; 32]).expect("context"),
            &exhausted,
        ),
        Err(AuthorityError::HostWorkRejected)
    );
    assert_eq!(calls.get(), 1);
}
