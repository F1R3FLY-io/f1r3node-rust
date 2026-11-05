use casper::rust::safety::clique_oracle::FtThreshold;
use casper::rust::v6::handoff::{handoff_hash, verify_handoff, Certification, HandoffError, HandoffRecord};
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{ETuple, Expr, Par};
use models::rust::par_map::ParMap;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set::ParSet;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use prost::bytes::Bytes;
use prost::Message;

const TWO_THIRDS: FtThreshold = FtThreshold { num: 2, den: 3 };

fn expr(e: ExprInstance) -> Par {
    Par::default().with_exprs(vec![Expr { expr_instance: Some(e) }])
}

fn key(n: u8) -> Bytes { Bytes::from(vec![n; 33]) }

fn record(previous: &[u8], active: &[u8], bonds: &[(u8, i64)]) -> Vec<u8> {
    let set = ParSet::create_from_vec(
        active.iter().map(|n| expr(ExprInstance::GByteArray(key(*n).to_vec()))).collect(),
    );
    let map = ParMap::create_from_vec(
        bonds
            .iter()
            .map(|(n, s)| (expr(ExprInstance::GByteArray(key(*n).to_vec())), expr(ExprInstance::GInt(*s))))
            .collect(),
    );
    expr(ExprInstance::ETupleBody(ETuple {
        ps: vec![
            expr(ExprInstance::GByteArray(previous.to_vec())),
            expr(ExprInstance::ESetBody(ParSetTypeMapper::par_set_to_eset(set))),
            expr(ExprInstance::EMapBody(ParMapTypeMapper::par_map_to_emap(map))),
        ],
        ..Default::default()
    }))
    .encode_to_vec()
}

fn certify(signers: &[u8], next: &[u8]) -> Vec<Certification> {
    signers
        .iter()
        .map(|n| Certification { validator: key(*n), handoff: handoff_hash(next) })
        .collect()
}

fn held() -> Vec<u8> { record(&[], &[1, 2, 3, 4], &[(1, 10), (2, 10), (3, 10), (4, 10), (5, 1_000)]) }

#[test]
fn a_record_decodes_to_its_committee_of_active_keys_with_their_stakes() {
    let decoded = HandoffRecord::decode(&held()).unwrap();
    assert!(decoded.previous.is_empty());
    let committee = decoded.committee();
    assert_eq!(committee.len(), 4, "a bonded key outside the active set is not in the committee");
    assert!(committee.iter().all(|b| b.stake == 10));
}

#[test]
fn more_than_two_thirds_of_the_held_stake_hands_off_to_the_next_record() {
    let held = held();
    let next = record(&handoff_hash(&held), &[2, 3, 5], &[(2, 10), (3, 10), (5, 1_000)]);
    let accepted = verify_handoff(&held, &next, &certify(&[1, 2, 3], &next), TWO_THIRDS).unwrap();
    assert_eq!(accepted, HandoffRecord::decode(&next).unwrap());
}

#[test]
fn exactly_two_thirds_or_less_is_refused() {
    let held = record(&[], &[1, 2, 3], &[(1, 10), (2, 10), (3, 10)]);
    let next = record(&handoff_hash(&held), &[1], &[(1, 10)]);
    assert_eq!(
        verify_handoff(&held, &next, &certify(&[1, 2], &next), TWO_THIRDS),
        Err(HandoffError::BelowThreshold { certified: 20, total: 30 })
    );
}

#[test]
fn signers_outside_the_held_committee_and_repeated_signers_add_nothing() {
    let held = held();
    let next = record(&handoff_hash(&held), &[5], &[(5, 1_000)]);
    let mut certifications = certify(&[1, 2, 5, 6], &next);
    certifications.extend(certify(&[2, 2], &next));
    assert_eq!(
        verify_handoff(&held, &next, &certifications, TWO_THIRDS),
        Err(HandoffError::BelowThreshold { certified: 20, total: 40 })
    );
}

#[test]
fn a_certification_of_another_record_does_not_count() {
    let held = held();
    let next = record(&handoff_hash(&held), &[1], &[(1, 10)]);
    let other = record(&handoff_hash(&held), &[5], &[(5, 1_000)]);
    let mut certifications = certify(&[1, 2], &next);
    certifications.extend(certify(&[3, 4], &other));
    assert!(matches!(
        verify_handoff(&held, &next, &certifications, TWO_THIRDS),
        Err(HandoffError::BelowThreshold { .. })
    ));
}

#[test]
fn a_record_that_skips_its_predecessor_is_refused() {
    let held = held();
    let between = record(&handoff_hash(&held), &[1, 2, 3, 4], &[(1, 10), (2, 10), (3, 10), (4, 10)]);
    let skipped = record(&handoff_hash(&between), &[1], &[(1, 10)]);
    assert_eq!(
        verify_handoff(&held, &skipped, &certify(&[1, 2, 3, 4], &skipped), TWO_THIRDS),
        Err(HandoffError::NotLinked)
    );
}

#[test]
fn bytes_that_are_not_a_record_are_refused() {
    let held = held();
    let not_a_record = expr(ExprInstance::GInt(7)).encode_to_vec();
    assert!(matches!(
        verify_handoff(&held, &not_a_record, &[], TWO_THIRDS),
        Err(HandoffError::Malformed(_))
    ));
}
